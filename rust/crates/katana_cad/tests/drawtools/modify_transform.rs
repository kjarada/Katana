// The Modify > Transform tools (tools/modify_transform.rs;
// tests/cad/tools/test_modify_transform.cpp): Move, Copy, Rotate, Scale,
// Mirror, Stretch, the two Arrays and Erase, driven as a user drives them -
// clicks, typed values, options, Undo, Enter - and checked against geometry
// worked out by hand in the comments.
//
// Inputs are chosen exact in binary wherever the operation allows (moves,
// scales by powers of two, mirrors about the axes), so those results are
// compared exactly. A rotation goes through cos and sin, which are not exact
// at 90 degrees, so rotated results are compared to within 1e-12.

use std::cmp::Ordering;

use katana_cad::ToolFeedback;
use katana_cad::interactive_tool::{ToolInput, ToolStepOutcome as Outcome, tool_catalog};
use katana_commands::{
    CommandPtr, EntityAttributes, create_arc, create_circle, create_dimension, create_entities,
    create_line, create_point, create_polyline, create_text,
};
use katana_entity::leader_values::leader_note;
use katana_entity::{
    AnchorRef, DimensionGeometry, Entity, EntityId, Geometry, LeaderContext, LeaderGeometry,
    TextGeometry,
};
use katana_geometry::{Arc2, Circle2, Point2, Segment2};
use katana_math::numerics::PI;

use crate::tool_driver::ToolDriver;
use crate::{near, near_point, point, polyline, segment};

const NEAR: f64 = 1e-12;

fn add_line(driver: &mut ToolDriver, a: Point2, b: Point2) -> EntityId {
    driver.add(create_line(a, b, EntityAttributes::default()))
}

fn select(driver: &mut ToolDriver, ids: Vec<EntityId>) {
    driver.document_mut().selection_mut().set(ids);
}

fn entity_of(driver: &ToolDriver, id: EntityId) -> Option<&Entity> {
    driver.document().model().entities.find(id)
}

#[track_caller]
fn line_of(driver: &ToolDriver, id: EntityId) -> Segment2 {
    match entity_of(driver, id).map(|entity| &entity.geometry) {
        Some(Geometry::Line(line)) => *line,
        _ => panic!("entity {id} is not a line"),
    }
}

#[track_caller]
fn geometry_of(driver: &ToolDriver, id: EntityId) -> Geometry {
    match entity_of(driver, id) {
        Some(entity) => entity.geometry.clone(),
        None => panic!("entity {id} is not in the drawing"),
    }
}

// Every line in the drawing, in id order - the order they were made in.
fn all_lines(driver: &ToolDriver) -> Vec<Segment2> {
    crate::entities_of(driver.document())
        .into_iter()
        .filter_map(|entity| match entity.geometry {
            Geometry::Line(line) => Some(line),
            _ => None,
        })
        .collect()
}

fn all_points(driver: &ToolDriver) -> Vec<Point2> {
    crate::entities_of(driver.document())
        .into_iter()
        .filter_map(|entity| match entity.geometry {
            Geometry::Point(point) => Some(point.position),
            _ => None,
        })
        .collect()
}

fn entity_count(driver: &ToolDriver) -> usize {
    driver.document().model().entities.size()
}

// The line shapes of a preview, which is where the transformed selection is.
fn preview_lines(feedback: &ToolFeedback) -> Vec<Segment2> {
    feedback
        .shapes
        .iter()
        .filter_map(|shape| match shape {
            Geometry::Line(line) => Some(*line),
            _ => None,
        })
        .collect()
}

// A driver with one line selected, the usual start of a Modify tool.
fn one_line(a: Point2, b: Point2, tool: &str) -> (ToolDriver, EntityId) {
    let mut driver = ToolDriver::new();
    let id = add_line(&mut driver, a, b);
    select(&mut driver, vec![id]);
    driver.start(tool);
    (driver, id)
}

fn last_message(driver: &ToolDriver) -> &str {
    driver
        .messages()
        .last()
        .map_or("", |message| message.as_str())
}

// The starts of every line, bottom row first and left to right along a row.
fn sorted_starts(driver: &ToolDriver) -> Vec<Point2> {
    let mut starts: Vec<Point2> = all_lines(driver).iter().map(|line| line.start).collect();
    starts.sort_by(|a, b| {
        let by = if a.y != b.y {
            a.y.partial_cmp(&b.y)
        } else {
            a.x.partial_cmp(&b.x)
        };
        by.unwrap_or(Ordering::Equal)
    });
    starts
}

// ---- the catalogue -----------------------------------------------------------------

mod modify_transform_tools {
    use super::*;

    #[test]
    fn every_tool_is_in_the_modify_menus_transform_group_under_its_auto_cad_aliases() {
        let catalog = tool_catalog();
        let aliases = [
            ("MOVE", "modify.move"),
            ("M", "modify.move"),
            ("COPY", "modify.copy"),
            ("CO", "modify.copy"),
            ("CP", "modify.copy"),
            ("ROTATE", "modify.rotate"),
            ("RO", "modify.rotate"),
            ("SCALE", "modify.scale"),
            ("SC", "modify.scale"),
            ("MIRROR", "modify.mirror"),
            ("MI", "modify.mirror"),
            ("STRETCH", "modify.stretch"),
            ("S", "modify.stretch"),
            ("ARRAY", "modify.array_rectangular"),
            ("AR", "modify.array_rectangular"),
            ("ARRAYRECT", "modify.array_rectangular"),
            ("ARRAYPOLAR", "modify.array_polar"),
            ("ERASE", "modify.erase"),
            ("E", "modify.erase"),
            ("DELETE", "modify.erase"),
            ("DEL", "modify.erase"),
        ];
        for (verb, id) in aliases {
            let Some(tool) = catalog.find_by_alias(verb) else {
                panic!("{verb}");
            };
            assert_eq!(tool.id, id, "{verb}");
            assert_eq!(tool.category, "Modify");
            assert_eq!(tool.group, "Transform");
            assert!(!tool.tip.is_empty(), "{id}");
        }
    }

    // ---- asking for a selection ------------------------------------------------------

    #[test]
    fn with_nothing_selected_a_tool_asks_for_a_selection_and_refuses_an_empty_one() {
        let mut driver = ToolDriver::new();
        let id = add_line(&mut driver, point(0.0, 0.0), point(1.0, 0.0));
        driver.start("modify.move");
        assert_eq!(driver.expects(), ToolInput::Selection);
        assert_eq!(
            driver.prompt(),
            "Select entities to move or [All], then press Enter"
        );

        assert_eq!(driver.enter().outcome, Outcome::Rejected);
        assert_eq!(driver.expects(), ToolInput::Selection);

        assert_eq!(driver.pick(id, 0.5, 0.0).outcome, Outcome::Continue);
        assert_eq!(driver.enter().outcome, Outcome::Continue);
        assert_eq!(driver.expects(), ToolInput::Point);
        assert_eq!(last_message(&driver), "1 entity selected");
        driver.click(0.0, 0.0);
        driver.click(2.0, 0.0);
        assert_eq!(line_of(&driver, id), segment([2.0, 0.0], [3.0, 0.0]));
    }

    #[test]
    fn enter_takes_what_the_view_selected_in_the_document_while_the_tool_asked() {
        let mut driver = ToolDriver::new();
        let id = add_line(&mut driver, point(0.0, 0.0), point(1.0, 0.0));
        driver.start("modify.move");
        // The view selects as the Select tool does, into the document.
        select(&mut driver, vec![id]);
        assert_eq!(driver.enter().outcome, Outcome::Continue);
        assert_eq!(driver.expects(), ToolInput::Point);
    }

    #[test]
    fn all_selects_everything_and_undo_takes_the_picks_back_one_at_a_time() {
        let mut driver = ToolDriver::new();
        let a = add_line(&mut driver, point(0.0, 0.0), point(1.0, 0.0));
        let b = add_line(&mut driver, point(0.0, 1.0), point(1.0, 1.0));
        driver.start("modify.erase");
        assert_eq!(driver.pick(a, 0.5, 0.0).outcome, Outcome::Continue);
        assert_eq!(
            driver.pick(a, 0.5, 0.0).outcome,
            Outcome::Rejected,
            "already selected"
        );
        assert_eq!(driver.undo().outcome, Outcome::Continue);
        assert_eq!(
            driver.undo().outcome,
            Outcome::Rejected,
            "nothing left to undo"
        );
        assert_eq!(
            driver.enter().outcome,
            Outcome::Rejected,
            "the pick was undone"
        );

        assert_eq!(driver.r#type("all").outcome, Outcome::Continue);
        assert_eq!(last_message(&driver), "2 entities selected");
        assert_eq!(driver.enter().outcome, Outcome::Done);
        assert_eq!(entity_count(&driver), 0);
        assert!(entity_of(&driver, b).is_none());
    }

    #[test]
    fn undo_at_the_first_step_goes_back_to_the_selection_the_tool_asked_for() {
        let mut driver = ToolDriver::new();
        let id = add_line(&mut driver, point(0.0, 0.0), point(1.0, 0.0));
        driver.start("modify.rotate");
        driver.pick(id, 0.5, 0.0);
        driver.enter();
        assert_eq!(driver.prompt(), "Specify base point");
        assert_eq!(driver.undo().outcome, Outcome::Continue);
        assert_eq!(driver.expects(), ToolInput::Selection);
        // The pick is kept, so Enter goes straight on.
        assert_eq!(driver.enter().outcome, Outcome::Continue);
        assert_eq!(driver.prompt(), "Specify base point");
    }

    #[test]
    fn undo_at_the_first_step_of_a_given_selection_has_nothing_to_undo() {
        let (mut driver, _) = one_line(point(0.0, 0.0), point(1.0, 0.0), "modify.move");
        assert_eq!(driver.undo().outcome, Outcome::Rejected);
    }
}

// ---- Move ----------------------------------------------------------------------------

mod move_tool {
    use super::*;

