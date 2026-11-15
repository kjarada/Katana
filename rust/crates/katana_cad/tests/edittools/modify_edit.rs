// The Modify > Edit tools (src/katana_cad/tools/modify_edit*.rs;
// tests/cad/tools/test_modify_edit.cpp), driven as the plan view drives them.
// Expected geometry is worked out by hand in the comments; coordinates are
// chosen so that the arithmetic is exact in binary wherever the construction
// allows it (quarters and halves of lengths that are powers of two), and
// compared to a stated tolerance where a square root or a trigonometric
// function is involved.
//
// The family's own session is tested directly below (ModifyEditSession):
// tools/modify_edit_support is internal to the tools, published to the tests
// as a hidden module for it alone.

use katana_cad::ToolFeedback;
use katana_cad::interactive_tool::{ToolInput, tool_catalog};
use katana_cad::tools::modify_edit_support::EditSession;
use katana_commands::{
    EntityAttributes, create_arc, create_circle, create_entities, create_layer, create_line,
    create_polyline, create_text, set_entity_layer, update_layer,
};
use katana_entity::entity::{heights_of, set_heights};
use katana_entity::{Entity, EntityId, Geometry, Layer, TextGeometry};
use katana_geometry::{Arc2, Circle2, Point2, Polyline2, Segment2};
use katana_math::numerics::PI;

use crate::tool_driver::ToolDriver;
use crate::{CONTINUE, DONE, REJECTED, created, entity_of, near, ok};

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn segment(ax: f64, ay: f64, bx: f64, by: f64) -> Segment2 {
    Segment2::new(p(ax, ay), p(bx, by))
}

fn path(points: &[(f64, f64)], closed: bool) -> Polyline2 {
    Polyline2::new(points.iter().map(|&(x, y)| p(x, y)).collect(), closed)
}

fn attributes(driver: &ToolDriver) -> EntityAttributes {
    driver.document().current_attributes()
}

#[track_caller]
fn segment_of(driver: &ToolDriver, id: EntityId) -> Segment2 {
    match entity_of(driver, id).geometry {
        Geometry::Line(line) => line,
        other => panic!("entity {id} is a different kind: {other:?}"),
    }
}

#[track_caller]
fn polyline_of(driver: &ToolDriver, id: EntityId) -> Polyline2 {
    match entity_of(driver, id).geometry {
        Geometry::Polyline(polyline) => polyline,
        other => panic!("entity {id} is a different kind: {other:?}"),
    }
}

#[track_caller]
fn arc_of(driver: &ToolDriver, id: EntityId) -> Arc2 {
    match entity_of(driver, id).geometry {
        Geometry::Arc(arc) => arc,
        other => panic!("entity {id} is a different kind: {other:?}"),
    }
}

#[track_caller]
fn circle_of(driver: &ToolDriver, id: EntityId) -> Circle2 {
    match entity_of(driver, id).geometry {
        Geometry::Circle(circle) => circle,
        other => panic!("entity {id} is a different kind: {other:?}"),
    }
}

fn entity_count(driver: &ToolDriver) -> usize {
    driver.document().model().entities.size()
}

fn line(driver: &mut ToolDriver, a: Point2, b: Point2) -> EntityId {
    let attributes = attributes(driver);
    driver.add(create_line(a, b, attributes))
}

fn polyline(driver: &mut ToolDriver, points: &[(f64, f64)], closed: bool) -> EntityId {
    let attributes = attributes(driver);
    driver.add(create_polyline(path(points, closed), attributes))
}

fn circle(driver: &mut ToolDriver, center: Point2, radius: f64) -> EntityId {
    let attributes = attributes(driver);
    driver.add(create_circle(center, radius, attributes))
}

fn arc(driver: &mut ToolDriver, arc: Arc2) -> EntityId {
    let attributes = attributes(driver);
    driver.add(create_arc(arc, attributes))
}

fn text(driver: &mut ToolDriver, text: TextGeometry) -> EntityId {
    let attributes = attributes(driver);
    driver.add(create_text(text, attributes))
}

fn contains_shape(feedback: &ToolFeedback, shape: &Geometry) -> bool {
    feedback.shapes.iter().any(|drawn| drawn == shape)
}

fn heights(driver: &ToolDriver, id: EntityId, count: usize) -> Vec<Option<f64>> {
    heights_of(&entity_of(driver, id).properties, count)
}

// An entity carrying a height per vertex, as the archive import writes a 3D
// string.
fn with_3d_heights(
    driver: &mut ToolDriver,
    geometry: Geometry,
    vertex_heights: &[Option<f64>],
) -> EntityId {
    let mut entity = Entity {
        geometry,
        ..Entity::default()
    };
    set_heights(&mut entity.properties, vertex_heights);
    driver.add(create_entities(vec![entity]))
}

// Moves `ids` onto a new layer `name` and locks it. The layer is made
// unlocked and locked afterwards: nothing can be moved onto a layer that is
// already locked.
#[track_caller]
fn lock_on_new_layer(driver: &mut ToolDriver, ids: Vec<EntityId>, name: &str) {
    let mut layer = Layer {
        name: String::from(name),
        ..Layer::default()
    };
    ok(driver.document_mut().execute(create_layer(layer.clone())));
    ok(driver.document_mut().execute(set_entity_layer(ids, name)));
    layer.locked = true;
    ok(driver.document_mut().execute(update_layer(layer)));
}

// ============================================================================
// Trim
// ============================================================================

// A line from (0,0) to (8,0) crossed by vertical edges at x = 2 and x = 6.
// Picking at x = 4 removes [2, 6]: the line keeps (0,0)-(2,0) and a new line
// (6,0)-(8,0) is made - parameters 1/4 and 3/4 of 8, exact in binary.
struct TrimScene {
    target: EntityId,
    left: EntityId,
    right: EntityId,
}

fn trim_scene() -> (ToolDriver, TrimScene) {
    let mut driver = ToolDriver::new();
    let target = line(&mut driver, p(0.0, 0.0), p(8.0, 0.0));
    let left = line(&mut driver, p(2.0, -2.0), p(2.0, 2.0));
    let right = line(&mut driver, p(6.0, -2.0), p(6.0, 2.0));
    (
        driver,
        TrimScene {
            target,
            left,
            right,
        },
    )
}

mod modify_edit_trim {
    use super::*;

    #[test]
    fn picked_cutting_edges_cut_the_span_under_the_pick_out_of_a_line() {
        let (mut d, scene) = trim_scene();
        d.start("modify.trim");
        assert_eq!(d.expects(), ToolInput::Selection);
        assert_eq!(d.pick(scene.left, 2.0, 1.0).outcome, CONTINUE);
        assert_eq!(d.pick(scene.right, 6.0, 1.0).outcome, CONTINUE);
        assert_eq!(d.prompt(), "Select cutting edges or [Undo] <use 2 edges>");
        assert_eq!(d.enter().outcome, CONTINUE);
        assert_eq!(d.expects(), ToolInput::Entity);
        assert_eq!(d.prompt(), "Select object to trim or [Undo]");
        assert_eq!(d.pick(scene.target, 4.0, 0.0).outcome, CONTINUE);
        // Nothing reaches the document until the tool finishes.
        assert_eq!(segment_of(&d, scene.target), segment(0.0, 0.0, 8.0, 0.0));
        let finished = d.enter();
        assert_eq!(finished.outcome, DONE);
        assert_eq!(finished.message, "Trimmed 1 part.");
        assert!(d.finished());
        assert_eq!(d.executed(), 1);
        assert_eq!(segment_of(&d, scene.target), segment(0.0, 0.0, 2.0, 0.0));
        assert_eq!(segment_of(&d, created(&d)), segment(6.0, 0.0, 8.0, 0.0));
        assert_eq!(entity_count(&d), 4);
    }

    #[test]
    fn a_preselection_is_offered_as_the_cutting_edges() {
        let (mut d, scene) = trim_scene();
        d.document_mut()
            .selection_mut()
            .set(vec![scene.left, scene.right]);
        d.start("modify.trim");
        assert_eq!(d.prompt(), "Select cutting edges <use 2 edges>");
        assert_eq!(d.enter().outcome, CONTINUE);
        assert_eq!(d.pick(scene.target, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        assert_eq!(segment_of(&d, scene.target), segment(0.0, 0.0, 2.0, 0.0));
    }

    #[test]
    fn enter_at_once_cuts_with_every_object_in_the_drawing() {
        let (mut d, scene) = trim_scene();
        d.start("modify.trim");
        assert_eq!(d.prompt(), "Select cutting edges or <all objects>");
        let all = d.enter();
        assert_eq!(all.outcome, CONTINUE);
        assert_eq!(all.message, "Every object is an edge.");
        assert_eq!(d.pick(scene.target, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        assert_eq!(segment_of(&d, scene.target), segment(0.0, 0.0, 2.0, 0.0));
        assert_eq!(segment_of(&d, created(&d)), segment(6.0, 0.0, 8.0, 0.0));
    }

    #[test]
    fn every_pick_of_one_trim_is_one_undo_of_the_document() {
        // Edges at x = 2, 4 and 6 across (0,0)-(8,0). The pick at x = 3
        // removes [2, 4], leaving (0,0)-(2,0) and a new piece (4,0)-(8,0). The
        // pick at x = 5 lands on that NEW piece, which has no id yet: its only
        // crossing is x = 6 (x = 4 is its own end), so [4, 6] goes and
        // (6,0)-(8,0) is left.
        let mut d = ToolDriver::new();
        let target = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        line(&mut d, p(2.0, -2.0), p(2.0, 2.0));
        line(&mut d, p(4.0, -2.0), p(4.0, 2.0));
        line(&mut d, p(6.0, -2.0), p(6.0, 2.0));
        d.start("modify.trim");
        assert_eq!(d.enter().outcome, CONTINUE);
        assert_eq!(d.pick(target, 3.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.pick(target, 5.0, 0.0).outcome, CONTINUE);
        let finished = d.enter();
        assert_eq!(finished.outcome, DONE);
        assert_eq!(finished.message, "Trimmed 2 parts.");
        assert_eq!(d.executed(), 1);
        assert_eq!(segment_of(&d, target), segment(0.0, 0.0, 2.0, 0.0));
        assert_eq!(segment_of(&d, created(&d)), segment(6.0, 0.0, 8.0, 0.0));
        assert_eq!(entity_count(&d), 5);

        ok(d.document_mut().undo());
        assert_eq!(segment_of(&d, target), segment(0.0, 0.0, 8.0, 0.0));
        assert_eq!(entity_count(&d), 4);
    }

    #[test]
    fn undo_inside_the_tool_takes_back_the_last_pick_only() {
        let mut d = ToolDriver::new();
        let target = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        line(&mut d, p(2.0, -2.0), p(2.0, 2.0));
        line(&mut d, p(4.0, -2.0), p(4.0, 2.0));
        line(&mut d, p(6.0, -2.0), p(6.0, 2.0));
        d.start("modify.trim");
        assert_eq!(d.enter().outcome, CONTINUE);
        assert_eq!(d.pick(target, 3.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.pick(target, 7.0, 0.0).outcome, CONTINUE);
        let undone = d.r#type("u");
        assert_eq!(undone.outcome, CONTINUE);
        assert_eq!(undone.message, "Undid the last trim.");
        assert_eq!(d.enter().outcome, DONE);
        // Only the x = 3 pick is left: (0,0)-(2,0) and (4,0)-(8,0).
        assert_eq!(segment_of(&d, target), segment(0.0, 0.0, 2.0, 0.0));
        assert_eq!(segment_of(&d, created(&d)), segment(4.0, 0.0, 8.0, 0.0));
    }

    #[test]
    fn undo_before_any_trim_goes_back_to_choosing_the_edges() {
        let (mut d, scene) = trim_scene();
        d.start("modify.trim");
        assert_eq!(d.pick(scene.left, 2.0, 1.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, CONTINUE);
        assert_eq!(d.undo().outcome, CONTINUE);
        assert_eq!(d.expects(), ToolInput::Selection);
        // The edge picked before is still picked.
        assert_eq!(d.prompt(), "Select cutting edges or [Undo] <use 1 edge>");
        assert_eq!(d.undo().outcome, CONTINUE);
        assert_eq!(d.prompt(), "Select cutting edges or <all objects>");
        assert_eq!(d.undo().outcome, REJECTED);
    }

    #[test]
    fn a_polyline_loses_the_span_between_the_crossings_either_side_of_the_pick() {
        // (0,0)-(8,0)-(8,8), 16 long. A vertical edge crosses at station 4, a
        // horizontal one at station 8 + 4 = 12. The pick at (8,2), station
        // 10, takes out [4, 12] round the corner: (0,0)-(4,0) is kept as the
        // polyline and (8,4)-(8,8) becomes a new one.
        let mut d = ToolDriver::new();
        let target = polyline(&mut d, &[(0.0, 0.0), (8.0, 0.0), (8.0, 8.0)], false);
        line(&mut d, p(4.0, -2.0), p(4.0, 2.0));
        line(&mut d, p(6.0, 4.0), p(10.0, 4.0));
        d.start("modify.trim");
        assert_eq!(d.enter().outcome, CONTINUE);
        assert_eq!(d.pick(target, 8.0, 2.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        assert_eq!(
            polyline_of(&d, target),
            path(&[(0.0, 0.0), (4.0, 0.0)], false)
        );
        assert_eq!(
            polyline_of(&d, created(&d)),
            path(&[(8.0, 4.0), (8.0, 8.0)], false)
        );
    }

    #[test]
    fn a_closed_polyline_becomes_the_open_polyline_away_from_the_pick() {
        // The square (0,0)-(8,0)-(8,8)-(0,8), 32 round. The vertical edge
        // x = 4 crosses the bottom at station 4 and the top at station
        // 16 + 4 = 20. The pick at (8,4), station 12, removes [4, 20] - the
        // right half - and what is left runs from station 20 on through the
        // first vertex to station 4: (4,8)-(0,8)-(0,0)-(4,0), open.
        let mut d = ToolDriver::new();
        let target = polyline(
            &mut d,
            &[(0.0, 0.0), (8.0, 0.0), (8.0, 8.0), (0.0, 8.0)],
            true,
        );
        line(&mut d, p(4.0, -2.0), p(4.0, 10.0));
        d.start("modify.trim");
        assert_eq!(d.enter().outcome, CONTINUE);
        assert_eq!(d.pick(target, 8.0, 4.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        assert_eq!(
            polyline_of(&d, target),
            path(&[(4.0, 8.0), (0.0, 8.0), (0.0, 0.0), (4.0, 0.0)], false)
        );
        assert_eq!(entity_count(&d), 2);
    }

    #[test]
    fn a_pick_on_the_span_through_the_first_vertex_of_a_closed_polyline_removes_that_span() {
        // The same square and edge, picked at (0,4), station 28: the span
        // from station 20 through the first vertex to station 4 goes, and the
        // right half (4,0)-(8,0)-(8,8)-(4,8) is kept.
        let mut d = ToolDriver::new();
        let target = polyline(
            &mut d,
            &[(0.0, 0.0), (8.0, 0.0), (8.0, 8.0), (0.0, 8.0)],
            true,
        );
        line(&mut d, p(4.0, -2.0), p(4.0, 10.0));
        d.start("modify.trim");
        assert_eq!(d.enter().outcome, CONTINUE);
        assert_eq!(d.pick(target, 0.0, 4.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        assert_eq!(
            polyline_of(&d, target),
            path(&[(4.0, 0.0), (8.0, 0.0), (8.0, 8.0), (4.0, 8.0)], false)
        );
    }

    #[test]
    fn a_circle_cut_twice_keeps_the_arc_away_from_the_pick() {
        // Radius 4 about the origin, cut by the y axis at 90 and 270 degrees.
        // A pick at (4,0) takes the right half; the left half is kept as an
        // arc from 90 degrees sweeping 180 counter-clockwise. The crossings
        // come from a square root, so the angles are compared to 1e-12.
        let mut d = ToolDriver::new();
        let target = circle(&mut d, p(0.0, 0.0), 4.0);
        line(&mut d, p(0.0, -8.0), p(0.0, 8.0));
        d.start("modify.trim");
        assert_eq!(d.enter().outcome, CONTINUE);
        assert_eq!(d.pick(target, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        let kept = arc_of(&d, target);
        assert_eq!(kept.center, p(0.0, 0.0));
        assert_eq!(kept.radius, 4.0);
        near(kept.start_angle, PI / 2.0, 1e-12);
        near(kept.sweep, PI, 1e-12);
    }

    #[test]
    fn the_heights_of_a3_d_line_are_interpolated_at_the_new_ends() {
        // (0,0) at height 10 to (8,0) at height 18: z = 10 + x. Trimmed
        // between x = 2 and x = 6, the kept line ends at height 12 and the new
        // one starts at 16; their far ends keep 10 and 18.
        let mut d = ToolDriver::new();
        let target = with_3d_heights(
            &mut d,
            Geometry::Line(segment(0.0, 0.0, 8.0, 0.0)),
            &[Some(10.0), Some(18.0)],
        );
        line(&mut d, p(2.0, -2.0), p(2.0, 2.0));
        line(&mut d, p(6.0, -2.0), p(6.0, 2.0));
        d.start("modify.trim");
        assert_eq!(d.enter().outcome, CONTINUE);
        assert_eq!(d.pick(target, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        assert_eq!(heights(&d, target, 2), vec![Some(10.0), Some(12.0)]);
        assert_eq!(heights(&d, created(&d), 2), vec![Some(16.0), Some(18.0)]);
    }

    #[test]
    fn a_line_no_edge_crosses_is_refused_and_the_tool_stays_put() {
        let mut d = ToolDriver::new();
        let target = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        let edge = line(&mut d, p(0.0, 4.0), p(8.0, 4.0)); // parallel: never crosses
        d.start("modify.trim");
        assert_eq!(d.pick(edge, 4.0, 4.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, CONTINUE);
        let refused = d.pick(target, 4.0, 0.0);
        assert_eq!(refused.outcome, REJECTED);
        assert_eq!(refused.message, "No cutting edge crosses that line.");
        assert_eq!(d.expects(), ToolInput::Entity);
        assert_eq!(d.enter().outcome, DONE);
        assert_eq!(d.executed(), 0);
    }

    #[test]
    fn a_circle_crossed_once_is_refused() {
        let mut d = ToolDriver::new();
        let target = circle(&mut d, p(0.0, 0.0), 4.0);
        line(&mut d, p(0.0, 0.0), p(0.0, 8.0)); // from the centre out: one crossing, at (0,4)
        d.start("modify.trim");
        assert_eq!(d.enter().outcome, CONTINUE);
        let refused = d.pick(target, 4.0, 0.0);
        assert_eq!(refused.outcome, REJECTED);
        assert_eq!(
            refused.message,
            "A circle needs two crossings with the cutting edges to be trimmed."
        );
    }

    #[test]
    fn text_is_refused_with_what_the_tool_takes() {
        let mut d = ToolDriver::new();
        let label = text(
            &mut d,
            TextGeometry {
                position: p(0.0, 0.0),
                text: String::from("KERB"),
                ..TextGeometry::default()
            },
        );
        d.start("modify.trim");
        assert_eq!(d.enter().outcome, CONTINUE);
        let refused = d.pick(label, 0.0, 0.0);
        assert_eq!(refused.outcome, REJECTED);
        assert_eq!(
            refused.message,
            "A text cannot be trimmed; pick a line, arc, circle or polyline."
        );
    }

    #[test]
    fn a_pick_on_a_part_already_trimmed_away_is_refused() {
        let (mut d, scene) = trim_scene();
        d.start("modify.trim");
        assert_eq!(d.enter().outcome, CONTINUE);
        assert_eq!(d.pick(scene.target, 4.0, 0.0).outcome, CONTINUE);
        // (5,0) was in the span just removed: the document still draws it,
        // but there is nothing of the line left there.
        let refused = d.pick(scene.target, 5.0, 0.0);
        assert_eq!(refused.outcome, REJECTED);
        assert_eq!(
            refused.message,
            "That part of the line has already been trimmed away."
        );
    }

    #[test]
    fn an_object_on_a_locked_layer_is_refused_by_name() {
        let (mut d, scene) = trim_scene();
        lock_on_new_layer(&mut d, vec![scene.target], "kerbs");
        d.start("modify.trim");
        assert_eq!(d.enter().outcome, CONTINUE);
        let refused = d.pick(scene.target, 4.0, 0.0);
        assert_eq!(refused.outcome, REJECTED);
        assert_eq!(
            refused.message,
            "That line is on the locked layer 'kerbs'; unlock the layer to edit it."
        );
    }

    #[test]
    fn a_closed_polyline_crossed_once_is_refused() {
        // The edge runs from (4,4), inside the square, down to (4,-4): it
        // crosses the square's outline once, at (4,0), which leaves no span
        // to remove.
        let mut d = ToolDriver::new();
        let square = polyline(
            &mut d,
            &[(0.0, 0.0), (8.0, 0.0), (8.0, 8.0), (0.0, 8.0)],
            true,
        );
        let edge = line(&mut d, p(4.0, 4.0), p(4.0, -4.0));
        d.start("modify.trim");
        assert_eq!(d.pick(edge, 4.0, -2.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, CONTINUE);
        let refused = d.pick(square, 2.0, 0.0);
        assert_eq!(refused.outcome, REJECTED);
        assert_eq!(
            refused.message,
            "A closed polyline needs two crossings with the cutting edges to be trimmed."
        );
        assert_eq!(d.expects(), ToolInput::Entity);
    }

    #[test]
    fn enter_with_nothing_trimmed_finishes_without_a_command() {
        let (mut d, _scene) = trim_scene();
        d.start("modify.trim");
        assert_eq!(d.enter().outcome, CONTINUE);
        let finished = d.enter();
        assert_eq!(finished.outcome, DONE);
        assert!(finished.command.is_none());
        assert_eq!(d.executed(), 0);
    }

    #[test]
    fn the_preview_shows_the_part_the_cursor_would_remove() {
        let (mut d, scene) = trim_scene();
        d.start("modify.trim");
        assert_eq!(d.enter().outcome, CONTINUE);
        let feedback = d.preview(4.0, 0.0);
        assert_eq!(feedback.shapes.len(), 1);
        assert_eq!(
            feedback.shapes[0],
            Geometry::Line(segment(2.0, 0.0, 6.0, 0.0))
        );
        // Once picked it stays marked while the cursor moves on to nothing.
        assert_eq!(d.pick(scene.target, 4.0, 0.0).outcome, CONTINUE);
        assert!(contains_shape(
            &d.preview(100.0, 100.0),
            &Geometry::Line(segment(2.0, 0.0, 6.0, 0.0))
        ));
    }
}

// ============================================================================
// Extend
// ============================================================================

mod modify_edit_extend {
    use super::*;

    #[test]
    fn a_line_is_lengthened_from_the_picked_end_to_the_boundary() {
        // (0,0)-(4,0) picked near its east end reaches the boundary x = 8.
        let mut d = ToolDriver::new();
        let target = line(&mut d, p(0.0, 0.0), p(4.0, 0.0));
        let boundary = line(&mut d, p(8.0, -2.0), p(8.0, 2.0));
        d.start("modify.extend");
        assert_eq!(d.prompt(), "Select boundary edges or <all objects>");
        assert_eq!(d.pick(boundary, 8.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, CONTINUE);
        assert_eq!(d.prompt(), "Select object to extend or [Undo]");
        assert_eq!(d.pick(target, 3.0, 0.0).outcome, CONTINUE);
        let finished = d.enter();
        assert_eq!(finished.outcome, DONE);
        assert_eq!(finished.message, "Extended 1 end.");
        assert_eq!(segment_of(&d, target), segment(0.0, 0.0, 8.0, 0.0));
    }

    #[test]
    fn both_ends_of_a_line_reach_their_boundaries_in_one_command() {
        // Every object is a boundary: x = -4 to the west, x = 8 to the east.
        // The pick at (1,0) is nearer the start, (3,0) nearer the end.
        let mut d = ToolDriver::new();
        let target = line(&mut d, p(0.0, 0.0), p(4.0, 0.0));
        line(&mut d, p(-4.0, -2.0), p(-4.0, 2.0));
        line(&mut d, p(8.0, -2.0), p(8.0, 2.0));
        d.start("modify.extend");
        assert_eq!(d.enter().outcome, CONTINUE);
        assert_eq!(d.pick(target, 1.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.pick(target, 3.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        assert_eq!(d.executed(), 1);
        assert_eq!(segment_of(&d, target), segment(-4.0, 0.0, 8.0, 0.0));
        ok(d.document_mut().undo());
        assert_eq!(segment_of(&d, target), segment(0.0, 0.0, 4.0, 0.0));
    }

    #[test]
    fn an_arc_is_lengthened_along_its_circle() {
        // The quarter arc of radius 4 from 0 to 90 degrees, picked near its
        // end (0,4), goes on round its circle to the boundary (-8,0)-(0,0),
        // which it meets at (-4,0), 180 degrees: the sweep becomes a half
        // turn.
        let mut d = ToolDriver::new();
        let target = arc(&mut d, Arc2::new(p(0.0, 0.0), 4.0, 0.0, PI / 2.0));
        line(&mut d, p(-8.0, 0.0), p(0.0, 0.0));
        d.start("modify.extend");
        assert_eq!(d.enter().outcome, CONTINUE);
        assert_eq!(d.pick(target, 0.5, 3.9).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        let grown = arc_of(&d, target);
        assert_eq!(grown.start_angle, 0.0);
        near(grown.sweep, PI, 1e-12);
    }

    #[test]
    fn an_open_polyline_extends_its_end_segment() {
        // (0,0)-(4,0)-(4,4), 8 long; the pick at (4,3.5) is at station 7.5,
        // in the half nearer the last vertex, so (4,4) moves up to y = 8.
        let mut d = ToolDriver::new();
        let target = polyline(&mut d, &[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0)], false);
        line(&mut d, p(0.0, 8.0), p(8.0, 8.0));
        d.start("modify.extend");
        assert_eq!(d.enter().outcome, CONTINUE);
        assert_eq!(d.pick(target, 4.0, 3.5).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        assert_eq!(
            polyline_of(&d, target),
            path(&[(0.0, 0.0), (4.0, 0.0), (4.0, 8.0)], false)
        );
    }

    #[test]
    fn an_extended_end_has_no_height_because_none_was_surveyed_there() {
        // A 3D line (0,0) at 10 to (4,0) at 14 extended to x = 8: the start
        // keeps 10, and the new end is absent rather than a guessed 18 -
        // absent is not zero, and an extrapolated grade is not a survey.
        let mut d = ToolDriver::new();
        let target = with_3d_heights(
            &mut d,
            Geometry::Line(segment(0.0, 0.0, 4.0, 0.0)),
            &[Some(10.0), Some(14.0)],
        );
        line(&mut d, p(8.0, -2.0), p(8.0, 2.0));
        d.start("modify.extend");
        assert_eq!(d.enter().outcome, CONTINUE);
        assert_eq!(d.pick(target, 3.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        assert_eq!(heights(&d, target, 2), vec![Some(10.0), None]);
    }

    #[test]
    fn a_circle_and_a_closed_polyline_are_refused_as_having_no_end() {
        let mut d = ToolDriver::new();
        let ring = circle(&mut d, p(0.0, 0.0), 4.0);
        let square = polyline(&mut d, &[(10.0, 0.0), (12.0, 0.0), (12.0, 2.0)], true);
        d.start("modify.extend");
        assert_eq!(d.enter().outcome, CONTINUE);
        let round = d.pick(ring, 4.0, 0.0);
        assert_eq!(round.outcome, REJECTED);
        assert_eq!(round.message, "A circle has no end to extend.");
        let closed = d.pick(square, 11.0, 0.0);
        assert_eq!(closed.outcome, REJECTED);
        assert_eq!(closed.message, "A closed polyline has no end to extend.");
    }

    #[test]
    fn an_end_with_no_boundary_ahead_is_refused() {
        // The boundary is behind the start; the pick asks for the east end.
        let mut d = ToolDriver::new();
        let target = line(&mut d, p(0.0, 0.0), p(4.0, 0.0));
        line(&mut d, p(-4.0, -2.0), p(-4.0, 2.0));
        d.start("modify.extend");
        assert_eq!(d.enter().outcome, CONTINUE);
        let refused = d.pick(target, 3.0, 0.0);
        assert_eq!(refused.outcome, REJECTED);
        assert_eq!(
            refused.message,
            "No boundary lies ahead of that end of the line."
        );
    }

    #[test]
    fn an_open_polyline_whose_end_side_has_no_length_is_refused() {
        // (0,0)-(4,0)-(4,0): the last vertex repeats the one before, so the
        // end side has no direction to extend along.
        let mut d = ToolDriver::new();
        let string = polyline(&mut d, &[(0.0, 0.0), (4.0, 0.0), (4.0, 0.0)], false);
        let boundary = line(&mut d, p(8.0, -2.0), p(8.0, 2.0));
        d.start("modify.extend");
        assert_eq!(d.pick(boundary, 8.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, CONTINUE);
        let refused = d.pick(string, 3.5, 0.0);
        assert_eq!(refused.outcome, REJECTED);
        assert_eq!(
            refused.message,
            "The end segment of that polyline has no length."
        );
        assert_eq!(d.expects(), ToolInput::Entity);
    }

    #[test]
    fn the_preview_shows_the_new_length() {
        let mut d = ToolDriver::new();
        line(&mut d, p(0.0, 0.0), p(4.0, 0.0));
        line(&mut d, p(8.0, -2.0), p(8.0, 2.0));
        d.start("modify.extend");
        assert_eq!(d.enter().outcome, CONTINUE);
        let feedback = d.preview(3.0, 0.0);
        assert_eq!(feedback.shapes.len(), 1);
        assert_eq!(
            feedback.shapes[0],
            Geometry::Line(segment(4.0, 0.0, 8.0, 0.0))
        );
    }
}

// ============================================================================
// Offset
// ============================================================================

mod modify_edit_offset {
    use super::*;

    #[test]
    fn a_typed_distance_offsets_a_line_to_the_clicked_side() {
        // (0,0)-(8,0) moved 2 towards (4,5), its left: (0,2)-(8,2).
        let mut d = ToolDriver::new();
        let source = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        d.start("modify.offset");
        assert_eq!(d.expects(), ToolInput::Point);
        assert_eq!(d.r#type("2").outcome, CONTINUE);
        assert_eq!(d.prompt(), "Select object to offset or [Exit/Undo]");
        assert_eq!(d.pick(source, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(
            d.prompt(),
            "Specify point on side to offset or [Multiple/Undo]"
        );
        assert_eq!(d.click(4.0, 5.0).outcome, CONTINUE);
        let finished = d.enter();
        assert_eq!(finished.outcome, DONE);
        assert_eq!(finished.message, "Made 1 offset.");
        assert_eq!(segment_of(&d, created(&d)), segment(0.0, 2.0, 8.0, 2.0));
        assert_eq!(segment_of(&d, source), segment(0.0, 0.0, 8.0, 0.0));
    }

    #[test]
    fn every_offset_of_one_session_is_one_undo_of_the_document() {
        // Above and below the same line: (0,2)-(8,2) and (0,-2)-(8,-2).
        let mut d = ToolDriver::new();
        let source = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        d.start("modify.offset");
        assert_eq!(d.r#type("2").outcome, CONTINUE);
        assert_eq!(d.pick(source, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.click(4.0, 5.0).outcome, CONTINUE);
        assert_eq!(d.pick(source, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.click(4.0, -5.0).outcome, CONTINUE);
        let finished = d.enter();
        assert_eq!(finished.outcome, DONE);
        assert_eq!(finished.message, "Made 2 offsets.");
        assert_eq!(d.executed(), 1);
        let made = d.document().last_created_entities();
        assert_eq!(made.len(), 2);
        assert_eq!(segment_of(&d, made[0]), segment(0.0, 2.0, 8.0, 2.0));
        assert_eq!(segment_of(&d, made[1]), segment(0.0, -2.0, 8.0, -2.0));
        ok(d.document_mut().undo());
        assert_eq!(entity_count(&d), 1);
    }

    #[test]
    fn through_puts_the_copy_exactly_through_the_clicked_point() {
        // (3,1.5) is 1.5 from the line y = 0: the copy is (0,1.5)-(8,1.5).
        let mut d = ToolDriver::new();
        let source = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        d.start("modify.offset");
        assert_eq!(d.r#type("t").outcome, CONTINUE);
        assert_eq!(d.pick(source, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.prompt(), "Specify through point or [Multiple/Undo]");
        assert_eq!(d.click(3.0, 1.5).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        assert_eq!(segment_of(&d, created(&d)), segment(0.0, 1.5, 8.0, 1.5));
    }

    #[test]
    fn two_clicked_points_measure_the_distance() {
        // (0,0) to (3,4) is 5 (a 3-4-5 triangle); the second point typed
        // relative to the first. The copy of (0,0)-(8,0) is (0,5)-(8,5).
        let mut d = ToolDriver::new();
        let source = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        d.start("modify.offset");
        assert_eq!(d.click(0.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.prompt(), "Specify second point");
        assert_eq!(d.r#type("@3,4").outcome, CONTINUE);
        assert_eq!(d.pick(source, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.click(4.0, 9.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        assert_eq!(segment_of(&d, created(&d)), segment(0.0, 5.0, 8.0, 5.0));
    }

    #[test]
    fn a_circle_grows_or_shrinks_by_the_side_clicked() {
        // Radius 4, distance 1: a click inside makes radius 3, outside
        // radius 5.
        let mut d = ToolDriver::new();
        let ring = circle(&mut d, p(0.0, 0.0), 4.0);
        d.start("modify.offset");
        assert_eq!(d.r#type("1").outcome, CONTINUE);
        assert_eq!(d.pick(ring, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.click(1.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.pick(ring, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.click(8.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        let made = d.document().last_created_entities();
        assert_eq!(made.len(), 2);
        assert_eq!(circle_of(&d, made[0]), Circle2::new(p(0.0, 0.0), 3.0));
        assert_eq!(circle_of(&d, made[1]), Circle2::new(p(0.0, 0.0), 5.0));
    }

    #[test]
    fn a_rectangle_offsets_outward_with_mitred_corners() {
        // The counter-clockwise square (0,0)-(4,0)-(4,4)-(0,4). (2,-3) is
        // outside (right of the bottom edge's direction), so every side moves
        // out by 1 and the corners meet at (-1,-1), (5,-1), (5,5), (-1,5).
        let mut d = ToolDriver::new();
        let square = polyline(
            &mut d,
            &[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)],
            true,
        );
        d.start("modify.offset");
        assert_eq!(d.r#type("1").outcome, CONTINUE);
        assert_eq!(d.pick(square, 2.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.click(2.0, -3.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        assert_eq!(
            polyline_of(&d, created(&d)),
            path(&[(-1.0, -1.0), (5.0, -1.0), (5.0, 5.0), (-1.0, 5.0)], true)
        );
    }

    #[test]
    fn multiple_keeps_offsetting_the_copy_just_made() {
        // Distance 2 from (0,0)-(8,0), three clicks above: y = 2, 4 and 6.
        // Undo takes back y = 6, and Enter makes the two left.
        let mut d = ToolDriver::new();
        let source = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        d.start("modify.offset");
        assert_eq!(d.r#type("2").outcome, CONTINUE);
        assert_eq!(d.pick(source, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.r#type("M").outcome, CONTINUE);
        assert_eq!(
            d.prompt(),
            "Specify point on side for the next offset or [Undo]"
        );
        assert_eq!(d.click(4.0, 1.0).outcome, CONTINUE);
        assert_eq!(d.click(4.0, 3.0).outcome, CONTINUE);
        assert_eq!(d.click(4.0, 5.0).outcome, CONTINUE);
        assert_eq!(d.undo().outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        let made = d.document().last_created_entities();
        assert_eq!(made.len(), 2);
        assert_eq!(segment_of(&d, made[0]), segment(0.0, 2.0, 8.0, 2.0));
        assert_eq!(segment_of(&d, made[1]), segment(0.0, 4.0, 8.0, 4.0));
    }

    #[test]
    fn the_distance_used_last_is_offered_next_time_and_enter_takes_it() {
        let mut d = ToolDriver::new();
        let source = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        d.start("modify.offset");
        assert_eq!(d.r#type("2.5").outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE); // nothing offset; the distance is kept
        d.start("modify.offset");
        assert_eq!(d.prompt(), "Specify offset distance or [Through] <2.5>");
        assert_eq!(d.enter().outcome, CONTINUE);
        assert_eq!(d.pick(source, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.click(4.0, 5.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        assert_eq!(segment_of(&d, created(&d)), segment(0.0, 2.5, 8.0, 2.5));
    }

    #[test]
    fn each_vertex_of_a3_d_string_keeps_its_height() {
        let mut d = ToolDriver::new();
        let source = with_3d_heights(
            &mut d,
            Geometry::Line(segment(0.0, 0.0, 8.0, 0.0)),
            &[Some(10.0), Some(18.0)],
        );
        d.start("modify.offset");
        assert_eq!(d.r#type("2").outcome, CONTINUE);
        assert_eq!(d.pick(source, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.click(4.0, 5.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        assert_eq!(heights(&d, created(&d), 2), vec![Some(10.0), Some(18.0)]);
    }

    #[test]
    fn a_zero_negative_or_unreadable_distance_is_refused() {
        let mut d = ToolDriver::new();
        d.start("modify.offset");
        for typed in ["0", "-1"] {
            let refused = d.r#type(typed);
            assert_eq!(refused.outcome, REJECTED, "{typed}");
            assert_eq!(
                refused.message, "The offset distance must be greater than zero.",
                "{typed}"
            );
        }
        assert_eq!(d.r#type("two").outcome, REJECTED);
        assert_eq!(d.click(1.0, 1.0).outcome, CONTINUE);
        let same = d.click(1.0, 1.0);
        assert_eq!(same.outcome, REJECTED);
        assert_eq!(d.prompt(), "Specify second point");
    }

    #[test]
    fn a_through_point_on_the_object_is_refused() {
        let mut d = ToolDriver::new();
        let source = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        d.start("modify.offset");
        assert_eq!(d.r#type("T").outcome, CONTINUE);
        assert_eq!(d.pick(source, 4.0, 0.0).outcome, CONTINUE);
        let refused = d.click(6.0, 0.0);
        assert_eq!(refused.outcome, REJECTED);
        assert_eq!(
            refused.message,
            "The through point lies on the line; pick a point off it."
        );
    }

    #[test]
    fn an_inward_offset_no_smaller_than_the_radius_is_refused() {
        let mut d = ToolDriver::new();
        let ring = circle(&mut d, p(0.0, 0.0), 4.0);
        d.start("modify.offset");
        assert_eq!(d.r#type("5").outcome, CONTINUE);
        assert_eq!(d.pick(ring, 4.0, 0.0).outcome, CONTINUE);
        let refused = d.click(1.0, 0.0);
        assert_eq!(refused.outcome, REJECTED);
        assert_eq!(
            refused.message,
            "The distance is not smaller than the radius, so there is no circle on that side."
        );
    }

    #[test]
    fn an_offset_too_large_for_a_closed_polyline_is_refused_and_the_tool_stays_put() {
        // The counter-clockwise 8x8 square (0,0)-(8,0)-(8,8)-(0,8), offset
        // inward (the click (4,1) is left of the bottom side's direction).
        // Each side moves in by the distance and the mitres are where the
        // moved sides meet:
        //   by 3: (3,3), (5,3), (5,5), (3,5) - the 2x2 square inside, a true
        //         offset;
        //   by 4: every vertex at (4,4) - nothing left;
        //   by 5: (5,5), (3,5), (3,3), (5,3) - the sides have passed each
        //         other, so the bottom side now runs from x = 5 back to
        //         x = 3, against its source's direction: a square drawn the
        //         other way round, which is no offset of this one.
        for distance in ["4", "5"] {
            let mut d = ToolDriver::new();
            let square = polyline(
                &mut d,
                &[(0.0, 0.0), (8.0, 0.0), (8.0, 8.0), (0.0, 8.0)],
                true,
            );
            d.start("modify.offset");
            assert_eq!(d.r#type(distance).outcome, CONTINUE);
            assert_eq!(d.pick(square, 4.0, 0.0).outcome, CONTINUE);
            assert!(d.preview(4.0, 1.0).shapes.is_empty(), "{distance}");
            let refused = d.click(4.0, 1.0);
            assert_eq!(refused.outcome, REJECTED, "{distance}");
            assert_eq!(
                refused.message,
                format!(
                    "An offset of {distance} is too large for that closed polyline: a side of \
                     the copy would turn back on itself or shrink to nothing."
                ),
                "{distance}"
            );
            assert_eq!(
                d.prompt(),
                "Specify point on side to offset or [Multiple/Undo]"
            );
            assert_eq!(d.enter().outcome, DONE);
            assert_eq!(d.executed(), 0, "{distance}");
        }
        let mut d = ToolDriver::new();
        let square = polyline(
            &mut d,
            &[(0.0, 0.0), (8.0, 0.0), (8.0, 8.0), (0.0, 8.0)],
            true,
        );
        d.start("modify.offset");
        assert_eq!(d.r#type("3").outcome, CONTINUE);
        assert_eq!(d.pick(square, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.click(4.0, 1.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        assert_eq!(
            polyline_of(&d, created(&d)),
            path(&[(3.0, 3.0), (5.0, 3.0), (5.0, 5.0), (3.0, 5.0)], true)
        );
    }

    #[test]
    fn an_offset_that_would_turn_a_short_end_side_back_is_refused() {
        // (0,0)-(8,0)-(8,1) offset to the left of its direction (above the
        // first side). By 2 the first side moves to y = 2 and the second, 1
        // long and running up, to x = 6: the mitre is (6,2) and the copy ends
        // at (6,1), so its last side runs DOWN where the source's runs up. By
        // 0.5 the copy is (0,0.5)-(7.5,0.5)-(7.5,1), whose last side still
        // runs up: a true offset.
        let mut d = ToolDriver::new();
        let hook = polyline(&mut d, &[(0.0, 0.0), (8.0, 0.0), (8.0, 1.0)], false);
        d.start("modify.offset");
        assert_eq!(d.r#type("2").outcome, CONTINUE);
        assert_eq!(d.pick(hook, 4.0, 0.0).outcome, CONTINUE);
        let refused = d.click(4.0, 0.5);
        assert_eq!(refused.outcome, REJECTED);
        assert_eq!(
            refused.message,
            "An offset of 2 is too large for that polyline: a side of the copy would turn back \
             on itself or shrink to nothing."
        );
        assert_eq!(d.undo().outcome, CONTINUE); // back to choosing the object
        assert_eq!(d.undo().outcome, CONTINUE); // and to the distance
        assert_eq!(d.r#type("0.5").outcome, CONTINUE);
        assert_eq!(d.pick(hook, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.click(4.0, 0.25).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        assert_eq!(
            polyline_of(&d, created(&d)),
            path(&[(0.0, 0.5), (7.5, 0.5), (7.5, 1.0)], false)
        );
    }

    #[test]
    fn a_refused_pick_loses_none_of_the_offsets_already_made() {
        // Through: the line (0,20)-(8,20) through (4,22) makes (0,22)-(8,22).
        // Then the 8x8 square through its centre (4,4): every side 4 in,
        // every vertex at (4,4) - a polyline of no length, which the drawing
        // would refuse. The whole session is one command, so had that pick
        // been taken the drawing would have refused the command at Enter and
        // the line's offset would have been lost with it.
        let mut d = ToolDriver::new();
        let rule = line(&mut d, p(0.0, 20.0), p(8.0, 20.0));
        let square = polyline(
            &mut d,
            &[(0.0, 0.0), (8.0, 0.0), (8.0, 8.0), (0.0, 8.0)],
            true,
        );
        d.start("modify.offset");
        assert_eq!(d.r#type("T").outcome, CONTINUE);
        assert_eq!(d.pick(rule, 4.0, 20.0).outcome, CONTINUE);
        assert_eq!(d.click(4.0, 22.0).outcome, CONTINUE);
        assert_eq!(d.pick(square, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.click(4.0, 4.0).outcome, REJECTED);
        let finished = d.enter();
        assert_eq!(finished.outcome, DONE);
        assert_eq!(finished.message, "Made 1 offset.");
        assert_eq!(d.executed(), 1);
        assert_eq!(entity_count(&d), 3);
        assert_eq!(segment_of(&d, created(&d)), segment(0.0, 22.0, 8.0, 22.0));
    }

    #[test]
    fn an_object_on_a_locked_layer_is_refused_as_the_copy_would_land_there() {
        let mut d = ToolDriver::new();
        let source = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        lock_on_new_layer(&mut d, vec![source], "kerbs");
        d.start("modify.offset");
        assert_eq!(d.r#type("2").outcome, CONTINUE);
        let refused = d.pick(source, 4.0, 0.0);
        assert_eq!(refused.outcome, REJECTED);
        assert_eq!(
            refused.message,
            "That line is on the locked layer 'kerbs'; unlock the layer to edit it."
        );
        assert_eq!(d.prompt(), "Select object to offset or [Exit/Undo]");
    }

    #[test]
    fn text_is_refused_as_a_source() {
        let mut d = ToolDriver::new();
        let label = text(
            &mut d,
            TextGeometry {
                text: String::from("KERB"),
                ..TextGeometry::default()
            },
        );
        d.start("modify.offset");
        assert_eq!(d.r#type("1").outcome, CONTINUE);
        let refused = d.pick(label, 0.0, 0.0);
        assert_eq!(refused.outcome, REJECTED);
        assert_eq!(
            refused.message,
            "A text cannot be offset; pick a line, arc, circle or polyline."
        );
    }

    #[test]
    fn the_preview_shows_the_copy_for_the_cursor_side() {
        let mut d = ToolDriver::new();
        let source = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        d.start("modify.offset");
        assert_eq!(d.r#type("2").outcome, CONTINUE);
        assert_eq!(d.pick(source, 4.0, 0.0).outcome, CONTINUE);
        assert!(contains_shape(
            &d.preview(4.0, -3.0),
            &Geometry::Line(segment(0.0, -2.0, 8.0, -2.0))
        ));
    }
}

// ============================================================================
// Fillet and Chamfer
// ============================================================================

// Two lines meeting at the corner (8,0): along the x axis and up x = 8.
struct CornerScene {
    along: EntityId,
    up: EntityId,
}

fn corner_scene() -> (ToolDriver, CornerScene) {
    let mut driver = ToolDriver::new();
    let along = line(&mut driver, p(0.0, 0.0), p(8.0, 0.0));
    let up = line(&mut driver, p(8.0, 0.0), p(8.0, 8.0));
    (driver, CornerScene { along, up })
}

mod modify_edit_fillet {
    use super::*;

    #[test]
    fn a_radius_rounds_a_right_angled_corner_with_a_tangent_arc() {
        // Radius 2 at a right angle: the tangent points are 2 back along each
        // line, (6,0) and (8,2), and the arc's centre is (6,2), sweeping 90
        // degrees counter-clockwise from -90. tan(45 degrees) is not exactly 1
        // in binary, so positions are compared to 1e-12.
        let (mut d, scene) = corner_scene();
        d.start("modify.fillet");
        assert_eq!(d.r#type("R").outcome, CONTINUE);
        assert_eq!(d.expects(), ToolInput::Value);
        assert_eq!(d.r#type("2").outcome, CONTINUE);
        assert_eq!(
            d.prompt(),
            "Select first line or [Radius/Multiple] (radius 2)"
        );
        assert_eq!(d.pick(scene.along, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.prompt(), "Select second line or [Undo] (radius 2)");
        let finished = d.pick(scene.up, 8.0, 4.0);
        assert_eq!(finished.outcome, DONE);
        assert_eq!(finished.message, "Filleted 1 corner with radius 2.");
        assert!(d.finished());

        let along = segment_of(&d, scene.along);
        assert_eq!(along.start, p(0.0, 0.0));
        near(along.end.x, 6.0, 1e-12);
        near(along.end.y, 0.0, 1e-12);
        let up = segment_of(&d, scene.up);
        near(up.start.x, 8.0, 1e-12);
        near(up.start.y, 2.0, 1e-12);
        assert_eq!(up.end, p(8.0, 8.0));
        let round = arc_of(&d, created(&d));
        near(round.center.x, 6.0, 1e-12);
        near(round.center.y, 2.0, 1e-12);
        near(round.radius, 2.0, 1e-12);
        near(round.start_angle, -PI / 2.0, 1e-12);
        near(round.sweep, PI / 2.0, 1e-12);

        // One undo of the document puts both lines back and takes the arc
        // away.
        ok(d.document_mut().undo());
        assert_eq!(segment_of(&d, scene.along), segment(0.0, 0.0, 8.0, 0.0));
        assert_eq!(segment_of(&d, scene.up), segment(8.0, 0.0, 8.0, 8.0));
        assert_eq!(entity_count(&d), 2);
    }

    #[test]
    fn radius_zero_extends_both_lines_to_a_sharp_corner() {
        // (0,0)-(6,0) and (8,2)-(8,8) stop short of the corner (8,0); radius
        // 0 runs both to it and makes no arc.
        let mut d = ToolDriver::new();
        let along = line(&mut d, p(0.0, 0.0), p(6.0, 0.0));
        let up = line(&mut d, p(8.0, 2.0), p(8.0, 8.0));
        d.start("modify.fillet");
        assert_eq!(d.r#type("0").outcome, CONTINUE); // a number alone is the radius
        assert_eq!(d.pick(along, 3.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.pick(up, 8.0, 5.0).outcome, DONE);
        assert_eq!(segment_of(&d, along), segment(0.0, 0.0, 8.0, 0.0));
        assert_eq!(segment_of(&d, up), segment(8.0, 0.0, 8.0, 8.0));
        assert_eq!(entity_count(&d), 2);
    }

    #[test]
    fn crossing_lines_keep_the_sides_that_were_picked() {
        // (0,4)-(8,4) and (4,0)-(4,8) cross at (4,4), each 4 long either side
        // of it, so "the end farther from the corner" cannot say which half
        // to keep: geometry's fillet alone keeps the west and south halves,
        // (0,4)-(4,4) and (4,0)-(4,4). Radius 0 then runs the kept halves to
        // the corner.
        //   Picked east and north: (4,4)-(8,4) and (4,4)-(4,8) - neither is
        //   the geometry's half, so both picks are seen to count.
        //   Picked west and north: (0,4)-(4,4) and (4,4)-(4,8) - the first
        //   line agrees with the geometry, the second still must not.
        struct Case {
            pick_across: Point2,
            pick_down: Point2,
            across: Segment2,
            down: Segment2,
        }
        let cases = [
            Case {
                pick_across: p(7.0, 4.0),
                pick_down: p(4.0, 7.0),
                across: segment(4.0, 4.0, 8.0, 4.0),
                down: segment(4.0, 4.0, 4.0, 8.0),
            },
            Case {
                pick_across: p(1.0, 4.0),
                pick_down: p(4.0, 7.0),
                across: segment(0.0, 4.0, 4.0, 4.0),
                down: segment(4.0, 4.0, 4.0, 8.0),
            },
        ];
        for c in &cases {
            let mut d = ToolDriver::new();
            let across = line(&mut d, p(0.0, 4.0), p(8.0, 4.0));
            let down = line(&mut d, p(4.0, 0.0), p(4.0, 8.0));
            d.start("modify.fillet");
            assert_eq!(d.r#type("0").outcome, CONTINUE);
            assert_eq!(
                d.pick(across, c.pick_across.x, c.pick_across.y).outcome,
                CONTINUE
            );
            assert_eq!(d.pick(down, c.pick_down.x, c.pick_down.y).outcome, DONE);
            assert_eq!(segment_of(&d, across), c.across, "{}", c.pick_across.x);
            assert_eq!(segment_of(&d, down), c.down, "{}", c.pick_across.x);
        }
    }

    #[test]
    fn parallel_lines_are_refused_as_having_no_corner() {
        let mut d = ToolDriver::new();
        let a = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        let b = line(&mut d, p(0.0, 2.0), p(8.0, 2.0));
        let c = line(&mut d, p(10.0, 0.0), p(12.0, 0.0)); // collinear with a
        d.start("modify.fillet");
        assert_eq!(d.pick(a, 4.0, 0.0).outcome, CONTINUE);
        for (id, x, y) in [(b, 4.0, 2.0), (c, 11.0, 0.0)] {
            let refused = d.pick(id, x, y);
            assert_eq!(refused.outcome, REJECTED);
            assert_eq!(
                refused.message,
                "The lines are parallel, so there is no corner to fillet."
            );
        }
        assert!(d.prompt().starts_with("Select second line"));
    }

    #[test]
    fn a_radius_too_large_for_the_lines_is_refused_by_value() {
        // Radius 10 needs 10 of each line back from the corner; they are 8
        // long.
        let (mut d, scene) = corner_scene();
        d.start("modify.fillet");
        assert_eq!(d.r#type("10").outcome, CONTINUE);
        assert_eq!(d.pick(scene.along, 4.0, 0.0).outcome, CONTINUE);
        let refused = d.pick(scene.up, 8.0, 4.0);
        assert_eq!(refused.outcome, REJECTED);
        assert_eq!(
            refused.message,
            "A radius of 10 is too large for these lines."
        );
        assert_eq!(d.executed(), 0);
    }

    #[test]
    fn the_same_line_twice_an_arc_and_a_polyline_are_refused() {
        let (mut d, scene) = corner_scene();
        let bend = arc(&mut d, Arc2::new(p(20.0, 0.0), 2.0, 0.0, PI / 2.0));
        let string = polyline(&mut d, &[(30.0, 0.0), (34.0, 0.0), (34.0, 4.0)], false);
        d.start("modify.fillet");
        let bent = d.pick(bend, 22.0, 0.0);
        assert_eq!(bent.outcome, REJECTED);
        assert_eq!(
            bent.message,
            "Only lines can be filleted, and that is an arc."
        );
        let poly = d.pick(string, 32.0, 0.0);
        assert_eq!(poly.outcome, REJECTED);
        assert_eq!(
            poly.message,
            "Only lines can be filleted; explode the polyline into lines first."
        );
        assert_eq!(d.pick(scene.along, 4.0, 0.0).outcome, CONTINUE);
        let twice = d.pick(scene.along, 5.0, 0.0);
        assert_eq!(twice.outcome, REJECTED);
        assert_eq!(twice.message, "Pick a different line for the second.");
    }

    #[test]
    fn a_negative_radius_is_refused_and_the_radius_is_still_asked() {
        let mut d = ToolDriver::new();
        d.start("modify.fillet");
        assert_eq!(d.r#type("r").outcome, CONTINUE);
        let refused = d.r#type("-1");
        assert_eq!(refused.outcome, REJECTED);
        assert_eq!(refused.message, "The fillet radius cannot be negative.");
        assert_eq!(d.expects(), ToolInput::Value);
    }

    #[test]
    fn a_line_on_a_locked_layer_is_refused_as_either_line() {
        // Fillet refuses it as the first line, Chamfer as the second: the two
        // picks go through the same check.
        let (mut d, scene) = corner_scene();
        lock_on_new_layer(&mut d, vec![scene.up], "kerbs");
        let sentence = "That line is on the locked layer 'kerbs'; unlock the layer to edit it.";
        d.start("modify.fillet");
        let as_first = d.pick(scene.up, 8.0, 4.0);
        assert_eq!(as_first.outcome, REJECTED);
        assert_eq!(as_first.message, sentence);
        assert!(d.prompt().starts_with("Select first line"));
        d.start("modify.chamfer");
        assert_eq!(d.pick(scene.along, 4.0, 0.0).outcome, CONTINUE);
        let as_second = d.pick(scene.up, 8.0, 4.0);
        assert_eq!(as_second.outcome, REJECTED);
        assert_eq!(as_second.message, sentence);
        assert!(d.prompt().starts_with("Select second line"));
        assert_eq!(segment_of(&d, scene.up), segment(8.0, 0.0, 8.0, 8.0));
    }

    #[test]
    fn undo_at_the_second_line_goes_back_to_the_first() {
        let (mut d, scene) = corner_scene();
        d.start("modify.fillet");
        assert_eq!(d.pick(scene.along, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.undo().outcome, CONTINUE);
        assert!(d.prompt().starts_with("Select first line"));
    }

    #[test]
    fn multiple_fillets_two_corners_of_one_line_as_one_command() {
        // The right side (8,0)-(8,8) of an open box is filleted at both ends
        // with radius 2: its bottom end moves up to (8,2), then - working on
        // that shortened line - its top end down to (8,6). Two arcs; one
        // command.
        let mut d = ToolDriver::new();
        let bottom = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        let right = line(&mut d, p(8.0, 0.0), p(8.0, 8.0));
        let top = line(&mut d, p(8.0, 8.0), p(0.0, 8.0));
        d.start("modify.fillet");
        assert_eq!(d.r#type("2").outcome, CONTINUE);
        assert_eq!(d.r#type("M").outcome, CONTINUE);
        assert_eq!(d.pick(bottom, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.pick(right, 8.0, 4.0).outcome, CONTINUE);
        assert_eq!(d.pick(right, 8.0, 4.0).outcome, CONTINUE);
        assert_eq!(d.pick(top, 4.0, 8.0).outcome, CONTINUE);
        let finished = d.enter();
        assert_eq!(finished.outcome, DONE);
        assert_eq!(finished.message, "Filleted 2 corners with radius 2.");
        assert_eq!(d.executed(), 1);
        let side = segment_of(&d, right);
        near(side.start.y, 2.0, 1e-12);
        near(side.end.y, 6.0, 1e-12);
        assert_eq!(d.document().last_created_entities().len(), 2);
    }

    #[test]
    fn the_radius_used_last_is_the_radius_next_time() {
        let mut d = ToolDriver::new();
        d.start("modify.fillet");
        assert_eq!(d.r#type("3").outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        d.start("modify.fillet");
        assert_eq!(
            d.prompt(),
            "Select first line or [Radius/Multiple] (radius 3)"
        );
    }

    #[test]
    fn the_arc_takes_its_end_heights_from_the_lines_it_joins() {
        // (0,0) at 10 to (8,0) at 18, then (8,0) at 18 to (8,8) at 26: z
        // rises 1 per unit along the path. The radius 2 arc starts 6 along
        // the first line (height 16) and ends 2 up the second (height 20).
        let mut d = ToolDriver::new();
        let along = with_3d_heights(
            &mut d,
            Geometry::Line(segment(0.0, 0.0, 8.0, 0.0)),
            &[Some(10.0), Some(18.0)],
        );
        let up = with_3d_heights(
            &mut d,
            Geometry::Line(segment(8.0, 0.0, 8.0, 8.0)),
            &[Some(18.0), Some(26.0)],
        );
        d.start("modify.fillet");
        assert_eq!(d.r#type("2").outcome, CONTINUE);
        assert_eq!(d.pick(along, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.pick(up, 8.0, 4.0).outcome, DONE);
        let ends = heights(&d, created(&d), 2);
        let (Some(start), Some(end)) = (ends[0], ends[1]) else {
            panic!("the arc's ends have no heights: {ends:?}");
        };
        near(start, 16.0, 1e-9);
        near(end, 20.0, 1e-9);
    }

    #[test]
    fn the_preview_shows_the_arc_for_the_line_under_the_cursor() {
        let (mut d, scene) = corner_scene();
        d.start("modify.fillet");
        assert_eq!(d.r#type("2").outcome, CONTINUE);
        assert_eq!(d.pick(scene.along, 4.0, 0.0).outcome, CONTINUE);
        let feedback = d.preview(8.0, 4.0);
        assert_eq!(feedback.shapes.len(), 3);
        assert!(matches!(feedback.shapes[2], Geometry::Arc(_)));
    }
}

mod modify_edit_chamfer {
    use super::*;

    #[test]
    fn two_distances_cut_the_corner_with_a_bevel() {
        // Distance 2 along the first line and 3 along the second, back from
        // the corner (8,0): the cut points are (6,0) and (8,3), exactly.
        let (mut d, scene) = corner_scene();
        d.start("modify.chamfer");
        assert_eq!(d.r#type("D").outcome, CONTINUE);
        assert_eq!(d.r#type("2").outcome, CONTINUE);
        assert_eq!(d.prompt(), "Specify second chamfer distance <2>");
        assert_eq!(d.r#type("3").outcome, CONTINUE);
        assert_eq!(
            d.prompt(),
            "Select first line or [Distance/Multiple] (distances 2, 3)"
        );
        assert_eq!(d.pick(scene.along, 4.0, 0.0).outcome, CONTINUE);
        let finished = d.pick(scene.up, 8.0, 4.0);
        assert_eq!(finished.outcome, DONE);
        assert_eq!(finished.message, "Chamfered 1 corner at 2 and 3.");
        assert_eq!(segment_of(&d, scene.along), segment(0.0, 0.0, 6.0, 0.0));
        assert_eq!(segment_of(&d, scene.up), segment(8.0, 3.0, 8.0, 8.0));
        assert_eq!(segment_of(&d, created(&d)), segment(6.0, 0.0, 8.0, 3.0));
        ok(d.document_mut().undo());
        assert_eq!(entity_count(&d), 2);
    }

    #[test]
    fn enter_makes_the_second_distance_the_first() {
        let (mut d, scene) = corner_scene();
        d.start("modify.chamfer");
        assert_eq!(d.r#type("1").outcome, CONTINUE); // a number alone is the first distance
        assert_eq!(d.prompt(), "Specify second chamfer distance <1>");
        assert_eq!(d.enter().outcome, CONTINUE);
        assert_eq!(d.pick(scene.along, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.pick(scene.up, 8.0, 4.0).outcome, DONE);
        assert_eq!(segment_of(&d, created(&d)), segment(7.0, 0.0, 8.0, 1.0));
    }

    #[test]
    fn the_preview_shows_the_bevel_for_the_line_under_the_cursor() {
        // Distances 2 and 2 at the corner (8,0): the bevel (6,0)-(8,2).
        let (mut d, scene) = corner_scene();
        d.start("modify.chamfer");
        assert_eq!(d.r#type("2").outcome, CONTINUE);
        assert_eq!(d.enter().outcome, CONTINUE);
        assert_eq!(d.pick(scene.along, 4.0, 0.0).outcome, CONTINUE);
        assert!(contains_shape(
            &d.preview(8.0, 4.0),
            &Geometry::Line(segment(6.0, 0.0, 8.0, 2.0))
        ));
        // Nowhere near a line, there is nothing to preview.
        assert!(d.preview(40.0, 40.0).shapes.is_empty());
    }

    #[test]
    fn parallel_lines_and_distances_too_large_are_refused() {
        let (mut d, scene) = corner_scene();
        let parallel = line(&mut d, p(0.0, 2.0), p(6.0, 2.0));
        d.start("modify.chamfer");
        assert_eq!(d.r#type("9").outcome, CONTINUE);
        assert_eq!(d.r#type("9").outcome, CONTINUE);
        assert_eq!(d.pick(scene.along, 4.0, 0.0).outcome, CONTINUE);
        let flat = d.pick(parallel, 3.0, 2.0);
        assert_eq!(flat.outcome, REJECTED);
        assert_eq!(
            flat.message,
            "The lines are parallel, so there is no corner to chamfer."
        );
        let large = d.pick(scene.up, 8.0, 4.0);
        assert_eq!(large.outcome, REJECTED);
        assert_eq!(
            large.message,
            "The chamfer distances are too large for these lines."
        );
        // A number at the second line is no distance: that prompt takes a
        // line.
        let typed = d.r#type("5");
        assert_eq!(typed.outcome, REJECTED);
        assert_eq!(
            typed.message,
            "Pick the second line, or type U to pick the first again."
        );
    }

    #[test]
    fn a_negative_distance_is_refused_wherever_it_is_typed() {
        // At the first line (where a number is the first distance), and at
        // both distance prompts; each time the tool stays where it was.
        let mut d = ToolDriver::new();
        let sentence = "A chamfer distance cannot be negative.";
        d.start("modify.chamfer");
        let at_first_line = d.prompt();
        let as_first_line = d.r#type("-1");
        assert_eq!(as_first_line.outcome, REJECTED);
        assert_eq!(as_first_line.message, sentence);
        assert_eq!(d.prompt(), at_first_line);
        assert_eq!(d.expects(), ToolInput::Entity);
        assert_eq!(d.r#type("D").outcome, CONTINUE);
        let at_first_distance = d.prompt();
        assert!(at_first_distance.starts_with("Specify first chamfer distance"));
        let as_first_distance = d.r#type("-1");
        assert_eq!(as_first_distance.outcome, REJECTED);
        assert_eq!(as_first_distance.message, sentence);
        assert_eq!(d.prompt(), at_first_distance);
        assert_eq!(d.r#type("2").outcome, CONTINUE);
        let as_second_distance = d.r#type("-1");
        assert_eq!(as_second_distance.outcome, REJECTED);
        assert_eq!(as_second_distance.message, sentence);
        assert_eq!(d.prompt(), "Specify second chamfer distance <2>");
        assert_eq!(d.expects(), ToolInput::Value);
    }
}

// ============================================================================
// Break and Break at Point
// ============================================================================

mod modify_edit_break {
    use super::*;

    #[test]
    fn two_points_remove_the_span_between_them_from_a_line() {
        // (0,0)-(8,0) picked at (2,0), second point (6,0): (0,0)-(2,0) keeps
        // the line's id and (6,0)-(8,0) is new.
        let mut d = ToolDriver::new();
        let target = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        d.start("modify.break");
        assert_eq!(d.prompt(), "Select object to break");
        assert_eq!(d.pick(target, 2.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.prompt(), "Specify second break point or [First point]");
        let finished = d.click(6.0, 0.0);
        assert_eq!(finished.outcome, DONE);
        assert_eq!(finished.message, "Broken in two.");
        assert_eq!(segment_of(&d, target), segment(0.0, 0.0, 2.0, 0.0));
        assert_eq!(segment_of(&d, created(&d)), segment(6.0, 0.0, 8.0, 0.0));
        ok(d.document_mut().undo());
        assert_eq!(segment_of(&d, target), segment(0.0, 0.0, 8.0, 0.0));
        assert_eq!(entity_count(&d), 1);
    }

    #[test]
    fn at_zero_zero_breaks_at_the_first_point_without_a_gap() {
        let mut d = ToolDriver::new();
        let target = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        d.start("modify.break");
        assert_eq!(d.pick(target, 2.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.r#type("@0,0").outcome, DONE);
        assert_eq!(segment_of(&d, target), segment(0.0, 0.0, 2.0, 0.0));
        assert_eq!(segment_of(&d, created(&d)), segment(2.0, 0.0, 8.0, 0.0));
    }

    #[test]
    fn first_point_gives_the_first_break_point_exactly() {
        // The pick at (1,0) only chooses the line; F then gives (2,0).
        let mut d = ToolDriver::new();
        let target = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        d.start("modify.break");
        assert_eq!(d.pick(target, 1.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.r#type("F").outcome, CONTINUE);
        assert_eq!(d.prompt(), "Specify first break point");
        assert_eq!(d.click(2.0, 0.0).outcome, CONTINUE);
        // Polar from the first break point: 4 at 0 degrees is (6,0) - cos 0
        // and sin 0 are exact, so the point is too.
        assert_eq!(d.r#type("@4<0").outcome, DONE);
        assert_eq!(segment_of(&d, target), segment(0.0, 0.0, 2.0, 0.0));
        assert_eq!(segment_of(&d, created(&d)), segment(6.0, 0.0, 8.0, 0.0));
    }

    #[test]
    fn a_second_point_past_the_end_removes_the_whole_end() {
        // (10,0) is beyond (8,0): the nearest point of the line is its end, so
        // (6,0)-(8,0) goes and one line is left.
        let mut d = ToolDriver::new();
        let target = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        d.start("modify.break");
        assert_eq!(d.pick(target, 6.0, 0.0).outcome, CONTINUE);
        let finished = d.click(10.0, 0.0);
        assert_eq!(finished.outcome, DONE);
        assert_eq!(finished.message, "Broken.");
        assert_eq!(segment_of(&d, target), segment(0.0, 0.0, 6.0, 0.0));
        assert_eq!(entity_count(&d), 1);
    }

    #[test]
    fn a_circle_loses_the_arc_counter_clockwise_from_the_first_point() {
        // Radius 4: first point (4,0) at 0 degrees, second (0,4) at 90. The
        // quarter from 0 to 90 goes; the arc left starts at 90 and sweeps
        // 270.
        let mut d = ToolDriver::new();
        let ring = circle(&mut d, p(0.0, 0.0), 4.0);
        d.start("modify.break");
        assert_eq!(d.pick(ring, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.click(0.0, 4.0).outcome, DONE);
        let left = arc_of(&d, ring);
        near(left.start_angle, PI / 2.0, 1e-12);
        near(left.sweep, 1.5 * PI, 1e-12);
        assert_eq!(left.radius, 4.0);
    }

    #[test]
    fn an_arc_loses_the_span_between_the_points() {
        // The half arc of radius 4 from 0 to 180 degrees, broken between 45
        // and 135 degrees: a quarter is left at either end.
        let mut d = ToolDriver::new();
        let target = arc(&mut d, Arc2::new(p(0.0, 0.0), 4.0, 0.0, PI));
        let r = 4.0 / 2.0_f64.sqrt();
        d.start("modify.break");
        assert_eq!(d.pick(target, r, r).outcome, CONTINUE);
        assert_eq!(d.click(-r, r).outcome, DONE);
        let first = arc_of(&d, target);
        assert_eq!(first.start_angle, 0.0);
        near(first.sweep, PI / 4.0, 1e-12);
        let second = arc_of(&d, created(&d));
        near(second.start_angle, 0.75 * PI, 1e-12);
        near(second.sweep, PI / 4.0, 1e-12);
    }

    #[test]
    fn an_open_polyline_is_broken_around_its_corner() {
        // (0,0)-(8,0)-(8,8) broken from (4,0) to (8,4): (0,0)-(4,0) and
        // (8,4)-(8,8) are left, the corner between them removed.
        let mut d = ToolDriver::new();
        let target = polyline(&mut d, &[(0.0, 0.0), (8.0, 0.0), (8.0, 8.0)], false);
        d.start("modify.break");
        assert_eq!(d.pick(target, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.click(8.0, 4.0).outcome, DONE);
        assert_eq!(
            polyline_of(&d, target),
            path(&[(0.0, 0.0), (4.0, 0.0)], false)
        );
        assert_eq!(
            polyline_of(&d, created(&d)),
            path(&[(8.0, 4.0), (8.0, 8.0)], false)
        );
    }

    #[test]
    fn a_closed_polyline_opens_with_the_span_from_first_to_second_removed() {
        // The square (0,0)-(8,0)-(8,8)-(0,8) in drawing order: from (4,0) to
        // (8,4) goes, and the rest runs from (8,4) round to (4,0).
        let mut d = ToolDriver::new();
        let target = polyline(
            &mut d,
            &[(0.0, 0.0), (8.0, 0.0), (8.0, 8.0), (0.0, 8.0)],
            true,
        );
        d.start("modify.break");
        assert_eq!(d.pick(target, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.click(8.0, 4.0).outcome, DONE);
        assert_eq!(
            polyline_of(&d, target),
            path(
                &[(8.0, 4.0), (8.0, 8.0), (0.0, 8.0), (0.0, 0.0), (4.0, 0.0)],
                false
            )
        );
    }

    #[test]
    fn the_heights_at_the_break_points_are_interpolated_along_a3_d_string() {
        // Heights 0, 8, 16 along (0,0)-(8,0)-(8,8): one per unit of length.
        // The break points at stations 4 and 12 get 4 and 12.
        let mut d = ToolDriver::new();
        let target = with_3d_heights(
            &mut d,
            Geometry::Polyline(path(&[(0.0, 0.0), (8.0, 0.0), (8.0, 8.0)], false)),
            &[Some(0.0), Some(8.0), Some(16.0)],
        );
        d.start("modify.break");
        assert_eq!(d.pick(target, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.click(8.0, 4.0).outcome, DONE);
        assert_eq!(heights(&d, target, 2), vec![Some(0.0), Some(4.0)]);
        assert_eq!(heights(&d, created(&d), 2), vec![Some(12.0), Some(16.0)]);
    }

    #[test]
    fn breaks_that_would_do_nothing_or_everything_are_refused() {
        let mut d = ToolDriver::new();
        let target = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        let ring = circle(&mut d, p(0.0, 20.0), 4.0);
        d.start("modify.break");
        assert_eq!(d.pick(target, 0.0, 0.0).outcome, CONTINUE);
        let at_end = d.r#type("@0,0");
        assert_eq!(at_end.outcome, REJECTED);
        assert_eq!(
            at_end.message,
            "The break point is at an end of the line, so there is nothing to split."
        );
        let whole = d.click(8.0, 0.0);
        assert_eq!(whole.outcome, REJECTED);
        assert_eq!(
            whole.message,
            "The two points take in the whole line; use Erase to remove it."
        );
        assert_eq!(d.undo().outcome, CONTINUE); // back to choosing the object
        assert_eq!(d.pick(ring, 4.0, 20.0).outcome, CONTINUE);
        let once = d.r#type("@0,0");
        assert_eq!(once.outcome, REJECTED);
        assert_eq!(
            once.message,
            "A circle cannot be broken at a single point; pick two points."
        );
        assert_eq!(d.executed(), 0);
    }

    #[test]
    fn an_object_on_a_locked_layer_is_refused_by_name() {
        let mut d = ToolDriver::new();
        let target = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        lock_on_new_layer(&mut d, vec![target], "kerbs");
        for tool in ["modify.break", "modify.break_at_point"] {
            d.start(tool);
            let refused = d.pick(target, 2.0, 0.0);
            assert_eq!(refused.outcome, REJECTED, "{tool}");
            assert_eq!(
                refused.message,
                "That line is on the locked layer 'kerbs'; unlock the layer to edit it.",
                "{tool}"
            );
            assert_eq!(d.prompt(), "Select object to break", "{tool}");
        }
    }

    #[test]
    fn the_preview_shows_what_the_cursor_would_remove() {
        let mut d = ToolDriver::new();
        let target = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        d.start("modify.break");
        assert_eq!(d.pick(target, 2.0, 0.0).outcome, CONTINUE);
        let feedback = d.preview(6.0, 1.0);
        assert_eq!(feedback.shapes.len(), 1);
        assert_eq!(
            feedback.shapes[0],
            Geometry::Line(segment(2.0, 0.0, 6.0, 0.0))
        );
        assert_eq!(feedback.markers, vec![p(2.0, 0.0), p(6.0, 0.0)]);
    }
}

mod modify_edit_break_at_point {
    use super::*;

    #[test]
    fn a_line_is_split_in_two_at_the_point() {
        let mut d = ToolDriver::new();
        let target = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        d.start("modify.break_at_point");
        assert_eq!(d.pick(target, 5.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.prompt(), "Specify break point");
        let finished = d.click(2.0, 0.0);
        assert_eq!(finished.outcome, DONE);
        assert_eq!(finished.message, "Broken in two.");
        assert_eq!(segment_of(&d, target), segment(0.0, 0.0, 2.0, 0.0));
        assert_eq!(segment_of(&d, created(&d)), segment(2.0, 0.0, 8.0, 0.0));
    }

    #[test]
    fn one_undo_puts_the_line_back_whole() {
        let mut d = ToolDriver::new();
        let target = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        d.start("modify.break_at_point");
        assert_eq!(d.pick(target, 5.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.click(2.0, 0.0).outcome, DONE);
        assert_eq!(entity_count(&d), 2);
        ok(d.document_mut().undo());
        assert_eq!(entity_count(&d), 1);
        assert_eq!(segment_of(&d, target), segment(0.0, 0.0, 8.0, 0.0));
    }

    #[test]
    fn the_preview_marks_where_the_cursor_would_split() {
        // The cursor at (3,1) is 1 off the line; the split would be at
        // (3,0).
        let mut d = ToolDriver::new();
        let target = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        d.start("modify.break_at_point");
        assert_eq!(d.pick(target, 5.0, 0.0).outcome, CONTINUE);
        let feedback = d.preview(3.0, 1.0);
        assert!(feedback.shapes.is_empty());
        assert_eq!(feedback.markers, vec![p(3.0, 0.0)]);
    }

    #[test]
    fn an_open_polyline_is_split_at_a_vertex() {
        let mut d = ToolDriver::new();
        let target = polyline(&mut d, &[(0.0, 0.0), (8.0, 0.0), (8.0, 8.0)], false);
        d.start("modify.break_at_point");
        assert_eq!(d.pick(target, 4.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.click(8.0, 0.0).outcome, DONE);
        assert_eq!(
            polyline_of(&d, target),
            path(&[(0.0, 0.0), (8.0, 0.0)], false)
        );
        assert_eq!(
            polyline_of(&d, created(&d)),
            path(&[(8.0, 0.0), (8.0, 8.0)], false)
        );
    }

    #[test]
    fn a_circle_or_a_closed_polyline_is_refused_when_picked() {
        // Neither has ends, so one point cannot split either into two; Break,
        // with two points, is the tool that opens them.
        let mut d = ToolDriver::new();
        let ring = circle(&mut d, p(0.0, 0.0), 4.0);
        let triangle = polyline(&mut d, &[(0.0, 10.0), (4.0, 10.0), (4.0, 14.0)], true);
        d.start("modify.break_at_point");
        let round = d.pick(ring, 4.0, 0.0);
        assert_eq!(round.outcome, REJECTED);
        assert_eq!(
            round.message,
            "A circle has no ends, so one point cannot split it; use Break with two points."
        );
        let closed = d.pick(triangle, 2.0, 10.0);
        assert_eq!(closed.outcome, REJECTED);
        assert_eq!(
            closed.message,
            "A closed polyline has no ends, so one point cannot split it; use Break with two \
             points."
        );
        assert_eq!(d.prompt(), "Select object to break");
        assert_eq!(d.expects(), ToolInput::Entity);
    }

    #[test]
    fn a_break_point_at_an_end_is_refused() {
        let mut d = ToolDriver::new();
        let target = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        d.start("modify.break_at_point");
        assert_eq!(d.pick(target, 4.0, 0.0).outcome, CONTINUE);
        let end = d.click(8.0, 0.0);
        assert_eq!(end.outcome, REJECTED);
        assert_eq!(
            end.message,
            "The break point is at an end of the line, so there is nothing to split."
        );
    }
}

// ============================================================================
// Join and Explode
// ============================================================================

mod modify_edit_join {
    use super::*;

    #[test]
    fn lines_meeting_end_to_end_become_one_polyline() {
        // Three lines, the middle one drawn backwards, chain
        // (0,0)-(4,0)-(4,4)-(8,4). The lowest id keeps its identity; the
        // other two go.
        let mut d = ToolDriver::new();
        let first = line(&mut d, p(0.0, 0.0), p(4.0, 0.0));
        let second = line(&mut d, p(4.0, 4.0), p(4.0, 0.0));
        let third = line(&mut d, p(4.0, 4.0), p(8.0, 4.0));
        d.document_mut()
            .selection_mut()
            .set(vec![first, second, third]);
        d.start("modify.join");
        assert_eq!(
            d.prompt(),
            "Select lines and polylines to join or [Tolerance] (3 selected, ends within 0.001)"
        );
        let finished = d.enter();
        assert_eq!(finished.outcome, DONE);
        assert_eq!(finished.message, "Joined 3 objects into 1 polyline.");
        assert_eq!(
            polyline_of(&d, first),
            path(&[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (8.0, 4.0)], false)
        );
        assert_eq!(entity_count(&d), 1);
        ok(d.document_mut().undo());
        assert_eq!(entity_count(&d), 3);
        assert_eq!(segment_of(&d, first), segment(0.0, 0.0, 4.0, 0.0));
    }

    #[test]
    fn ends_within_the_tolerance_join_and_the_earlier_end_is_kept() {
        // The second line starts 0.0005 from the first's end, inside the
        // default 0.001: the joint is the first line's (4,0), unmoved.
        let mut d = ToolDriver::new();
        let first = line(&mut d, p(0.0, 0.0), p(4.0, 0.0));
        let second = line(&mut d, p(4.0005, 0.0), p(4.0, 4.0));
        d.start("modify.join");
        assert_eq!(d.pick(first, 2.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.pick(second, 4.0, 2.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        assert_eq!(
            polyline_of(&d, first),
            path(&[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0)], false)
        );
    }

    #[test]
    fn ends_farther_apart_than_the_tolerance_are_refused() {
        let mut d = ToolDriver::new();
        let first = line(&mut d, p(0.0, 0.0), p(4.0, 0.0));
        let second = line(&mut d, p(4.0005, 0.0), p(4.0, 4.0));
        d.document_mut().selection_mut().set(vec![first, second]);
        d.start("modify.join");
        assert_eq!(d.r#type("T").outcome, CONTINUE);
        assert_eq!(d.expects(), ToolInput::Value);
        assert_eq!(d.r#type("-1").outcome, REJECTED);
        assert_eq!(d.r#type("0.0001").outcome, CONTINUE);
        let refused = d.enter();
        assert_eq!(refused.outcome, REJECTED);
        assert_eq!(
            refused.message,
            "None of the selected lines and polylines meet end to end within 0.0001."
        );
        assert_eq!(d.expects(), ToolInput::Selection);
        // Put the default back for the rest of the program's tests.
        assert_eq!(d.r#type("T").outcome, CONTINUE);
        assert_eq!(d.r#type("0.001").outcome, CONTINUE);
    }

    #[test]
    fn four_lines_round_a_square_make_a_closed_polyline() {
        let mut d = ToolDriver::new();
        let a = line(&mut d, p(0.0, 0.0), p(4.0, 0.0));
        let b = line(&mut d, p(4.0, 0.0), p(4.0, 4.0));
        let c = line(&mut d, p(4.0, 4.0), p(0.0, 4.0));
        let e = line(&mut d, p(0.0, 4.0), p(0.0, 0.0));
        d.document_mut().selection_mut().set(vec![a, b, c, e]);
        d.start("modify.join");
        assert_eq!(d.enter().outcome, DONE);
        assert_eq!(
            polyline_of(&d, a),
            path(&[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)], true)
        );
    }

    #[test]
    fn an_arc_is_left_out_and_the_tool_says_why() {
        let mut d = ToolDriver::new();
        let a = line(&mut d, p(0.0, 0.0), p(4.0, 0.0));
        let b = line(&mut d, p(4.0, 0.0), p(4.0, 4.0));
        let bend = arc(&mut d, Arc2::new(p(4.0, 6.0), 2.0, -PI / 2.0, PI));
        d.document_mut().selection_mut().set(vec![a, b, bend]);
        d.start("modify.join");
        let finished = d.enter();
        assert_eq!(finished.outcome, DONE);
        assert_eq!(
            finished.message,
            "Joined 2 objects into 1 polyline. 1 arc was left out: a polyline has straight \
             segments only."
        );
        assert!(matches!(entity_of(&d, bend).geometry, Geometry::Arc(_)));
    }

    #[test]
    fn only_arcs_is_refused() {
        let mut d = ToolDriver::new();
        let a = arc(&mut d, Arc2::new(p(0.0, 0.0), 2.0, 0.0, PI));
        let b = arc(&mut d, Arc2::new(p(4.0, 0.0), 2.0, PI, PI));
        d.document_mut().selection_mut().set(vec![a, b]);
        d.start("modify.join");
        let refused = d.enter();
        assert_eq!(refused.outcome, REJECTED);
        assert_eq!(
            refused.message,
            "Select at least two lines or open polylines to join. 2 arcs were left out: a \
             polyline has straight segments only."
        );
    }

    #[test]
    fn a_line_joins_onto_the_start_of_a_polyline_with_its_heights() {
        // The polyline (4,0)-(8,0)-(8,4) at heights 4, 8, 12 is lowest-id and
        // kept; the line (0,0)-(4,0) at 0, 4 joins before its start.
        let mut d = ToolDriver::new();
        let string = with_3d_heights(
            &mut d,
            Geometry::Polyline(path(&[(4.0, 0.0), (8.0, 0.0), (8.0, 4.0)], false)),
            &[Some(4.0), Some(8.0), Some(12.0)],
        );
        let lead = with_3d_heights(
            &mut d,
            Geometry::Line(segment(0.0, 0.0, 4.0, 0.0)),
            &[Some(0.0), Some(4.0)],
        );
        d.document_mut().selection_mut().set(vec![string, lead]);
        d.start("modify.join");
        assert_eq!(d.enter().outcome, DONE);
        assert_eq!(
            polyline_of(&d, string),
            path(&[(0.0, 0.0), (4.0, 0.0), (8.0, 0.0), (8.0, 4.0)], false)
        );
        assert_eq!(
            heights(&d, string, 4),
            vec![Some(0.0), Some(4.0), Some(8.0), Some(12.0)]
        );
    }

    #[test]
    fn a_line_on_a_locked_layer_is_left_out_and_the_rest_join() {
        // (0,0)-(4,0), (4,0)-(4,4) and (4,4)-(8,4) meet end to end, but the
        // third is on a locked layer: the first two join and the third stays
        // a line.
        let mut d = ToolDriver::new();
        let a = line(&mut d, p(0.0, 0.0), p(4.0, 0.0));
        let b = line(&mut d, p(4.0, 0.0), p(4.0, 4.0));
        let c = line(&mut d, p(4.0, 4.0), p(8.0, 4.0));
        lock_on_new_layer(&mut d, vec![c], "kerbs");
        d.document_mut().selection_mut().set(vec![a, b, c]);
        d.start("modify.join");
        let finished = d.enter();
        assert_eq!(finished.outcome, DONE);
        assert_eq!(
            finished.message,
            "Joined 2 objects into 1 polyline. 1 object on a locked layer was left out."
        );
        assert_eq!(
            polyline_of(&d, a),
            path(&[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0)], false)
        );
        assert_eq!(segment_of(&d, c), segment(4.0, 4.0, 8.0, 4.0));
    }

    #[test]
    fn undo_takes_back_the_last_object_picked() {
        let mut d = ToolDriver::new();
        let a = line(&mut d, p(0.0, 0.0), p(4.0, 0.0));
        let b = line(&mut d, p(4.0, 0.0), p(4.0, 4.0));
        d.start("modify.join");
        assert_eq!(d.pick(a, 2.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.pick(b, 4.0, 2.0).outcome, CONTINUE);
        assert_eq!(
            d.prompt(),
            "Select lines and polylines to join or [Tolerance] (2 selected, ends within 0.001)"
        );
        assert_eq!(d.undo().outcome, CONTINUE);
        assert_eq!(
            d.prompt(),
            "Select lines and polylines to join or [Tolerance] (1 selected, ends within 0.001)"
        );
        // With b taken back there is only one line, and nothing to join it
        // to.
        assert_eq!(d.enter().outcome, REJECTED);
        assert_eq!(d.undo().outcome, CONTINUE);
        assert_eq!(d.undo().outcome, REJECTED);
        assert_eq!(d.executed(), 0);
    }

    #[test]
    fn enter_with_nothing_selected_is_refused() {
        let mut d = ToolDriver::new();
        d.start("modify.join");
        let refused = d.enter();
        assert_eq!(refused.outcome, REJECTED);
        assert_eq!(
            refused.message,
            "Select at least two lines or open polylines to join."
        );
    }

    #[test]
    fn the_preview_shows_the_polyline_enter_would_make() {
        let mut d = ToolDriver::new();
        let a = line(&mut d, p(0.0, 0.0), p(4.0, 0.0));
        let b = line(&mut d, p(4.0, 0.0), p(4.0, 4.0));
        d.document_mut().selection_mut().set(vec![a, b]);
        d.start("modify.join");
        let feedback = d.preview(0.0, 0.0);
        assert_eq!(feedback.shapes.len(), 1);
        assert_eq!(
            feedback.shapes[0],
            Geometry::Polyline(path(&[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0)], false))
        );
    }
}

mod modify_edit_explode {
    use super::*;

    #[test]
    fn a_rectangle_comes_apart_into_its_four_lines() {
        let mut d = ToolDriver::new();
        let rectangle = polyline(
            &mut d,
            &[(0.0, 0.0), (4.0, 0.0), (4.0, 2.0), (0.0, 2.0)],
            true,
        );
        d.document_mut().selection_mut().set(vec![rectangle]);
        d.start("modify.explode");
        assert_eq!(d.prompt(), "Select polylines to explode (1 selected)");
        let finished = d.enter();
        assert_eq!(finished.outcome, DONE);
        assert_eq!(finished.message, "Exploded 1 polyline into 4 lines.");
        assert!(!d.document().model().entities.contains(rectangle));
        let lines = d.document().last_created_entities();
        assert_eq!(lines.len(), 4);
        assert_eq!(segment_of(&d, lines[0]), segment(0.0, 0.0, 4.0, 0.0));
        assert_eq!(segment_of(&d, lines[1]), segment(4.0, 0.0, 4.0, 2.0));
        assert_eq!(segment_of(&d, lines[2]), segment(4.0, 2.0, 0.0, 2.0));
        assert_eq!(segment_of(&d, lines[3]), segment(0.0, 2.0, 0.0, 0.0));
        ok(d.document_mut().undo());
        assert_eq!(entity_count(&d), 1);
        assert!(d.document().model().entities.contains(rectangle));
    }

    #[test]
    fn each_line_keeps_the_heights_of_its_two_ends() {
        let mut d = ToolDriver::new();
        let string = with_3d_heights(
            &mut d,
            Geometry::Polyline(path(&[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0)], false)),
            &[Some(1.0), Some(2.0), None],
        );
        d.start("modify.explode");
        assert_eq!(d.pick(string, 2.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.enter().outcome, DONE);
        let lines = d.document().last_created_entities();
        assert_eq!(lines.len(), 2);
        assert_eq!(heights(&d, lines[0], 2), vec![Some(1.0), Some(2.0)]);
        assert_eq!(heights(&d, lines[1], 2), vec![Some(2.0), None]);
    }

    #[test]
    fn the_preview_marks_every_vertex_where_the_polyline_will_come_apart() {
        let mut d = ToolDriver::new();
        let string = polyline(&mut d, &[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0)], false);
        d.document_mut().selection_mut().set(vec![string]);
        d.start("modify.explode");
        assert_eq!(
            d.preview(0.0, 0.0).markers,
            vec![p(0.0, 0.0), p(4.0, 0.0), p(4.0, 4.0)]
        );
    }

    #[test]
    fn a_selection_with_no_polyline_is_refused_and_others_are_left_alone() {
        let mut d = ToolDriver::new();
        let single = line(&mut d, p(0.0, 0.0), p(4.0, 0.0));
        d.start("modify.explode");
        let empty = d.enter();
        assert_eq!(empty.outcome, REJECTED);
        assert_eq!(empty.message, "Select the polylines to explode first.");
        assert_eq!(d.pick(single, 2.0, 0.0).outcome, CONTINUE);
        let none = d.enter();
        assert_eq!(none.outcome, REJECTED);
        assert_eq!(
            none.message,
            "Only polylines can be exploded, and the selection has none that can be. 1 object \
             that is not a polyline was left as it was."
        );
        let string = polyline(&mut d, &[(0.0, 4.0), (4.0, 4.0), (4.0, 8.0)], false);
        assert_eq!(d.pick(string, 2.0, 4.0).outcome, CONTINUE);
        let mixed = d.enter();
        assert_eq!(mixed.outcome, DONE);
        assert_eq!(
            mixed.message,
            "Exploded 1 polyline into 2 lines. 1 object that is not a polyline was left as it \
             was."
        );
        assert!(d.document().model().entities.contains(single));
    }

    #[test]
    fn a_polyline_on_a_locked_layer_is_left_as_it_was() {
        let mut d = ToolDriver::new();
        let open = polyline(&mut d, &[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0)], false);
        let locked = polyline(&mut d, &[(0.0, 8.0), (4.0, 8.0), (4.0, 12.0)], false);
        lock_on_new_layer(&mut d, vec![locked], "kerbs");
        d.document_mut().selection_mut().set(vec![open, locked]);
        d.start("modify.explode");
        let finished = d.enter();
        assert_eq!(finished.outcome, DONE);
        assert_eq!(
            finished.message,
            "Exploded 1 polyline into 2 lines. 1 polyline on a locked layer was left as it was."
        );
        assert_eq!(
            polyline_of(&d, locked),
            path(&[(0.0, 8.0), (4.0, 8.0), (4.0, 12.0)], false)
        );
        assert!(!d.document().model().entities.contains(open));
    }

    #[test]
    fn undo_takes_back_the_last_polyline_picked() {
        let mut d = ToolDriver::new();
        let string = polyline(&mut d, &[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0)], false);
        d.start("modify.explode");
        assert_eq!(d.pick(string, 2.0, 0.0).outcome, CONTINUE);
        assert_eq!(d.prompt(), "Select polylines to explode (1 selected)");
        assert_eq!(d.undo().outcome, CONTINUE);
        assert_eq!(d.prompt(), "Select polylines to explode (0 selected)");
        assert_eq!(d.undo().outcome, REJECTED);
        let empty = d.enter();
        assert_eq!(empty.outcome, REJECTED);
        assert_eq!(empty.message, "Select the polylines to explode first.");
        assert!(d.document().model().entities.contains(string));
    }
}

// ============================================================================
// The edit session
// ============================================================================

mod modify_edit_session {
    use super::*;

    #[test]
    fn an_operation_with_a_piece_the_drawing_would_refuse_is_not_taken_at_all() {
        // The line is shortened to (0,0)-(4,0) and, in the same operation, a
        // zero-length line (4,0)-(4,0) is added. The drawing refuses a line of
        // no length, and a session becomes ONE command, so taking the piece
        // would lose every edit of the session at Enter: the add is refused,
        // and the shortening made since begin() goes with it.
        let mut d = ToolDriver::new();
        let target = line(&mut d, p(0.0, 0.0), p(8.0, 0.0));
        let mut session = EditSession::new();
        let mut shorter = entity_of(&d, target);
        shorter.geometry = Geometry::Line(segment(0.0, 0.0, 4.0, 0.0));
        session.begin();
        ok(session.replace(target, vec![shorter.clone()]));
        let mut nothing = shorter;
        nothing.geometry = Geometry::Line(segment(4.0, 0.0, 4.0, 0.0));
        let refused = session
            .add(nothing.clone())
            .expect_err("a line of no length is refused");
        assert_eq!(
            refused.message,
            "That would make a line the drawing cannot hold: line has zero length."
        );
        assert_eq!(session.operations(), 0);
        assert!(!session.edited(target));
        assert!(session.added().is_empty());
        // A replacement the drawing would refuse is refused the same way.
        session.begin();
        assert!(session.replace(target, vec![nothing]).is_err());
        assert!(!session.edited(target));
        assert_eq!(session.operations(), 0);
    }
}

// ============================================================================
// The catalogue
// ============================================================================

mod modify_edit_catalogue {
    use super::*;

    #[test]
    fn each_tool_is_found_by_its_auto_cad_aliases() {
        let catalog = tool_catalog();
        let expected: [(&str, &[&str]); 9] = [
            ("modify.trim", &["TRIM", "TR"]),
            ("modify.extend", &["EXTEND", "EX"]),
            ("modify.offset", &["OFFSET", "O"]),
            ("modify.fillet", &["FILLET", "F"]),
            ("modify.chamfer", &["CHAMFER", "CHA"]),
            ("modify.break", &["BREAK", "BR"]),
            ("modify.break_at_point", &["BREAKATPOINT"]),
            ("modify.join", &["JOIN", "J"]),
            ("modify.explode", &["EXPLODE", "X"]),
        ];
        for (id, aliases) in expected {
            let Some(info) = catalog.find(id) else {
                panic!("no tool {id}");
            };
            assert_eq!(info.category, "Modify", "{id}");
            assert_eq!(info.group, "Edit", "{id}");
            assert!(!info.tip.is_empty(), "{id}");
            for &alias in aliases {
                let Some(found) = catalog.find_by_alias(alias) else {
                    panic!("no tool for {alias}");
                };
                assert_eq!(found.id, id, "{alias}");
            }
        }
    }
}