    #[test]
    fn moves_the_selection_from_the_base_point_to_the_second_point() {
        let (mut driver, id) = one_line(point(1.0, 2.0), point(3.0, 2.0), "modify.move");
        assert_eq!(
            driver.prompt(),
            "Specify base point or [Displacement] <Displacement>"
        );
        assert_eq!(driver.click(0.0, 0.0).outcome, Outcome::Continue);
        assert_eq!(
            driver.prompt(),
            "Specify second point or <use first point as displacement>"
        );
        assert_eq!(driver.click(10.0, 5.0).outcome, Outcome::Done);
        // (1,2)-(3,2) + (10,5) = (11,7)-(13,7).
        assert_eq!(line_of(&driver, id), segment([11.0, 7.0], [13.0, 7.0]));
        assert_eq!(last_message(&driver), "1 entity moved");
        assert!(
            driver.finished(),
            "a move needs a fresh selection, so it does not restart"
        );
    }

    #[test]
    fn takes_a_typed_displacement_after_the_displacement_option_or_enter() {
        let (mut driver, id) = one_line(point(1.0, 2.0), point(3.0, 2.0), "modify.move");
        assert_eq!(driver.r#type("d").outcome, Outcome::Continue);
        assert_eq!(driver.prompt(), "Specify displacement as dx,dy");
        assert_eq!(driver.r#type("10,5").outcome, Outcome::Done);
        assert_eq!(line_of(&driver, id), segment([11.0, 7.0], [13.0, 7.0]));

        // Enter at the base point is the <Displacement> default, and a
        // relative displacement there is relative to nothing, i.e. the same
        // numbers.
        select(&mut driver, vec![id]);
        driver.start("modify.move");
        assert_eq!(driver.enter().outcome, Outcome::Continue);
        assert_eq!(
            driver.enter().outcome,
            Outcome::Rejected,
            "the displacement has no default"
        );
        assert_eq!(driver.r#type("@-10,-5").outcome, Outcome::Done);
        assert_eq!(line_of(&driver, id), segment([1.0, 2.0], [3.0, 2.0]));
    }

    #[test]
    fn takes_the_second_point_relative_or_polar_from_the_base_point() {
        let (mut driver, id) = one_line(point(1.0, 2.0), point(3.0, 2.0), "modify.move");
        driver.r#type("2,3");
        assert_eq!(driver.r#type("@4,-1").outcome, Outcome::Done);
        // Displacement (4,-1): (5,1)-(7,1).
        assert_eq!(line_of(&driver, id), segment([5.0, 1.0], [7.0, 1.0]));

        select(&mut driver, vec![id]);
        driver.start("modify.move");
        driver.click(0.0, 0.0);
        assert_eq!(driver.r#type("@5<90").outcome, Outcome::Done);
        // 5 at 90 degrees is (0,5): (5,6)-(7,6), x through cos(90 deg), not
        // exact.
        let moved = line_of(&driver, id);
        near_point(moved.start, point(5.0, 6.0), NEAR);
        near_point(moved.end, point(7.0, 6.0), NEAR);
    }

    #[test]
    fn a_relative_point_with_a_dz_moves_in_plan_as_it_always_did() {
        // Move's points have no height to change: the dz of @dx,dy,dz is
        // dropped, as the z of x,y,z is. The dz rule of 2026-09-30 (a change of
        // the last point's height, refused where there is none) is for the
        // tools that take heights, and applied to every tool it refused
        // "@0,5,0" here.
        for typed in ["@0,5,0", "@0,5,1"] {
            let (mut driver, id) = one_line(point(1.0, 2.0), point(3.0, 2.0), "modify.move");
            assert_eq!(driver.r#type("0,0").outcome, Outcome::Continue);
            let step = driver.r#type(typed);
            assert_eq!(step.outcome, Outcome::Done, "{typed}: {}", step.message);
            assert_eq!(
                line_of(&driver, id),
                segment([1.0, 7.0], [3.0, 7.0]),
                "{typed}"
            );
        }
    }

    #[test]
    fn enter_at_the_second_point_uses_the_base_point_as_the_displacement() {
        let (mut driver, id) = one_line(point(1.0, 2.0), point(3.0, 2.0), "modify.move");
        driver.click(4.0, 1.0);
        assert_eq!(driver.enter().outcome, Outcome::Done);
        assert_eq!(line_of(&driver, id), segment([5.0, 3.0], [7.0, 3.0]));
    }

    #[test]
    fn a_typed_distance_goes_that_far_towards_the_cursor() {
        let (mut driver, id) = one_line(point(1.0, 2.0), point(3.0, 2.0), "modify.move");
        driver.click(0.0, 0.0);
        assert_eq!(
            driver.r#type("10").outcome,
            Outcome::Rejected,
            "no cursor has been seen yet"
        );
        // The cursor at (3,4) is the direction (0.6, 0.8); 10 along it is
        // (6,8).
        driver.preview(3.0, 4.0);
        assert_eq!(driver.r#type("10").outcome, Outcome::Done);
        let moved = line_of(&driver, id);
        near_point(moved.start, point(7.0, 10.0), NEAR);
        near_point(moved.end, point(9.0, 10.0), NEAR);
    }

    #[test]
    fn refuses_a_zero_displacement_and_stays_at_the_second_point() {
        let (mut driver, id) = one_line(point(1.0, 2.0), point(3.0, 2.0), "modify.move");
        driver.click(1.0, 1.0);
        let refused = driver.click(1.0, 1.0);
        assert_eq!(refused.outcome, Outcome::Rejected);
        assert!(!refused.message.is_empty());
        assert_eq!(
            driver.prompt(),
            "Specify second point or <use first point as displacement>"
        );
        assert_eq!(driver.r#type("banana").outcome, Outcome::Rejected);
        assert_eq!(driver.click(2.0, 1.0).outcome, Outcome::Done);
        assert_eq!(line_of(&driver, id), segment([2.0, 2.0], [4.0, 2.0]));
    }

    #[test]
    fn undo_steps_back_to_the_base_point() {
        let (mut driver, id) = one_line(point(1.0, 2.0), point(3.0, 2.0), "modify.move");
        driver.click(5.0, 5.0);
        assert_eq!(driver.undo().outcome, Outcome::Continue);
        assert_eq!(
            driver.prompt(),
            "Specify base point or [Displacement] <Displacement>"
        );
        driver.click(0.0, 0.0);
        driver.click(1.0, 0.0);
        assert_eq!(line_of(&driver, id), segment([2.0, 2.0], [4.0, 2.0]));
    }

    #[test]
    fn one_undo_of_the_document_puts_the_whole_selection_back() {
        let mut driver = ToolDriver::new();
        let a = add_line(&mut driver, point(0.0, 0.0), point(1.0, 0.0));
        let b = add_line(&mut driver, point(0.0, 1.0), point(1.0, 1.0));
        select(&mut driver, vec![a, b]);
        driver.start("modify.move");
        driver.click(0.0, 0.0);
        driver.click(3.0, 0.0);
        assert_eq!(last_message(&driver), "2 entities moved");
        assert_eq!(driver.executed(), 1);
        crate::ok(driver.document_mut().undo());
        assert_eq!(line_of(&driver, a), segment([0.0, 0.0], [1.0, 0.0]));
        assert_eq!(line_of(&driver, b), segment([0.0, 1.0], [1.0, 1.0]));
    }

    #[test]
    fn previews_the_selection_moved_to_the_cursor_with_the_band_from_the_base_point() {
        let (mut driver, _) = one_line(point(1.0, 2.0), point(3.0, 2.0), "modify.move");
        assert!(
            driver.preview(5.0, 0.0).shapes.is_empty(),
            "no base point yet"
        );
        driver.click(0.0, 0.0);
        let feedback = driver.preview(5.0, 0.0);
        let lines = preview_lines(&feedback);
        assert!(lines.contains(&segment([6.0, 2.0], [8.0, 2.0])));
        assert!(lines.contains(&segment([0.0, 0.0], [5.0, 0.0])));
        assert_eq!(feedback.markers, [point(0.0, 0.0)]);
    }
}

// ---- Copy ----------------------------------------------------------------------------

mod copy_tool {
    use super::*;

    #[test]
    fn copies_to_every_second_point_until_enter_as_one_command() {
        let (mut driver, _) = one_line(point(0.0, 0.0), point(1.0, 0.0), "modify.copy");
        driver.click(0.0, 0.0);
        assert_eq!(driver.click(0.0, 2.0).outcome, Outcome::Continue);
        assert_eq!(
            driver.prompt(),
            "Specify second point or [Exit/Undo] <Exit>"
        );
        // Relative to the base point, as every second point is: (0,0) + (0,4).
        assert_eq!(driver.r#type("@0,4").outcome, Outcome::Continue);
        assert_eq!(driver.executed(), 0, "nothing is made until Enter");
        assert_eq!(driver.enter().outcome, Outcome::Done);
        assert_eq!(driver.executed(), 1);
        assert_eq!(last_message(&driver), "2 copies of 1 entity");
        let expected = [
            segment([0.0, 0.0], [1.0, 0.0]),
            segment([0.0, 2.0], [1.0, 2.0]),
            segment([0.0, 4.0], [1.0, 4.0]),
        ];
        assert_eq!(all_lines(&driver), expected);

        crate::ok(driver.document_mut().undo());
        assert_eq!(all_lines(&driver), [expected[0]], "one undo removes both");
    }

    #[test]
    fn undo_removes_the_last_copy_placed_and_the_undo_option_does_the_same() {
        let (mut driver, _) = one_line(point(0.0, 0.0), point(1.0, 0.0), "modify.copy");
        driver.click(0.0, 0.0);
        driver.click(0.0, 2.0);
        driver.click(0.0, 4.0);
        driver.click(0.0, 6.0);
        assert_eq!(driver.undo().outcome, Outcome::Continue);
        assert_eq!(driver.r#type("U").outcome, Outcome::Continue);
        assert_eq!(driver.r#type("exit").outcome, Outcome::Done);
        assert_eq!(
            all_lines(&driver),
            [
                segment([0.0, 0.0], [1.0, 0.0]),
                segment([0.0, 2.0], [1.0, 2.0])
            ]
        );
    }

    #[test]
    fn enter_before_any_copy_uses_the_base_point_as_the_displacement() {
        let (mut driver, _) = one_line(point(0.0, 0.0), point(1.0, 0.0), "modify.copy");
        driver.click(3.0, 0.5);
        assert_eq!(driver.enter().outcome, Outcome::Done);
        assert_eq!(
            all_lines(&driver),
            [
                segment([0.0, 0.0], [1.0, 0.0]),
                segment([3.0, 0.5], [4.0, 0.5])
            ]
        );
    }

    #[test]
    fn takes_a_typed_displacement_and_a_typed_distance_towards_the_cursor() {
        let (mut driver, id) = one_line(point(0.0, 0.0), point(1.0, 0.0), "modify.copy");
        driver.r#type("D");
        assert_eq!(driver.r#type("5,0").outcome, Outcome::Done);
        assert_eq!(
            all_lines(&driver).last().copied(),
            Some(segment([5.0, 0.0], [6.0, 0.0]))
        );

        select(&mut driver, vec![id]);
        driver.start("modify.copy");
        driver.click(0.0, 0.0);
        driver.preview(0.0, -7.0); // straight down: exact
        assert_eq!(driver.r#type("2").outcome, Outcome::Continue);
        driver.enter();
        assert_eq!(
            all_lines(&driver).last().copied(),
            Some(segment([0.0, -2.0], [1.0, -2.0]))
        );
    }

    #[test]
    fn refuses_a_copy_onto_the_original_and_exit_with_no_copies_makes_nothing() {
        let (mut driver, _) = one_line(point(0.0, 0.0), point(1.0, 0.0), "modify.copy");
        // A typed displacement of nothing puts the copy on the original.
        driver.r#type("D");
        assert_eq!(
            driver.r#type("0,0").outcome,
            Outcome::Rejected,
            "a typed displacement of zero"
        );
        driver.undo();
        // With the base point at the origin, Enter's "use first point as
        // displacement" is a displacement of nothing too.
        driver.click(0.0, 0.0);
        assert_eq!(
            driver.enter().outcome,
            Outcome::Rejected,
            "the base point is the origin"
        );
        driver.undo();
        driver.click(1.0, 1.0);
        assert_eq!(driver.click(1.0, 1.0).outcome, Outcome::Rejected);
        assert_eq!(driver.r#type("E").outcome, Outcome::Done);
        assert_eq!(driver.executed(), 0);
        assert_eq!(entity_count(&driver), 1);
    }

    #[test]
    fn previews_the_copies_placed_so_far_and_the_one_at_the_cursor() {
        let (mut driver, _) = one_line(point(0.0, 0.0), point(1.0, 0.0), "modify.copy");
        driver.click(0.0, 0.0);
        driver.click(0.0, 2.0);
        let lines = preview_lines(&driver.preview(0.0, 5.0));
        assert!(
            lines.contains(&segment([0.0, 2.0], [1.0, 2.0])),
            "the placed copy"
        );
        assert!(
            lines.contains(&segment([0.0, 5.0], [1.0, 5.0])),
            "the copy at the cursor"
        );
        assert!(lines.contains(&segment([0.0, 0.0], [0.0, 5.0])), "the band");
    }
}

// ---- Rotate --------------------------------------------------------------------------

mod rotate_tool {
    use super::*;

    #[test]
    fn rotates_by_a_typed_angle_counter_clockwise_about_the_base_point() {
        let (mut driver, id) = one_line(point(2.0, 0.0), point(4.0, 0.0), "modify.rotate");
        driver.click(0.0, 0.0);
        assert_eq!(
            driver.prompt(),
            "Specify rotation angle or [Copy/Reference]"
        );
        assert_eq!(driver.r#type("90").outcome, Outcome::Done);
        // (2,0) and (4,0) turned a quarter counter-clockwise about the origin.
        let turned = line_of(&driver, id);
        near_point(turned.start, point(0.0, 2.0), NEAR);
        near_point(turned.end, point(0.0, 4.0), NEAR);
        assert_eq!(last_message(&driver), "1 entity rotated");
        crate::ok(driver.document_mut().undo());
        assert_eq!(line_of(&driver, id), segment([2.0, 0.0], [4.0, 0.0]));
    }

    #[test]
    fn a_picked_point_gives_the_angle_of_its_direction_from_the_base_point() {
        let (mut driver, id) = one_line(point(2.0, 0.0), point(4.0, 0.0), "modify.rotate");
        driver.click(1.0, 0.0);
        // (1,-3) is straight down from the base: -90 degrees. (2,0) is 1 east
        // of the base and goes to 1 south of it, (1,-1); (4,0) goes to (1,-3).
        assert_eq!(driver.r#type("@0,-3").outcome, Outcome::Done);
        let turned = line_of(&driver, id);
        near_point(turned.start, point(1.0, -1.0), NEAR);
        near_point(turned.end, point(1.0, -3.0), NEAR);
    }

    #[test]
    fn reference_turns_from_the_current_angle_to_the_new_one() {
        // A line at 45 degrees made to run due north: a turn of 45 degrees
        // about the origin takes (1,1) to (0, sqrt 2).
        let root2 = 2.0_f64.sqrt();
        {
            let (mut driver, id) = one_line(point(0.0, 0.0), point(1.0, 1.0), "modify.rotate");
            driver.click(0.0, 0.0);
            assert_eq!(driver.r#type("r").outcome, Outcome::Continue);
            assert_eq!(
                driver.prompt(),
                "Specify the reference angle or its first point <0>"
            );
            driver.click(0.0, 0.0);
            driver.click(1.0, 1.0);
            assert_eq!(driver.prompt(), "Specify the new angle or [Points]");
            assert_eq!(driver.r#type("90").outcome, Outcome::Done);
            near_point(line_of(&driver, id).end, point(0.0, root2), NEAR);
        }
        {
            // The same with the reference typed and the new angle given by
            // Points.
            let (mut driver, id) = one_line(point(0.0, 0.0), point(1.0, 1.0), "modify.rotate");
            driver.click(0.0, 0.0);
            driver.r#type("REFERENCE");
            driver.r#type("45");
            assert_eq!(driver.r#type("p").outcome, Outcome::Continue);
            driver.click(5.0, 5.0);
            assert_eq!(driver.click(5.0, 6.0).outcome, Outcome::Done);
            near_point(line_of(&driver, id).end, point(0.0, root2), NEAR);
        }
        {
            // Enter takes the <0> reference, so the new angle is the turn
            // itself.
            let (mut driver, id) = one_line(point(0.0, 0.0), point(1.0, 1.0), "modify.rotate");
            driver.click(0.0, 0.0);
            driver.r#type("R");
            assert_eq!(driver.enter().outcome, Outcome::Continue);
            assert_eq!(
                driver.click(-3.0, 3.0).outcome,
                Outcome::Done,
                "135 degrees from the base"
            );
            // (1,1) turned by 135 degrees: its angle becomes 180, so
            // (-sqrt 2, 0).
            near_point(line_of(&driver, id).end, point(-root2, 0.0), NEAR);
        }
    }

    #[test]
    fn the_copy_option_rotates_a_copy_and_leaves_the_original() {
        let (mut driver, _) = one_line(point(2.0, 0.0), point(4.0, 0.0), "modify.rotate");
        driver.click(0.0, 0.0);
        assert_eq!(driver.r#type("C").outcome, Outcome::Continue);
        assert_eq!(last_message(&driver), "Rotating a copy of the selection.");
        assert_eq!(driver.r#type("180").outcome, Outcome::Done);
        let lines = all_lines(&driver);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], segment([2.0, 0.0], [4.0, 0.0]));
        near_point(lines[1].start, point(-2.0, 0.0), NEAR);
        near_point(lines[1].end, point(-4.0, 0.0), NEAR);
        crate::ok(driver.document_mut().undo());
        assert_eq!(entity_count(&driver), 1);
    }

    #[test]
    fn refuses_no_turn_a_point_with_no_direction_and_a_collapsed_reference_or_new_angle() {
        let (mut driver, id) = one_line(point(2.0, 0.0), point(4.0, 0.0), "modify.rotate");
        driver.click(1.0, 1.0);
        assert_eq!(driver.r#type("0").outcome, Outcome::Rejected);
        assert_eq!(
            driver.r#type("360").outcome,
            Outcome::Rejected,
            "a whole turn changes nothing"
        );
        assert_eq!(
            driver.click(1.0, 1.0).outcome,
            Outcome::Rejected,
            "on the base point"
        );
        assert_eq!(driver.r#type("north").outcome, Outcome::Rejected);
        assert_eq!(
            driver.enter().outcome,
            Outcome::Rejected,
            "the angle has no default"
        );
        driver.r#type("R");
        driver.click(3.0, 3.0);
        assert_eq!(
            driver.click(3.0, 3.0).outcome,
            Outcome::Rejected,
            "reference points coincide"
        );
        assert_eq!(driver.executed(), 0);
        assert_eq!(
            driver.prompt(),
            "Specify second point of the reference angle"
        );

        // A reference of 30 degrees, then a new angle with no direction. Taken
        // as due east, either would turn the selection by -30 degrees unasked.
        driver.undo();
        assert_eq!(driver.r#type("30").outcome, Outcome::Continue);
        assert_eq!(
            driver.click(1.0, 1.0).outcome,
            Outcome::Rejected,
            "new angle on the base point"
        );
        assert_eq!(driver.r#type("P").outcome, Outcome::Continue);
        driver.click(3.0, 3.0);
        assert_eq!(
            driver.click(3.0, 3.0).outcome,
            Outcome::Rejected,
            "new angle points coincide"
        );
        assert_eq!(driver.executed(), 0);
        assert_eq!(driver.prompt(), "Specify second point of the new angle");
        assert_eq!(line_of(&driver, id), segment([2.0, 0.0], [4.0, 0.0]));
    }

    #[test]
    fn undo_steps_back_through_the_reference_dialogue() {
        let (mut driver, _) = one_line(point(2.0, 0.0), point(4.0, 0.0), "modify.rotate");
        driver.click(0.0, 0.0);
        driver.r#type("R");
        driver.r#type("30");
        assert_eq!(driver.prompt(), "Specify the new angle or [Points]");
        driver.undo();
        assert_eq!(
            driver.prompt(),
            "Specify the reference angle or its first point <0>"
        );
        driver.undo();
        assert_eq!(
            driver.prompt(),
            "Specify rotation angle or [Copy/Reference]"
        );
        driver.undo();
        assert_eq!(driver.prompt(), "Specify base point");
    }

    #[test]
    fn previews_the_selection_turned_to_the_cursor() {
        let (mut driver, _) = one_line(point(2.0, 0.0), point(4.0, 0.0), "modify.rotate");
        driver.click(0.0, 0.0);
        let lines = preview_lines(&driver.preview(0.0, 5.0));
        assert_eq!(lines.len(), 2, "the turned line and the band");
        near_point(lines[0].start, point(0.0, 2.0), NEAR);
        near_point(lines[0].end, point(0.0, 4.0), NEAR);
        assert_eq!(lines[1], segment([0.0, 0.0], [0.0, 5.0]));
    }
}

// ---- Scale ---------------------------------------------------------------------------

mod scale_tool {
    use super::*;

    #[test]
    fn scales_by_a_typed_factor_about_the_base_point() {
        let (mut driver, id) = one_line(point(1.0, 1.0), point(2.0, 1.0), "modify.scale");
        driver.click(0.0, 0.0);
        assert_eq!(driver.prompt(), "Specify scale factor or [Copy/Reference]");
        assert_eq!(driver.r#type("2").outcome, Outcome::Done);
        assert_eq!(line_of(&driver, id), segment([2.0, 2.0], [4.0, 2.0]));
        assert_eq!(last_message(&driver), "1 entity scaled");
        crate::ok(driver.document_mut().undo());
        assert_eq!(line_of(&driver, id), segment([1.0, 1.0], [2.0, 1.0]));
    }

    #[test]
    fn a_picked_point_gives_its_distance_from_the_base_point_as_the_factor() {
        let (mut driver, id) = one_line(point(1.0, 1.0), point(2.0, 1.0), "modify.scale");
        driver.click(0.0, 0.0);
        // (3,4) is 5 from the origin: the classic 3-4-5 triangle.
        assert_eq!(driver.click(3.0, 4.0).outcome, Outcome::Done);
        assert_eq!(line_of(&driver, id), segment([5.0, 5.0], [10.0, 5.0]));
    }

    #[test]
    fn reference_scales_by_the_ratio_of_the_new_length_to_the_current_one() {
        {
            let (mut driver, id) = one_line(point(1.0, 1.0), point(2.0, 1.0), "modify.scale");
            driver.click(0.0, 0.0);
            driver.r#type("R");
            driver.r#type("4");
            assert_eq!(driver.prompt(), "Specify new length or [Points]");
            assert_eq!(driver.r#type("10").outcome, Outcome::Done);
            // 10 / 4 = 2.5: (2.5,2.5)-(5,2.5).
            assert_eq!(line_of(&driver, id), segment([2.5, 2.5], [5.0, 2.5]));
        }
        {
            // Both lengths picked: 4 up the y axis now, 8 along x wanted -
            // factor 2.
            let (mut driver, id) = one_line(point(1.0, 1.0), point(2.0, 1.0), "modify.scale");
            driver.click(0.0, 0.0);
            driver.r#type("reference");
            driver.click(0.0, 0.0);
            driver.click(0.0, 4.0);
            driver.r#type("P");
            driver.click(0.0, 0.0);
            assert_eq!(driver.click(8.0, 0.0).outcome, Outcome::Done);
            assert_eq!(line_of(&driver, id), segment([2.0, 2.0], [4.0, 2.0]));
        }
        {
            // Enter takes the <1> reference; the new length is then the
            // factor.
            let (mut driver, id) = one_line(point(1.0, 1.0), point(2.0, 1.0), "modify.scale");
            driver.click(0.0, 0.0);
            driver.r#type("R");
            driver.enter();
            assert_eq!(driver.r#type("3").outcome, Outcome::Done);
            assert_eq!(line_of(&driver, id), segment([3.0, 3.0], [6.0, 3.0]));
        }
    }

    #[test]
    fn the_copy_option_scales_a_copy_and_leaves_the_original() {
        let (mut driver, _) = one_line(point(1.0, 1.0), point(2.0, 1.0), "modify.scale");
        driver.click(0.0, 0.0);
        driver.r#type("copy");
        assert_eq!(driver.r#type("0.5").outcome, Outcome::Done);
        assert_eq!(
            all_lines(&driver),
            [
                segment([1.0, 1.0], [2.0, 1.0]),
                segment([0.5, 0.5], [1.0, 0.5])
            ]
        );
    }

    #[test]
    fn refuses_a_zero_negative_or_unit_factor_and_a_zero_reference() {
        let (mut driver, id) = one_line(point(1.0, 1.0), point(2.0, 1.0), "modify.scale");
        driver.click(2.0, 2.0);
        assert_eq!(driver.r#type("0").outcome, Outcome::Rejected);
        assert_eq!(driver.r#type("-2").outcome, Outcome::Rejected);
        assert_eq!(
            driver.r#type("1").outcome,
            Outcome::Rejected,
            "a factor of 1 changes nothing"
        );
        assert_eq!(
            driver.click(2.0, 2.0).outcome,
            Outcome::Rejected,
            "the base point: factor 0"
        );
        driver.r#type("R");
        assert_eq!(driver.r#type("0").outcome, Outcome::Rejected);
        driver.click(5.0, 5.0);
        assert_eq!(
            driver.click(5.0, 5.0).outcome,
            Outcome::Rejected,
            "a reference of length 0"
        );
        driver.undo();
        driver.r#type("2");
        assert_eq!(
            driver.r#type("0").outcome,
            Outcome::Rejected,
            "a new length of 0"
        );
        assert_eq!(driver.executed(), 0);
        assert_eq!(line_of(&driver, id), segment([1.0, 1.0], [2.0, 1.0]));
    }

    #[test]
    fn previews_the_selection_scaled_by_the_cursors_distance() {
        let (mut driver, _) = one_line(point(1.0, 1.0), point(2.0, 1.0), "modify.scale");
        driver.click(0.0, 0.0);
        let lines = preview_lines(&driver.preview(0.0, 2.0));
        assert!(lines.contains(&segment([2.0, 2.0], [4.0, 2.0])));
    }
}

// Every tool that copies copies a callout with its pit as a callout of the
// pit's copy - the tip on the copy, the note read off it - not a second
// callout of the original (docs/annotation.md, "Associativity"). The tools
// once built their copies apart from COPY and missed this.
mod copying_tools {
    use super::*;

    #[test]
    fn a_callout_copied_with_its_pit_reads_the_pits_copy() {
        // The pit at (10,0). Copy by (100,0); rotate a copy half a turn and
        // scale one by 2 about the origin; four polar items a quarter-turn
        // apart, the original counted. "" is Enter.
        let cases: [(&str, &[&str], Vec<Point2>); 4] = [
            (
                "modify.copy",
                &["0,0", "@100,0", ""][..],
                vec![point(110.0, 0.0)],
            ),
            (
                "modify.rotate",
                &["0,0", "C", "180"][..],
                vec![point(-10.0, 0.0)],
            ),
            (
                "modify.scale",
                &["0,0", "copy", "2"][..],
                vec![point(20.0, 0.0)],
            ),
            (
                "modify.array_polar",
                &["0,0", "4", "", ""][..],
                vec![point(0.0, 10.0), point(-10.0, 0.0), point(0.0, -10.0)],
            ),
        ];
        for (tool, steps, pits) in cases {
            let mut driver = ToolDriver::new();
            let pit = driver.add(create_point(point(10.0, 0.0), EntityAttributes::default()));
            let callout = Entity {
                geometry: Geometry::Leader(LeaderGeometry {
                    vertices: vec![point(10.0, 0.0), point(15.0, 5.0)],
                    text: String::from("PIT {id}"),
                    tip_ref: AnchorRef {
                        entity: pit,
                        ..AnchorRef::default()
                    },
                    fields: true,
                    ..LeaderGeometry::default()
                }),
                ..Entity::default()
            };
            let leader = driver.add(create_entities(vec![callout]));
            select(&mut driver, vec![pit, leader]);
            driver.start(tool);
            for &step in steps {
                if step.is_empty() {
                    driver.enter();
                } else {
                    driver.r#type(step);
                }
            }
            let made = driver.document().last_created_entities();
            assert_eq!(made.len(), 2 * pits.len(), "{tool}");
            for (k, &pit_at) in pits.iter().enumerate() {
                // Copied in selection order, a set per placement: pit, callout.
                let pit_copy = made[2 * k];
                let Geometry::Leader(copy) = geometry_of(&driver, made[2 * k + 1]) else {
                    panic!("{tool}: the copy is not a leader");
                };
                assert_eq!(copy.tip_ref.entity, pit_copy, "{tool}");
                near_point(copy.vertices[0], pit_at, NEAR);
                assert_eq!(
                    leader_note(
                        driver.document().model(),
                        &copy,
                        &[],
                        &LeaderContext::default()
                    ),
                    format!("PIT {pit_copy}"),
                    "{tool}"
                );
            }
            let Geometry::Leader(original) = geometry_of(&driver, leader) else {
                panic!("{tool}: the original is not a leader");
            };
            assert_eq!(original.tip_ref.entity, pit, "{tool}");
            assert_eq!(original.vertices[0], point(10.0, 0.0), "{tool}");
        }
    }
}

// ---- Mirror --------------------------------------------------------------------------

mod mirror_tool {
    use super::*;

    #[test]
    fn mirrors_about_the_line_through_two_points_keeping_the_source_by_default() {
        let (mut driver, _) = one_line(point(1.0, 0.0), point(2.0, 1.0), "modify.mirror");
        assert_eq!(driver.prompt(), "Specify first point of mirror line");
        driver.click(0.0, 0.0);
        driver.click(0.0, 1.0);
        assert_eq!(driver.prompt(), "Erase source entities? [Yes/No] <No>");
        assert_eq!(driver.expects(), ToolInput::Value);
        assert_eq!(driver.enter().outcome, Outcome::Done);
        // About the y axis x changes sign: (1,0)-(2,1) becomes (-1,0)-(-2,1).
        assert_eq!(
            all_lines(&driver),
            [
                segment([1.0, 0.0], [2.0, 1.0]),
                segment([-1.0, 0.0], [-2.0, 1.0])
            ]
        );
        crate::ok(driver.document_mut().undo());
        assert_eq!(entity_count(&driver), 1);
    }

    #[test]
    fn yes_erases_the_source_and_no_keeps_it() {
        {
            let (mut driver, _) = one_line(point(1.0, 0.0), point(2.0, 1.0), "modify.mirror");
            driver.click(0.0, 0.0);
            driver.click(1.0, 0.0);
            assert_eq!(driver.r#type("y").outcome, Outcome::Done);
            // About the x axis y changes sign.
            assert_eq!(all_lines(&driver), [segment([1.0, 0.0], [2.0, -1.0])]);
        }
        {
            let (mut driver, _) = one_line(point(1.0, 0.0), point(2.0, 1.0), "modify.mirror");
            driver.click(0.0, 0.0);
            driver.click(1.0, 0.0);
            assert_eq!(driver.r#type("maybe").outcome, Outcome::Rejected);
            assert_eq!(driver.r#type("NO").outcome, Outcome::Done);
            assert_eq!(entity_count(&driver), 2);
        }
    }

    #[test]
    fn refuses_a_mirror_line_of_one_point_and_undo_steps_back() {
        let (mut driver, _) = one_line(point(1.0, 0.0), point(2.0, 1.0), "modify.mirror");
        driver.click(3.0, 3.0);
        assert_eq!(driver.click(3.0, 3.0).outcome, Outcome::Rejected);
        assert_eq!(driver.prompt(), "Specify second point of mirror line");
        driver.click(3.0, 4.0);
        assert_eq!(driver.undo().outcome, Outcome::Continue);
        assert_eq!(driver.prompt(), "Specify second point of mirror line");
        assert_eq!(driver.executed(), 0);
    }

    #[test]
    fn previews_the_mirror_image_for_the_cursor_as_the_second_point() {
        let (mut driver, _) = one_line(point(1.0, 0.0), point(2.0, 1.0), "modify.mirror");
        driver.click(0.0, 0.0);
        let lines = preview_lines(&driver.preview(0.0, 5.0));
        assert!(lines.contains(&segment([-1.0, 0.0], [-2.0, 1.0])));
        assert!(
            lines.contains(&segment([0.0, 0.0], [0.0, 5.0])),
            "the mirror line"
        );
    }
}

// ---- Rectangular array ---------------------------------------------------------------

mod array_rectangular_tool {
    use super::*;

    #[test]
    fn makes_rows_and_columns_at_the_typed_spacing() {
        let (mut driver, _) =
            one_line(point(0.0, 0.0), point(1.0, 0.0), "modify.array_rectangular");
        assert_eq!(driver.prompt(), "Enter the number of rows <1>");
        assert_eq!(driver.expects(), ToolInput::Value);
        driver.r#type("2");
        driver.r#type("3");
        assert_eq!(
            driver.prompt(),
            "Enter the distance between rows or specify unit cell"
        );
        driver.r#type("5");
        assert_eq!(driver.r#type("2").outcome, Outcome::Done);
        assert_eq!(last_message(&driver), "2 x 3 array of 1 entity");
        // Columns 2 apart along x, rows 5 apart along y; the original is
        // (0,0).
        assert_eq!(
            sorted_starts(&driver),
            [
                point(0.0, 0.0),
                point(2.0, 0.0),
                point(4.0, 0.0),
                point(0.0, 5.0),
                point(2.0, 5.0),
                point(4.0, 5.0)
            ]
        );
        crate::ok(driver.document_mut().undo());
        assert_eq!(entity_count(&driver), 1);
    }

    #[test]
    fn a_unit_cell_gives_both_spacings_at_once() {
        let (mut driver, _) =
            one_line(point(0.0, 0.0), point(1.0, 0.0), "modify.array_rectangular");
        driver.r#type("2");
        driver.r#type("3");
        driver.click(10.0, 10.0);
        // The cell from (10,10) to (12,15): 2 across, 5 up.
        let lines = preview_lines(&driver.preview(12.0, 15.0));
        assert_eq!(lines.len(), 5, "the five copies");
        assert!(lines.contains(&segment([4.0, 5.0], [5.0, 5.0])));
        // The opposite corner typed relative to the first: the same cell.
        assert_eq!(driver.r#type("@2,5").outcome, Outcome::Done);
        assert_eq!(
            sorted_starts(&driver),
            [
                point(0.0, 0.0),
                point(2.0, 0.0),
                point(4.0, 0.0),
                point(0.0, 5.0),
                point(2.0, 5.0),
                point(4.0, 5.0)
            ]
        );
    }

    #[test]
    fn one_row_asks_only_for_the_columns_and_one_column_only_for_the_rows() {
        {
            let (mut driver, _) =
                one_line(point(0.0, 0.0), point(1.0, 0.0), "modify.array_rectangular");
            driver.enter(); // <1> row
            driver.r#type("4");
            assert_eq!(
                driver.prompt(),
                "Enter the distance between columns or its first point"
            );
            // Two points 3 apart across the columns; the rise between them is
            // not part of it.
            driver.click(1.0, 1.0);
            assert_eq!(driver.click(4.0, 7.0).outcome, Outcome::Done);
            assert_eq!(
                sorted_starts(&driver),
                [
                    point(0.0, 0.0),
                    point(3.0, 0.0),
                    point(6.0, 0.0),
                    point(9.0, 0.0)
                ]
            );
        }
        {
            let (mut driver, _) =
                one_line(point(0.0, 0.0), point(1.0, 0.0), "modify.array_rectangular");
            driver.r#type("3");
            driver.enter(); // <1> column
            assert_eq!(
                driver.r#type("-2").outcome,
                Outcome::Done,
                "a negative spacing goes down"
            );
            assert_eq!(
                sorted_starts(&driver),
                [point(0.0, -4.0), point(0.0, -2.0), point(0.0, 0.0)]
            );
        }
    }

    #[test]
    fn refuses_bad_counts_one_cell_a_zero_spacing_a_flat_unit_cell_and_level_column_points() {
        {
            // Two points straight above one another are 0 apart across the
            // columns: every column would land on the first.
            let (mut driver, _) =
                one_line(point(0.0, 0.0), point(1.0, 0.0), "modify.array_rectangular");
            driver.enter(); // <1> row
            driver.r#type("2");
            driver.click(1.0, 1.0);
            assert_eq!(
                driver.click(1.0, 5.0).outcome,
                Outcome::Rejected,
                "level across the columns"
            );
            assert_eq!(
                driver.prompt(),
                "Specify second point of the distance between columns"
            );
            assert_eq!(driver.executed(), 0);
        }
        let (mut driver, _) =
            one_line(point(0.0, 0.0), point(1.0, 0.0), "modify.array_rectangular");
        assert_eq!(
            driver.r#type("100001").outcome,
            Outcome::Rejected,
            "100001 rows is over the limit"
        );
        assert_eq!(driver.r#type("0").outcome, Outcome::Rejected);
        assert_eq!(driver.r#type("2.5").outcome, Outcome::Rejected);
        assert_eq!(driver.r#type("rows").outcome, Outcome::Rejected);
        driver.enter();
        assert_eq!(
            driver.enter().outcome,
            Outcome::Rejected,
            "1 x 1 is the original alone"
        );
        driver.undo();
        driver.r#type("1000");
        assert_eq!(
            driver.r#type("101").outcome,
            Outcome::Rejected,
            "101000 items is over the limit"
        );
        driver.r#type("2");
        assert_eq!(
            driver.r#type("0").outcome,
            Outcome::Rejected,
            "a spacing of zero"
        );
        driver.click(0.0, 0.0);
        assert_eq!(
            driver.click(3.0, 0.0).outcome,
            Outcome::Rejected,
            "a cell with no height"
        );
        assert_eq!(
            driver.click(0.0, 3.0).outcome,
            Outcome::Rejected,
            "a cell with no width"
        );
        assert_eq!(driver.executed(), 0);
    }

    #[test]
    fn refuses_a_column_count_so_large_that_rows_times_columns_would_overflow() {
        let (mut driver, _) =
            one_line(point(0.0, 0.0), point(1.0, 0.0), "modify.array_rectangular");
        // 2^63 = 9223372036854775808, and 3074457345618258603 is 2^63 / 3
        // rounded up, so 3 rows of it are 2^63 + 1 items - one past the
        // largest int64. Multiplied in int64 that wraps round to a negative
        // count, which is "under" the limit of 100000.
        driver.r#type("3");
        assert_eq!(
            driver.r#type("3074457345618258603").outcome,
            Outcome::Rejected
        );
        assert_eq!(driver.prompt(), "Enter the number of columns <1>");
        // 2 rows of the largest int64, 2^63 - 1, are 2^64 - 2: -2 once
        // wrapped.
        driver.undo();
        driver.r#type("2");
        assert_eq!(
            driver.r#type("9223372036854775807").outcome,
            Outcome::Rejected
        );
        assert_eq!(driver.prompt(), "Enter the number of columns <1>");
        assert_eq!(driver.executed(), 0);
        assert_eq!(entity_count(&driver), 1);
    }
}

// ---- Polar array ---------------------------------------------------------------------

mod array_polar_tool {
    use super::*;

    fn one_point(driver: &mut ToolDriver) {
        let id = driver.add(create_point(point(10.0, 0.0), EntityAttributes::default()));
        select(driver, vec![id]);
    }

    #[test]
    fn spaces_the_items_evenly_round_a_whole_turn_counting_the_original() {
        let mut driver = ToolDriver::new();
        one_point(&mut driver);
        driver.start("modify.array_polar");
        assert_eq!(driver.prompt(), "Specify center point of array");
        assert_eq!(driver.r#type("0,0").outcome, Outcome::Continue);
        assert_eq!(driver.expects(), ToolInput::Value);
        driver.r#type("4");
        assert_eq!(
            driver.prompt(),
            "Specify the angle to fill (+=ccw, -=cw) <360>"
        );
        driver.enter();
        assert_eq!(driver.prompt(), "Rotate arrayed entities? [Yes/No] <Yes>");
        assert_eq!(driver.enter().outcome, Outcome::Done);
        // Four items in 360 degrees are 90 apart: (10,0), (0,10), (-10,0),
        // (0,-10).
        let placed = all_points(&driver);
        assert_eq!(placed.len(), 4);
        assert_eq!(placed[0], point(10.0, 0.0));
        near_point(placed[1], point(0.0, 10.0), NEAR);
        near_point(placed[2], point(-10.0, 0.0), NEAR);
        near_point(placed[3], point(0.0, -10.0), NEAR);
        crate::ok(driver.document_mut().undo());
        assert_eq!(entity_count(&driver), 1);
    }

    #[test]
    fn a_part_turn_puts_the_first_and_last_items_at_its_ends_and_negative_is_clockwise() {
        {
            let mut driver = ToolDriver::new();
            one_point(&mut driver);
            driver.start("modify.array_polar");
            driver.click(0.0, 0.0);
            driver.r#type("3");
            driver.r#type("180");
            driver.r#type("Y");
            // Three items over 180 degrees are 90 apart: the last at 180.
            let placed = all_points(&driver);
            assert_eq!(placed.len(), 3);
            near_point(placed[1], point(0.0, 10.0), NEAR);
            near_point(placed[2], point(-10.0, 0.0), NEAR);
        }
        {
            let mut driver = ToolDriver::new();
            one_point(&mut driver);
            driver.start("modify.array_polar");
            driver.click(0.0, 0.0);
            driver.r#type("2");
            driver.r#type("-90");
            driver.enter();
            let placed = all_points(&driver);
            assert_eq!(placed.len(), 2);
            near_point(placed[1], point(0.0, -10.0), NEAR);
        }
    }

    #[test]
    fn items_that_do_not_turn_are_carried_round_by_the_middle_of_the_selection() {
        // A line from (9,0) to (11,0), its middle at (10,0), a quarter turn
        // round the origin: turned, it stands on end at (0,9)-(0,11); carried
        // without turning, it lies level about (0,10), at (-1,10)-(1,10).
        let quarter = |answer: &str| {
            let (mut driver, _) = one_line(point(9.0, 0.0), point(11.0, 0.0), "modify.array_polar");
            driver.click(0.0, 0.0);
            driver.r#type("4");
            driver.enter();
            assert_eq!(driver.r#type(answer).outcome, Outcome::Done);
            all_lines(&driver)[1]
        };
        let turned = quarter("yes");
        near_point(turned.start, point(0.0, 9.0), NEAR);
        near_point(turned.end, point(0.0, 11.0), NEAR);
        let carried = quarter("n");
        near_point(carried.start, point(-1.0, 10.0), NEAR);
        near_point(carried.end, point(1.0, 10.0), NEAR);
    }

    #[test]
    fn refuses_too_few_items_too_many_no_angle_too_much_angle_and_an_unclear_answer() {
        let (mut driver, _) = one_line(point(9.0, 0.0), point(11.0, 0.0), "modify.array_polar");
        driver.click(0.0, 0.0);
        assert_eq!(
            driver.r#type("1").outcome,
            Outcome::Rejected,
            "the original alone"
        );
        assert_eq!(
            driver.r#type("100001").outcome,
            Outcome::Rejected,
            "over the limit of 100000"
        );
        assert_eq!(driver.r#type("six").outcome, Outcome::Rejected);
        assert_eq!(
            driver.enter().outcome,
            Outcome::Rejected,
            "the count has no default"
        );
        driver.r#type("6");
        assert_eq!(driver.r#type("0").outcome, Outcome::Rejected);
        assert_eq!(driver.r#type("400").outcome, Outcome::Rejected);
        driver.r#type("90");
        assert_eq!(driver.r#type("perhaps").outcome, Outcome::Rejected);
        assert_eq!(driver.undo().outcome, Outcome::Continue);
        assert_eq!(
            driver.prompt(),
            "Specify the angle to fill (+=ccw, -=cw) <360>"
        );
        assert_eq!(driver.executed(), 0);
    }

    #[test]
    fn previews_the_whole_turn_the_default_would_make() {
        let (mut driver, _) = one_line(point(9.0, 0.0), point(11.0, 0.0), "modify.array_polar");
        driver.click(0.0, 0.0);
        driver.r#type("4");
        let feedback = driver.preview(50.0, 50.0);
        assert_eq!(preview_lines(&feedback).len(), 3, "the three copies");
        assert_eq!(feedback.markers, [point(0.0, 0.0)]);
    }
}

// ---- Erase ---------------------------------------------------------------------------

mod erase_tool {
    use super::*;

    #[test]
    fn one_enter_erases_the_selection_it_started_with_and_one_undo_brings_it_back() {
        let mut driver = ToolDriver::new();
        let a = add_line(&mut driver, point(0.0, 0.0), point(1.0, 0.0));
        let b = add_line(&mut driver, point(0.0, 1.0), point(1.0, 1.0));
        let kept = add_line(&mut driver, point(0.0, 2.0), point(1.0, 2.0));
        select(&mut driver, vec![a, b]);
        driver.start("modify.erase");
        assert_eq!(
            driver.prompt(),
            "Select entities to erase or [All], then press Enter"
        );
        assert_eq!(driver.enter().outcome, Outcome::Done);
        assert_eq!(last_message(&driver), "2 entities erased");
        assert_eq!(driver.executed(), 1);
        assert_eq!(entity_count(&driver), 1);
        assert!(entity_of(&driver, kept).is_some());
        crate::ok(driver.document_mut().undo());
        assert_eq!(entity_count(&driver), 3);
    }

    #[test]
    fn a_selection_changed_at_its_prompt_is_what_enter_erases() {
        let mut driver = ToolDriver::new();
        let a = add_line(&mut driver, point(0.0, 0.0), point(1.0, 0.0));
        let b = add_line(&mut driver, point(0.0, 1.0), point(1.0, 1.0));
        let c = add_line(&mut driver, point(0.0, 2.0), point(1.0, 2.0));
        {
            // At "Select entities to erase" the view selects as the Select tool
            // does: a plain click on C replaces A and B with C, and C alone is
            // highlighted - so C alone is erased.
            select(&mut driver, vec![a, b]);
            driver.start("modify.erase");
            select(&mut driver, vec![c]);
            assert_eq!(driver.enter().outcome, Outcome::Done);
            assert_eq!(last_message(&driver), "1 entity erased");
            assert!(entity_of(&driver, a).is_some());
            assert!(entity_of(&driver, b).is_some());
            assert!(entity_of(&driver, c).is_none());
            crate::ok(driver.document_mut().undo());
        }
        {
            // Cleared there, nothing is highlighted and nothing is erased.
            select(&mut driver, vec![a, b]);
            driver.start("modify.erase");
            select(&mut driver, Vec::new());
            let executed = driver.executed();
            assert_eq!(driver.enter().outcome, Outcome::Rejected);
            assert_eq!(driver.executed(), executed);
            assert_eq!(entity_count(&driver), 3);
        }
    }

    #[test]
    fn with_nothing_selected_enter_is_refused_until_something_is_picked() {
        let mut driver = ToolDriver::new();
        let id = add_line(&mut driver, point(0.0, 0.0), point(1.0, 0.0));
        driver.start("modify.erase");
        assert_eq!(driver.enter().outcome, Outcome::Rejected);
        assert_eq!(
            driver.click(0.5, 0.0).outcome,
            Outcome::Rejected,
            "a click is not a pick"
        );
        driver.pick(id, 0.5, 0.0);
        assert_eq!(driver.enter().outcome, Outcome::Done);
        assert_eq!(entity_count(&driver), 0);
    }
}

// ---- Stretch -------------------------------------------------------------------------

mod stretch_tool {
    use super::*;

    #[test]
    fn moves_the_points_inside_the_window_and_what_lies_wholly_within_it() {
        let mut driver = ToolDriver::new();
        let rectangle = driver.add(create_polyline(
            polyline(&[[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [0.0, 5.0]], true),
            EntityAttributes::default(),
        ));
        let inside = add_line(&mut driver, point(9.0, 2.0), point(11.0, 2.0));
        let across = add_line(&mut driver, point(7.0, 3.0), point(13.0, 3.0));
        let ring = driver.add(create_circle(
            point(20.0, 20.0),
            1.0,
            EntityAttributes::default(),
        ));
        driver.start("modify.stretch");
        assert_eq!(
            driver.expects(),
            ToolInput::Point,
            "the window is the selection"
        );
        assert_eq!(
            driver.prompt(),
            "Specify first corner of the crossing window"
        );
        driver.click(12.0, 6.0);
        assert_eq!(driver.click(8.0, -1.0).outcome, Outcome::Continue);
        assert_eq!(last_message(&driver), "2 entities to stretch");
        driver.click(0.0, 0.0);
        assert_eq!(driver.r#type("@3,0").outcome, Outcome::Done);
        assert_eq!(driver.executed(), 1);

        // The window x 8..12, y -1..6 holds the rectangle's right-hand corners
        // and the whole short line; each moves 3 to the right.
        assert_eq!(
            geometry_of(&driver, rectangle),
            Geometry::Polyline(polyline(
                &[[0.0, 0.0], [13.0, 0.0], [13.0, 5.0], [0.0, 5.0]],
                true
            ))
        );
        assert_eq!(line_of(&driver, inside), segment([12.0, 2.0], [14.0, 2.0]));
        // Crossing the window with neither end in it, the long line stays.
        assert_eq!(line_of(&driver, across), segment([7.0, 3.0], [13.0, 3.0]));
        assert_eq!(
            geometry_of(&driver, ring),
            Geometry::Circle(Circle2::new(point(20.0, 20.0), 1.0))
        );

        crate::ok(driver.document_mut().undo());
        assert_eq!(
            geometry_of(&driver, rectangle),
            Geometry::Polyline(polyline(
                &[[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [0.0, 5.0]],
                true
            ))
        );
        assert_eq!(line_of(&driver, inside), segment([9.0, 2.0], [11.0, 2.0]));
    }

    #[test]
    fn an_arc_with_one_end_inside_keeps_its_height_above_the_chord() {
        let mut driver = ToolDriver::new();
        // The upper half of the circle of radius 2 about the origin: ends
        // (2,0) and (-2,0), 2 above its chord at the top.
        let id = driver.add(create_arc(
            Arc2::new(point(0.0, 0.0), 2.0, 0.0, PI),
            EntityAttributes::default(),
        ));
        driver.start("modify.stretch");
        driver.click(1.5, -0.5);
        driver.click(2.5, 0.5);
        driver.click(0.0, 0.0);
        assert_eq!(driver.click(2.0, 0.0).outcome, Outcome::Done);
        // The end at (2,0) goes to (4,0). The new arc runs from (4,0) through
        // (1,2) - the chord's new middle, 2 above - to (-2,0). Its centre
        // (1,k) is as far from (4,0) as from (1,2): 9 + k^2 = (2 - k)^2, so
        // k = -5/4, and the radius is 2 - k = 13/4.
        let Geometry::Arc(stretched) = geometry_of(&driver, id) else {
            panic!("not an arc");
        };
        near_point(stretched.center, point(1.0, -1.25), 1e-9);
        near(stretched.radius, 3.25, 1e-9);
        near_point(stretched.start_point(), point(4.0, 0.0), 1e-9);
        near_point(stretched.end_point(), point(-2.0, 0.0), 1e-9);
        assert!(
            stretched.sweep > 0.0,
            "still counter-clockwise, bulging upwards"
        );
        near_point(stretched.midpoint(), point(1.0, 2.0), 1e-9);
    }

    #[test]
    fn an_arc_wholly_inside_moves_whole_and_a_dimension_moves_only_the_point_inside() {
        let mut driver = ToolDriver::new();
        let bend = driver.add(create_arc(
            Arc2::new(point(2.0, 2.0), 1.0, 0.0, 1.5),
            EntityAttributes::default(),
        ));
        let measured = DimensionGeometry {
            start: point(-10.0, 2.0),
            end: point(2.0, 2.0),
            offset: 1.5,
            ..DimensionGeometry::default()
        };
        let dimension = driver.add(create_dimension(measured, EntityAttributes::default()));
        driver.start("modify.stretch");
        driver.click(0.0, 0.0);
        driver.click(4.0, 4.0);
        assert_eq!(last_message(&driver), "2 entities to stretch");
        driver.click(0.0, 0.0);
        assert_eq!(driver.click(0.5, -1.0).outcome, Outcome::Done);
        // Both ends of the arc are inside, so it moves as it is: centre
        // (2.5,1).
        assert_eq!(
            geometry_of(&driver, bend),
            Geometry::Arc(Arc2::new(point(2.5, 1.0), 1.0, 0.0, 1.5))
        );
        // Only the dimension's end at (2,2) is inside; its start stays put,
        // and it goes on measuring from there, to (2.5,1).
        let Geometry::Dimension(stretched) = geometry_of(&driver, dimension) else {
            panic!("not a dimension");
        };
        assert_eq!(stretched.start, point(-10.0, 2.0));
        assert_eq!(stretched.end, point(2.5, 1.0));
        assert_eq!(stretched.offset, 1.5);
    }

    #[test]
    fn a_circle_a_point_and_a_text_move_when_their_centre_or_position_is_inside() {
        let mut driver = ToolDriver::new();
        // Small enough to lie inside the window: a crossing window catches
        // what it touches, and a circle round the whole window touches none of
        // it.
        let ring = driver.add(create_circle(
            point(1.0, 1.0),
            0.5,
            EntityAttributes::default(),
        ));
        let mark = driver.add(create_point(point(2.0, 1.0), EntityAttributes::default()));
        let label = TextGeometry {
            position: point(1.0, 2.0),
            text: String::from("A"),
            ..TextGeometry::default()
        };
        let text = driver.add(create_text(label, EntityAttributes::default()));
        driver.start("modify.stretch");
        driver.click(0.0, 0.0);
        driver.click(3.0, 3.0);
        driver.r#type("D");
        assert_eq!(driver.r#type("0,4").outcome, Outcome::Done);
        assert_eq!(
            geometry_of(&driver, ring),
            Geometry::Circle(Circle2::new(point(1.0, 5.0), 0.5))
        );
        let Geometry::Point(moved) = geometry_of(&driver, mark) else {
            panic!("not a point");
        };
        assert_eq!(moved.position, point(2.0, 5.0));
        let Geometry::Text(moved) = geometry_of(&driver, text) else {
            panic!("not a text");
        };
        assert_eq!(moved.position, point(1.0, 6.0));
    }

    #[test]
    fn a_selection_made_beforehand_limits_what_the_window_catches() {
        let mut driver = ToolDriver::new();
        let chosen = add_line(&mut driver, point(0.0, 0.0), point(5.0, 0.0));
        let other = add_line(&mut driver, point(0.0, 1.0), point(5.0, 1.0));
        select(&mut driver, vec![chosen]);
        driver.start("modify.stretch");
        driver.click(4.0, -1.0);
        driver.click(6.0, 2.0);
        assert_eq!(last_message(&driver), "1 entity to stretch");
        driver.click(0.0, 0.0);
        driver.click(1.0, 0.0);
        assert_eq!(line_of(&driver, chosen), segment([0.0, 0.0], [6.0, 0.0]));
        assert_eq!(line_of(&driver, other), segment([0.0, 1.0], [5.0, 1.0]));
    }

    #[test]
    fn refuses_a_flat_window_an_empty_one_no_displacement_and_an_arc_whose_ends_would_meet() {
        let mut driver = ToolDriver::new();
        let straight = add_line(&mut driver, point(0.0, 0.0), point(10.0, 0.0));
        let bend = driver.add(create_arc(
            Arc2::new(point(20.0, 0.0), 2.0, 0.0, PI),
            EntityAttributes::default(),
        ));
        driver.start("modify.stretch");
        driver.click(1.0, 1.0);
        assert_eq!(
            driver.click(1.0, 5.0).outcome,
            Outcome::Rejected,
            "no width"
        );
        // The line crosses this window, but neither end is in it.
        assert_eq!(
            driver.click(5.0, -1.0).outcome,
            Outcome::Rejected,
            "nothing to stretch"
        );
        assert_eq!(driver.prompt(), "Specify opposite corner");
        driver.undo();
        // Round the arc's east end (22,0): moving it 4 west puts it on the
        // west end.
        driver.click(21.5, -0.5);
        driver.click(22.5, 0.5);
        driver.click(0.0, 0.0);
        assert_eq!(
            driver.click(0.0, 0.0).outcome,
            Outcome::Rejected,
            "no displacement"
        );
        let collapsed = driver.click(-4.0, 0.0);
        assert_eq!(collapsed.outcome, Outcome::Rejected);
        assert!(collapsed.message.contains("arc"), "{}", collapsed.message);
        assert_eq!(driver.executed(), 0);
        assert_eq!(line_of(&driver, straight), segment([0.0, 0.0], [10.0, 0.0]));
        assert_eq!(
            geometry_of(&driver, bend),
            Geometry::Arc(Arc2::new(point(20.0, 0.0), 2.0, 0.0, PI))
        );
    }

    // The window round the east end (10,0) of a line or open polyline from
    // (0,0), and that end taken to (0,0) - the other end, where a snap puts
    // it: what is left has zero length, which the document would refuse.
    fn collapse(make: CommandPtr) -> Geometry {
        let mut driver = ToolDriver::new();
        let id = driver.add(make);
        let before = geometry_of(&driver, id);
        driver.start("modify.stretch");
        driver.click(9.0, -1.0);
        driver.click(11.0, 1.0);
        driver.click(10.0, 0.0);
        let collapsed = driver.click(0.0, 0.0);
        assert_eq!(collapsed.outcome, Outcome::Rejected);
        assert!(
            collapsed.message.contains("zero length"),
            "{}",
            collapsed.message
        );
        assert_eq!(driver.executed(), 0);
        assert!(!driver.finished());
        assert_eq!(geometry_of(&driver, id), before);
        // The window and base point are kept: the next point finishes it.
        // (10,0) to (4,0) leaves 4 of the 10.
        assert_eq!(
            driver.prompt(),
            "Specify second point or <use first point as displacement>"
        );
        assert_eq!(driver.click(4.0, 0.0).outcome, Outcome::Done);
        assert_eq!(driver.executed(), 1);
        geometry_of(&driver, id)
    }

    #[test]
    fn refuses_a_stretch_that_leaves_a_line_or_polyline_of_zero_length_and_stays_put() {
        assert_eq!(
            collapse(create_line(
                point(0.0, 0.0),
                point(10.0, 0.0),
                EntityAttributes::default()
            )),
            Geometry::Line(segment([0.0, 0.0], [4.0, 0.0]))
        );
        assert_eq!(
            collapse(create_polyline(
                polyline(&[[0.0, 0.0], [10.0, 0.0]], false),
                EntityAttributes::default()
            )),
            Geometry::Polyline(polyline(&[[0.0, 0.0], [4.0, 0.0]], false))
        );
    }

    #[test]
    fn undo_steps_back_and_the_preview_shows_the_stretch_for_the_cursor() {
        let mut driver = ToolDriver::new();
        add_line(&mut driver, point(0.0, 0.0), point(10.0, 0.0));
        driver.start("modify.stretch");
        driver.click(9.0, -1.0);
        let window = driver.preview(11.0, 1.0);
        assert!(!window.shapes.is_empty());
        assert_eq!(
            window.shapes[0],
            Geometry::Polyline(polyline(
                &[[9.0, -1.0], [11.0, -1.0], [11.0, 1.0], [9.0, 1.0]],
                true
            ))
        );
        driver.click(11.0, 1.0);
        driver.click(10.0, 0.0);
        let lines = preview_lines(&driver.preview(12.0, 0.0));
        assert!(
            lines.contains(&segment([0.0, 0.0], [12.0, 0.0])),
            "the stretched line"
        );
        assert_eq!(driver.undo().outcome, Outcome::Continue);
        assert_eq!(
            driver.prompt(),
            "Specify base point or [Displacement] <Displacement>"
        );
        assert_eq!(driver.undo().outcome, Outcome::Continue);
        assert_eq!(driver.prompt(), "Specify opposite corner");
    }
}
