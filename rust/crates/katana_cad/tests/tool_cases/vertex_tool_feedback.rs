// What the Draw > Vertices tools show before a click and what they act on
// (src/tools/modify_vertex.rs; docs/drawing.md, "What a vertex tool acts on")
// - the owner's request of 2026-09-30: an inserted vertex gave "no visual
// clue where the vertex is going", and a tool asked for a polyline "but what
// about the already selected vertex" (tests/cad/tools/test_vertex_tool_feedback.cpp).
//
// Every expected coordinate here is worked out by hand from the polyline
// given, never read back from a run. The pick aperture is 0.5, so a vertex is
// reached within 0.75 and a segment within 0.5 (the view's 12 and 8 px).

use katana_cad::drawing::vertex_editing::read_polyline;
use katana_cad::{CommandInterpreter, FeedbackRole, Grip, GripKind, ToolFeedback};
use katana_commands::create_entities;
use katana_core::text::format_exact_real;
use katana_entity::entity::set_heights;
use katana_entity::{Entity, EntityId, EnumName, Geometry};
use katana_geometry::{CurvePolyline2, CurveVertex, Point2, Polyline2};

use crate::tool_driver::ToolDriver;

const APERTURE: f64 = 0.5;
const EXACT: f64 = 1.0e-9;

#[track_caller]
fn add_polyline_of(
    driver: &mut ToolDriver,
    points: &[Point2],
    closed: bool,
    heights: &[Option<f64>],
) -> EntityId {
    let mut entity = Entity {
        geometry: Geometry::Polyline(Polyline2::new(points.to_vec(), closed)),
        ..Entity::default()
    };
    if !heights.is_empty() {
        set_heights(&mut entity.properties, heights);
    }
    driver.add(create_entities(vec![entity]))
}

// An open polyline with no heights (the C++ defaults).
#[track_caller]
fn add_polyline(driver: &mut ToolDriver, points: &[Point2]) -> EntityId {
    add_polyline_of(driver, points, false, &[])
}

#[track_caller]
fn add_curve(driver: &mut ToolDriver, shape: &CurvePolyline2) -> EntityId {
    let entity = Entity {
        geometry: Geometry::CurvePolyline(shape.clone()),
        ..Entity::default()
    };
    driver.add(create_entities(vec![entity]))
}

#[track_caller]
fn shape_of(driver: &ToolDriver, id: EntityId) -> CurvePolyline2 {
    let Some(entity) = driver.document().model().entities.find(id) else {
        panic!("entity {id} is not in the drawing");
    };
    match read_polyline(entity) {
        Some(shape) => shape,
        None => panic!("entity {id} is not a polyline"),
    }
}

#[track_caller]
fn positions_of(driver: &ToolDriver, id: EntityId) -> Vec<Point2> {
    shape_of(driver, id).positions()
}

// The height of vertex `index` of `id`, failing the test when it has none
// (the C++ dereferences the optional).
#[track_caller]
fn height_of(driver: &ToolDriver, id: EntityId, index: usize) -> f64 {
    match shape_of(driver, id).vertices[index].height {
        Some(height) => height,
        None => panic!("vertex {index} of {id} has no height"),
    }
}

fn select_only(driver: &mut ToolDriver, ids: Vec<EntityId>) {
    driver.document_mut().selection_mut().set(ids);
}

#[track_caller]
fn vertex_handle(driver: &ToolDriver, id: EntityId, index: usize) -> Grip {
    Grip {
        entity: id,
        kind: GripKind::Vertex,
        index,
        position: shape_of(driver, id).vertices[index].position,
    }
}

fn points_of(feedback: &ToolFeedback, role: FeedbackRole) -> Vec<Point2> {
    feedback
        .marks
        .iter()
        .filter(|mark| mark.role == role)
        .filter_map(|mark| match &mark.geometry {
            Geometry::Point(point) => Some(point.position),
            _ => None,
        })
        .collect()
}

// The vertices a preview marks as the reason a click is refused
// (FeedbackMark::refused), whatever their role.
fn refused_points_of(feedback: &ToolFeedback) -> Vec<Point2> {
    feedback
        .marks
        .iter()
        .filter(|mark| mark.refused)
        .filter_map(|mark| match &mark.geometry {
            Geometry::Point(point) => Some(point.position),
            _ => None,
        })
        .collect()
}

fn labels_of(feedback: &ToolFeedback, role: FeedbackRole) -> Vec<String> {
    feedback
        .marks
        .iter()
        .filter(|mark| mark.role == role && !mark.label.is_empty())
        .map(|mark| mark.label.clone())
        .collect()
}

fn pieces_of(feedback: &ToolFeedback, role: FeedbackRole) -> Vec<Geometry> {
    feedback
        .marks
        .iter()
        .filter(|mark| mark.role == role && !matches!(mark.geometry, Geometry::Point(_)))
        .map(|mark| mark.geometry.clone())
        .collect()
}

fn close_to(a: Point2, b: Point2, tolerance: f64) -> bool {
    a.distance_to(b) <= tolerance
}

fn is_segment(geometry: &Geometry, a: Point2, b: Point2) -> bool {
    matches!(geometry, Geometry::Line(segment)
        if close_to(segment.start, a, EXACT) && close_to(segment.end, b, EXACT))
}

fn has_point(points: &[Point2], p: Point2, tolerance: f64) -> bool {
    points.iter().any(|&q| close_to(p, q, tolerance))
}

// A preview's marks as text - role, place, label, refused - with its caption,
// to tell one preview from another.
fn describe(feedback: &ToolFeedback) -> String {
    let mut text = format!(
        "{}{}",
        feedback.caption,
        if feedback.refused { " refused" } else { "" }
    );
    for mark in &feedback.marks {
        text += &format!(
            " | {} {}{}",
            mark.role.name(),
            mark.label,
            if mark.refused { " (refused)" } else { "" }
        );
        match &mark.geometry {
            Geometry::Point(point) => {
                text += &format!(
                    " at {},{}",
                    format_exact_real(point.position.x),
                    format_exact_real(point.position.y)
                );
            }
            Geometry::Line(segment) => {
                let middle = segment.midpoint();
                text += &format!(
                    " segment {},{}",
                    format_exact_real(middle.x),
                    format_exact_real(middle.y)
                );
            }
            Geometry::Arc(arc) => {
                let middle = arc.midpoint();
                text += &format!(
                    " arc {},{}",
                    format_exact_real(middle.x),
                    format_exact_real(middle.y)
                );
            }
            _ => text += " whole",
        }
    }
    text
}

// Whether `ghost` - the result as the preview drew it - is one of the
// segments of `result`, the polyline the click made: at its ends and middle.
fn is_piece_of(result: &CurvePolyline2, ghost: &Geometry) -> bool {
    let ghost_at = |parameter: f64| match ghost {
        Geometry::Line(line) => Some(line.point_at(parameter)),
        Geometry::Arc(arc) => Some(arc.point_at(parameter)),
        _ => None,
    };
    (0..result.segment_count()).any(|i| {
        let piece = result.segment(i);
        [0.0, 0.5, 1.0].iter().all(|&parameter| {
            ghost_at(parameter).is_some_and(|at| close_to(piece.point_at(parameter), at, 1.0e-9))
        })
    })
}

// The vertex of `shape` at `at`, if one is.
fn vertex_at(shape: &CurvePolyline2, at: Point2) -> Option<&CurveVertex> {
    shape
        .vertices
        .iter()
        .find(|vertex| close_to(vertex.position, at, EXACT))
}

// A line of the layers' interpreter on the driver's drawing, failing the test
// when it is refused.
#[track_caller]
fn layer_line(layers: &mut CommandInterpreter, driver: &mut ToolDriver, line: &str) {
    if let Err(error) = layers.run(driver.document_mut(), line) {
        panic!("{line}: {}", error.describe());
    }
}

// A polyline on layer B, made with the layer, for the tests of what the view
// lets be edited.
#[track_caller]
fn on_layer_b(
    driver: &mut ToolDriver,
    layers: &mut CommandInterpreter,
    points: &[Point2],
    closed: bool,
) -> EntityId {
    layer_line(layers, driver, "LAYER NEW B");
    let entity = Entity {
        geometry: Geometry::Polyline(Polyline2::new(points.to_vec(), closed)),
        layer: String::from("B"),
        ..Entity::default()
    };
    driver.add(create_entities(vec![entity]))
}

// Hides layer B in the drawing or in the view alone, or locks it.
#[track_caller]
fn take_away_layer_b(driver: &mut ToolDriver, layers: &mut CommandInterpreter, how: &str) {
    if how == "hidden" {
        layer_line(layers, driver, "LAYER HIDE B");
    } else if how == "hidden in the view" {
        driver.hide_in_view("B");
    } else {
        layer_line(layers, driver, "LAYER LOCK B");
    }
}

mod vertex_tool_feedback {
    use katana_cad::drawing::vertex_editing::{edit_polyline, height_text};
    use katana_cad::interactive_tool::ToolInput;
    use katana_cad::snapping::snap;
    use katana_cad::{SnapMode, SnapRequest};
    use katana_commands::{EntityAttributes, create_line};
    use katana_entity::{PointGeometry, PropertyValue};
    use katana_geometry::curves2d::arc_from_bulge;
    use katana_geometry::polyline_vertices::move_vertex;

    use super::*;
    use crate::{CONTINUE, DONE, REJECTED, double_eq, near, ok, pt};

    // ---- Insert Vertex -------------------------------------------------------------------

    #[test]
    fn insert_vertex_shows_the_segment_it_splits_and_the_vertex_on_it() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        select_only(&mut driver, vec![p]);
        driver.start("draw.vertex.insert");
        let shown = driver.preview(4.0, 0.3);
        // (4, 0.3) projects onto segment 0 at (4, 0), 4 from vertex 0.
        let targets = pieces_of(&shown, FeedbackRole::Target);
        assert_eq!(targets.len(), 1);
        assert!(is_segment(&targets[0], pt(0.0, 0.0), pt(10.0, 0.0)));
        let added = points_of(&shown, FeedbackRole::Added);
        assert_eq!(added.len(), 1);
        assert!(close_to(added[0], pt(4.0, 0.0), EXACT));
        assert!(
            shown.shapes.is_empty(),
            "the two halves lie on the old segment: no ghost"
        );
        assert_eq!(shown.focus, Some(p));
        assert!(!shown.refused);
        assert_eq!(shown.caption, "new vertex between 0 and 1 · 4.000 from 0");
        assert_eq!(driver.executed(), 0, "a preview changes nothing");
    }

    #[test]
    fn a_click_near_the_line_puts_the_vertex_on_it() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        select_only(&mut driver, vec![p]);
        driver.start("draw.vertex.insert");
        assert_eq!(
            driver.expects(),
            ToolInput::Point,
            "one step: the point, snapped"
        );
        let step = driver.click(4.0, 0.3);
        assert_eq!(step.outcome, DONE, "{}", step.message);
        assert_eq!(
            positions_of(&driver, p),
            vec![pt(0.0, 0.0), pt(4.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]
        );
        assert_eq!(driver.document().history().undo_name(), "VERTEX_INSERT");
        assert!(!driver.finished(), "it restarts for the next vertex");
        assert_eq!(driver.document().selection().ids(), vec![p]);
        assert_eq!(
            step.message,
            format!("vertex 1 added to polyline {p} on segment 0 (4 vertices)")
        );
    }

    #[test]
    fn on_an_arc_the_new_vertex_is_on_the_arc() {
        // Bulge +1 from (0,0) to (10,0) is a counter-clockwise half circle
        // about (5,0) of radius 5: it runs below the chord, through (5,-5).
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let mut shape = CurvePolyline2::from_points(&[pt(0.0, 0.0), pt(10.0, 0.0)], false);
        shape.vertices[0].bulge = 1.0;
        let p = add_curve(&mut driver, &shape);
        select_only(&mut driver, vec![p]);
        driver.start("draw.vertex.insert");
        let shown = driver.preview(5.0, -5.2);
        let added = points_of(&shown, FeedbackRole::Added);
        assert_eq!(added.len(), 1);
        assert!(
            close_to(added[0], pt(5.0, -5.0), 1.0e-12),
            "{},{}",
            added[0].x,
            added[0].y
        );
        // A quarter of the circle, 5 pi / 2 along the arc.
        assert_eq!(shown.caption, "new vertex between 0 and 1 · 7.854 from 0");
        // (5,6) is above the chord: the arc's nearest point is an end,
        // sqrt(61) = 7.81 away, far out of reach.
        let away = driver.preview(5.0, 6.0);
        assert!(away.marks.is_empty());
        assert!(!away.refused);

        assert_eq!(driver.click(5.0, -5.2).outcome, DONE);
        let after = shape_of(&driver, p);
        assert_eq!(after.vertices.len(), 3);
        // Each half sweeps a quarter turn: bulge tan(pi/8) = sqrt(2) - 1.
        near(after.vertices[0].bulge, 2.0_f64.sqrt() - 1.0, 1.0e-12);
        near(after.vertices[1].bulge, 2.0_f64.sqrt() - 1.0, 1.0e-12);
        assert!(close_to(after.vertices[1].position, pt(5.0, -5.0), 1.0e-12));
    }

    #[test]
    fn a_selected_polyline_is_not_asked_for() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0)]);
        driver.start("draw.vertex.insert");
        assert_eq!(
            driver.prompt(),
            "Click on a polyline where the new vertex goes"
        );
        select_only(&mut driver, vec![p]);
        driver.start("draw.vertex.insert");
        assert_eq!(
            driver.prompt(),
            format!("Click on polyline {p} where the new vertex goes")
        );
    }

    #[test]
    fn a_hot_vertex_chooses_the_segment_either_side_nearest_the_cursor() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        select_only(&mut driver, vec![p]);
        let handles = vec![vertex_handle(&driver, p, 1)];
        driver.start_with("draw.vertex.insert", handles);
        assert_eq!(
            driver.prompt(),
            format!(
                "Press Enter for a vertex at the marked middle of segment 1, or click beside \
                 vertex 1 of polyline {p}"
            )
        );
        // (7,1) is 1 from segment 0 and 3 from segment 1.
        let west = driver.preview(7.0, 1.0);
        let west_targets = pieces_of(&west, FeedbackRole::Target);
        assert_eq!(west_targets.len(), 1);
        assert!(is_segment(&west_targets[0], pt(0.0, 0.0), pt(10.0, 0.0)));
        assert!(has_point(
            &points_of(&west, FeedbackRole::Added),
            pt(7.0, 0.0),
            EXACT
        ));
        assert!(
            has_point(
                &points_of(&west, FeedbackRole::Target),
                pt(10.0, 0.0),
                EXACT
            ),
            "the hot vertex is marked"
        );
        assert_eq!(labels_of(&west, FeedbackRole::Target), ["1"]);
        // (11,4) is 1 from segment 1 and sqrt(17) from segment 0.
        let north = driver.preview(11.0, 4.0);
        let north_targets = pieces_of(&north, FeedbackRole::Target);
        assert_eq!(north_targets.len(), 1);
        assert!(is_segment(&north_targets[0], pt(10.0, 0.0), pt(10.0, 10.0)));
        assert!(has_point(
            &points_of(&north, FeedbackRole::Added),
            pt(10.0, 4.0),
            EXACT
        ));
    }

    #[test]
    fn enter_with_a_hot_vertex_adds_the_middle_of_the_segment_after_it() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(
            &mut driver,
            &[pt(0.0, 0.0), pt(10.0, 0.0), pt(20.0, 0.0), pt(20.0, 10.0)],
        );
        select_only(&mut driver, vec![p]);
        let handles = vec![vertex_handle(&driver, p, 1)];
        driver.start_with("draw.vertex.insert", handles);
        assert_eq!(driver.enter().outcome, DONE);
        assert_eq!(
            positions_of(&driver, p),
            vec![
                pt(0.0, 0.0),
                pt(10.0, 0.0),
                pt(15.0, 0.0),
                pt(20.0, 0.0),
                pt(20.0, 10.0)
            ]
        );

        // At the last vertex of an open polyline there is no segment after
        // it: the middle of the one before, (20,0)-(20,10).
        let mut end = ToolDriver::new();
        end.set_pick_tolerance(APERTURE);
        let q = add_polyline(
            &mut end,
            &[pt(0.0, 0.0), pt(10.0, 0.0), pt(20.0, 0.0), pt(20.0, 10.0)],
        );
        select_only(&mut end, vec![q]);
        let handles = vec![vertex_handle(&end, q, 3)];
        end.start_with("draw.vertex.insert", handles);
        assert_eq!(
            end.prompt(),
            format!(
                "Press Enter for a vertex at the marked middle of segment 2, or click beside \
                 vertex 3 of polyline {q}"
            )
        );
        assert_eq!(end.enter().outcome, DONE);
        assert_eq!(
            positions_of(&end, q),
            vec![
                pt(0.0, 0.0),
                pt(10.0, 0.0),
                pt(20.0, 0.0),
                pt(20.0, 5.0),
                pt(20.0, 10.0)
            ]
        );
    }

    #[test]
    fn handles_are_used_once_so_a_restarted_tool_does_not_reuse_them() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(
            &mut driver,
            &[pt(0.0, 0.0), pt(10.0, 0.0), pt(20.0, 0.0), pt(20.0, 10.0)],
        );
        select_only(&mut driver, vec![p]);
        let handles = vec![vertex_handle(&driver, p, 1)];
        driver.start_with("draw.vertex.insert", handles);
        assert_eq!(driver.enter().outcome, DONE);
        assert!(!driver.finished());
        // Vertex 1 is still (10,0), but the handle named it before the edit;
        // the restarted tool asks afresh.
        assert_eq!(
            driver.prompt(),
            format!("Click on polyline {p} where the new vertex goes")
        );
        assert_eq!(driver.enter().outcome, DONE, "Enter now ends the tool");
        assert!(driver.finished());
        assert_eq!(driver.executed(), 1);
    }

    #[test]
    fn inserting_near_a_vertex_is_refused_before_the_click() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        select_only(&mut driver, vec![p]);
        driver.start("draw.vertex.insert");
        // (0.2, 0.1) projects to (0.2, 0), 0.2 from vertex 0: within the 0.5.
        let shown = driver.preview(0.2, 0.1);
        assert!(shown.refused);
        assert!(
            shown.caption.starts_with("too close to vertex 0"),
            "{}",
            shown.caption
        );
        assert!(points_of(&shown, FeedbackRole::Added).is_empty());
        // The vertex it is too near is marked, as the refusal, with its
        // number; the whole segment in red flashed at every vertex passed and
        // named none.
        assert!(pieces_of(&shown, FeedbackRole::Target).is_empty());
        assert_eq!(refused_points_of(&shown), vec![pt(0.0, 0.0)]);
        assert_eq!(labels_of(&shown, FeedbackRole::Target), ["0"]);
        let step = driver.click(0.2, 0.1);
        assert_eq!(step.outcome, REJECTED);
        assert_eq!(
            step.message, shown.caption,
            "the click says what the preview said"
        );
        assert_eq!(driver.executed(), 0);
    }

    #[test]
    fn a_far_click_is_refused_and_changes_nothing() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        select_only(&mut driver, vec![p]);
        driver.start("draw.vertex.insert");
        assert!(driver.preview(4.0, 3.0).marks.is_empty());
        assert_eq!(driver.click(4.0, 3.0).outcome, REJECTED);
        assert_eq!(driver.executed(), 0);
        assert_eq!(shape_of(&driver, p).vertices.len(), 3);
    }

    #[test]
    fn a_typed_point_goes_on_the_line_and_a_typed_height_is_kept() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        select_only(&mut driver, vec![p]);
        driver.start("draw.vertex.insert");
        assert_eq!(driver.r#type("4,0.3").outcome, DONE);
        assert_eq!(driver.r#type("6,0.2,101.5").outcome, DONE);
        let after = shape_of(&driver, p);
        assert_eq!(after.vertices.len(), 5);
        assert!(close_to(after.vertices[1].position, pt(4.0, 0.0), EXACT));
        assert!(close_to(after.vertices[2].position, pt(6.0, 0.0), EXACT));
        let Some(height) = after.vertices[2].height else {
            panic!("vertex 2 has no height");
        };
        double_eq(height, 101.5);
        assert!(
            after.vertices[1].height.is_none(),
            "no ends with heights, none invented"
        );
    }

    #[test]
    fn an_edit_made_since_the_choice_is_refused_not_reverted() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        select_only(&mut driver, vec![p]);
        let handles = vec![vertex_handle(&driver, p, 1)];
        driver.start_with("draw.vertex.insert", handles);
        // Another edit - the Vertices panel, an agent - while the tool waits.
        ok(driver.document_mut().execute(edit_polyline(
            p,
            "VERTEX_MOVE",
            |shape: &CurvePolyline2| move_vertex(shape, 2, pt(10.0, 12.0)),
        )));
        let undo_count = driver.document().history().undo_count();
        let step = driver.enter();
        assert_eq!(step.outcome, REJECTED);
        assert_eq!(
            step.message,
            format!("polyline {p} changed since it was picked; pick it again")
        );
        assert_eq!(driver.document().history().undo_count(), undo_count);
        assert_eq!(
            positions_of(&driver, p),
            vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 12.0)],
            "the other edit stays"
        );
        assert_eq!(
            driver.prompt(),
            format!("Click on polyline {p} where the new vertex goes"),
            "the stale choice is dropped"
        );
    }

    #[test]
    fn the_insert_preview_is_what_the_click_commits() {
        // A straight segment (0,0)-(10,0), then an arc of bulge -0.5 to
        // (10,10): clockwise, so it bulges to the left of its way north, with
        // sagitta 0.5 x 5 = 2.5 through (7.5,5), radius (25 + 2.5^2) /
        // (2 x 2.5) = 6.25 about (13.75,5). Every point of a 21 x 21 grid over
        // [-2,12]^2 is previewed and then clicked: a refused preview must be a
        // refused click, an empty one too, and an Added point exactly the
        // vertex the click adds.
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let mut shape =
            CurvePolyline2::from_points(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)], false);
        shape.vertices[1].bulge = -0.5;
        let p = add_curve(&mut driver, &shape);
        select_only(&mut driver, vec![p]);
        driver.start("draw.vertex.insert");
        let mut on_straight = 0;
        let mut on_arc = 0;
        let mut refused = 0;
        let mut out_of_reach = 0;
        for i in 0..=20 {
            for j in 0..=20 {
                let x = -2.0 + 14.0 * f64::from(i) / 20.0;
                let y = -2.0 + 14.0 * f64::from(j) / 20.0;
                let shown = driver.preview(x, y);
                let added = points_of(&shown, FeedbackRole::Added);
                let before = shape_of(&driver, p).vertices.len();
                let step = driver.click(x, y);
                if shown.refused || added.is_empty() {
                    assert_eq!(step.outcome, REJECTED, "{x},{y}");
                    assert_eq!(shape_of(&driver, p).vertices.len(), before);
                    if shown.refused {
                        refused += 1;
                    } else {
                        out_of_reach += 1;
                    }
                    continue;
                }
                assert_eq!(step.outcome, DONE, "{x},{y} {}", step.message);
                let after = shape_of(&driver, p);
                assert_eq!(after.vertices.len(), before + 1, "{x},{y}");
                assert!(
                    after
                        .vertices
                        .iter()
                        .any(|vertex| vertex.position == added[0]),
                    "the Added point is exactly the vertex made, at {x},{y}"
                );
                let targets = pieces_of(&shown, FeedbackRole::Target);
                assert_eq!(targets.len(), 1);
                if matches!(targets[0], Geometry::Arc(_)) {
                    on_arc += 1;
                } else {
                    on_straight += 1;
                }
                ok(driver.document_mut().undo());
            }
        }
        println!(
            "insert grid: {on_straight} on the straight segment, {on_arc} on the arc, \
             {refused} refused near a vertex, {out_of_reach} out of reach"
        );
        assert!(on_straight > 0);
        assert!(on_arc > 0);
        assert!(refused > 0);
        assert!(out_of_reach > 0);
    }

    // ---- what is already chosen ----------------------------------------------------------

    #[test]
    fn a_preselected_polyline_is_never_asked_for_again() {
        for id in [
            "draw.vertex.insert",
            "draw.vertex.delete",
            "draw.vertex.move",
            "draw.vertex.edit",
            "draw.vertex.straighten",
            "draw.vertex.start",
            "draw.vertex.height",
            "draw.vertex.grade",
            "draw.vertex.arc",
            "draw.vertex.line",
            "draw.vertex.fillet",
            "draw.vertex.chamfer",
        ] {
            let mut driver = ToolDriver::new();
            driver.set_pick_tolerance(APERTURE);
            let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
            select_only(&mut driver, vec![p]);
            driver.start(id);
            let prompt = driver.prompt();
            assert!(!prompt.contains("Select the polyline"), "{id}: {prompt}");
            assert!(
                prompt.contains(&format!("olyline {p}")),
                "{id} names the polyline it has: {prompt}"
            );
        }
    }

    #[test]
    fn a_hot_vertex_answers_delete_and_enter_deletes_it_in_one_step() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(5.0, 3.0), pt(10.0, 0.0)]);
        select_only(&mut driver, vec![p]);
        let handles = vec![vertex_handle(&driver, p, 1)];
        driver.start_with("draw.vertex.delete", handles);
        assert_eq!(
            driver.prompt(),
            format!("Press Enter to delete vertex 1 of polyline {p}, or click another vertex")
        );
        let undo_count = driver.document().history().undo_count();
        assert_eq!(driver.enter().outcome, DONE);
        assert_eq!(positions_of(&driver, p), vec![pt(0.0, 0.0), pt(10.0, 0.0)]);
        assert_eq!(driver.document().history().undo_count(), undo_count + 1);
        assert_eq!(driver.document().history().undo_name(), "VERTEX_DELETE");
        assert_eq!(
            driver.prompt(),
            format!("Click the vertex of polyline {p} to delete"),
            "the restarted tool asks for a click"
        );
    }

    #[test]
    fn several_hot_vertices_go_in_one_step() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(
            &mut driver,
            &[
                pt(0.0, 0.0),
                pt(1.0, 1.0),
                pt(2.0, 0.0),
                pt(3.0, 1.0),
                pt(4.0, 0.0),
            ],
        );
        select_only(&mut driver, vec![p]);
        let handles = vec![vertex_handle(&driver, p, 1), vertex_handle(&driver, p, 2)];
        driver.start_with("draw.vertex.delete", handles);
        assert_eq!(
            driver.prompt(),
            "Press Enter to delete the 2 chosen vertices, or click another vertex"
        );
        // (8,8) is far from every vertex: a click there takes nothing, so the
        // preview is what Enter would take, and says it is Enter's.
        let shown = driver.preview(8.0, 8.0);
        assert_eq!(
            points_of(&shown, FeedbackRole::Removed).len(),
            2,
            "what Enter would take"
        );
        assert_eq!(shown.caption, "Enter: delete 2 vertices");
        let undo_count = driver.document().history().undo_count();
        assert_eq!(driver.enter().outcome, DONE);
        assert_eq!(
            positions_of(&driver, p),
            vec![pt(0.0, 0.0), pt(3.0, 1.0), pt(4.0, 0.0)]
        );
        assert_eq!(driver.document().history().undo_count(), undo_count + 1);
    }

    #[test]
    fn delete_reaches_only_a_vertex_near_the_cursor() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(20.0, 0.0)]);
        driver.start("draw.vertex.delete");
        // (5,0) is ON the polyline, but 5 from the nearest vertex: before,
        // that deleted vertex 0 or 1.
        assert!(driver.preview(5.0, 0.0).marks.is_empty());
        assert_eq!(driver.pick(p, 5.0, 0.0).outcome, REJECTED);
        assert_eq!(shape_of(&driver, p).vertices.len(), 3);
        assert_eq!(driver.executed(), 0);
    }

    #[test]
    fn a_survey_point_on_a_vertex_does_not_steal_the_pick() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(20.0, 0.0)]);
        let mut mark = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: pt(10.0, 0.0),
            }),
            ..Entity::default()
        };
        mark.properties
            .insert(String::from("point"), PropertyValue::from("7"));
        let survey = driver.add(create_entities(vec![mark]));
        driver.start("draw.vertex.delete");
        // The view hands over whatever it picked - the point, drawn on top.
        let step = driver.pick(survey, 10.0, 0.1);
        assert_eq!(step.outcome, DONE, "{}", step.message);
        assert_eq!(positions_of(&driver, p), vec![pt(0.0, 0.0), pt(20.0, 0.0)]);
        assert!(driver.document().model().entities.find(survey).is_some());
    }

    #[test]
    fn a_typed_number_names_the_vertex() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(20.0, 0.0)]);
        select_only(&mut driver, vec![p]);
        driver.start("draw.vertex.delete");
        assert_eq!(driver.r#type("7").outcome, REJECTED, "no vertex 7");
        assert_eq!(driver.r#type("1").outcome, DONE);
        assert_eq!(positions_of(&driver, p), vec![pt(0.0, 0.0), pt(20.0, 0.0)]);
    }

    #[test]
    fn a_typed_point_answers_a_pick() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(20.0, 0.0)]);
        driver.start("draw.vertex.delete");
        assert_eq!(driver.r#type("10.2,0").outcome, DONE);
        assert_eq!(positions_of(&driver, p), vec![pt(0.0, 0.0), pt(20.0, 0.0)]);
    }

    #[test]
    fn two_hot_vertices_answer_both_picks_of_straighten() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(
            &mut driver,
            &[
                pt(0.0, 0.0),
                pt(1.0, 1.0),
                pt(2.0, -1.0),
                pt(3.0, 1.0),
                pt(4.0, 0.0),
            ],
        );
        select_only(&mut driver, vec![p]);
        let handles = vec![vertex_handle(&driver, p, 0), vertex_handle(&driver, p, 4)];
        driver.start_with("draw.vertex.straighten", handles);
        assert_eq!(
            driver.prompt(),
            format!(
                "Press Enter to straighten polyline {p} from vertex 0 to 4, or click another \
                 vertex"
            )
        );
        assert_eq!(driver.enter().outcome, DONE);
        assert_eq!(positions_of(&driver, p), vec![pt(0.0, 0.0), pt(4.0, 0.0)]);
    }

    #[test]
    fn one_hot_vertex_answers_the_first_pick_of_move() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(5.0, 0.0), pt(10.0, 0.0)]);
        select_only(&mut driver, vec![p]);
        let handles = vec![vertex_handle(&driver, p, 1)];
        driver.start_with("draw.vertex.move", handles);
        assert_eq!(driver.expects(), ToolInput::Point);
        assert_eq!(driver.prompt(), "Specify its new position");
        assert_eq!(driver.r#type("@0,2").outcome, DONE, "@ is from the vertex");
        assert_eq!(
            positions_of(&driver, p),
            vec![pt(0.0, 0.0), pt(5.0, 2.0), pt(10.0, 0.0)]
        );

        // Several hot vertices: which one to move is not known, and the tool
        // says so.
        let mut several = ToolDriver::new();
        several.set_pick_tolerance(APERTURE);
        let q = add_polyline(&mut several, &[pt(0.0, 0.0), pt(5.0, 0.0), pt(10.0, 0.0)]);
        select_only(&mut several, vec![q]);
        let handles = vec![vertex_handle(&several, q, 0), vertex_handle(&several, q, 1)];
        several.start_with("draw.vertex.move", handles);
        assert_eq!(
            several.prompt(),
            format!(
                "2 vertices are chosen, and this tool moves one. Click the vertex of polyline \
                 {q} to move"
            )
        );
        assert_eq!(several.expects(), ToolInput::Entity);
    }

    #[test]
    fn a_click_after_the_handles_answered_picks_anew() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(
            &mut driver,
            &[pt(0.0, 0.0), pt(5.0, 3.0), pt(10.0, 0.0), pt(15.0, 3.0)],
        );
        select_only(&mut driver, vec![p]);
        let handles = vec![vertex_handle(&driver, p, 1)];
        driver.start_with("draw.vertex.delete", handles);
        assert_eq!(driver.click(10.1, 0.0).outcome, DONE);
        assert_eq!(
            positions_of(&driver, p),
            vec![pt(0.0, 0.0), pt(5.0, 3.0), pt(15.0, 3.0)],
            "the vertex clicked goes, not the hot one"
        );
    }

    #[test]
    fn edit_vertices_with_a_selected_polyline_does_not_ask_for_one() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        select_only(&mut driver, vec![p]);
        driver.start("draw.vertex.edit");
        assert_eq!(
            driver.prompt(),
            format!(
                "Polyline {p} is in the Vertices panel; click another polyline, or press Enter"
            )
        );
        // Over nothing a click takes nothing: the preview is what Enter keeps
        // in the panel, and says it is Enter's, as every tool answered by what
        // was chosen before it does.
        let shown = driver.preview(50.0, 50.0);
        assert_eq!(
            shown.caption,
            format!("Enter: polyline {p} · 3 vertices, open")
        );
        assert_eq!(driver.enter().outcome, DONE);
        assert!(driver.finished());
        assert_eq!(driver.executed(), 0);
        assert_eq!(driver.document().selection().ids(), vec![p]);
    }

    #[test]
    fn the_selection_prompt_counts_in_the_singular() {
        let mut driver = ToolDriver::new();
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        select_only(&mut driver, vec![p]);
        driver.start("draw.vertex.close");
        assert_eq!(
            driver.prompt(),
            "Press Enter to apply to the 1 selected polyline"
        );
    }

    // ---- every other one-polyline tool shows what it will do -------------------------------

    #[test]
    fn delete_shows_the_vertex_that_goes_and_the_segment_that_joins() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(5.0, 3.0), pt(10.0, 0.0)]);
        driver.start("draw.vertex.delete");
        let shown = driver.preview(5.2, 2.9);
        assert_eq!(points_of(&shown, FeedbackRole::Removed), vec![pt(5.0, 3.0)]);
        let removed = pieces_of(&shown, FeedbackRole::Removed);
        assert_eq!(removed.len(), 2);
        assert!(is_segment(&removed[0], pt(0.0, 0.0), pt(5.0, 3.0)));
        assert!(is_segment(&removed[1], pt(5.0, 3.0), pt(10.0, 0.0)));
        assert_eq!(shown.shapes.len(), 1);
        assert!(is_segment(&shown.shapes[0], pt(0.0, 0.0), pt(10.0, 0.0)));
        assert_eq!(
            shown.caption,
            "delete vertex 1 · segments 0 and 1 become one"
        );
        assert_eq!(shown.focus, Some(p));
    }

    #[test]
    fn move_shows_the_vertex_where_it_was_and_where_it_goes() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(5.0, 0.0), pt(10.0, 0.0)]);
        driver.start("draw.vertex.move");
        // Before the pick: only what the pick would take.
        let hover = driver.preview(5.0, 0.2);
        assert_eq!(points_of(&hover, FeedbackRole::Target), vec![pt(5.0, 0.0)]);
        assert_eq!(labels_of(&hover, FeedbackRole::Target), ["1"]);
        assert_eq!(driver.pick(p, 5.0, 0.2).outcome, CONTINUE);
        let shown = driver.preview(5.0, 3.0);
        assert_eq!(points_of(&shown, FeedbackRole::Target), vec![pt(5.0, 0.0)]);
        assert_eq!(points_of(&shown, FeedbackRole::Added), vec![pt(5.0, 3.0)]);
        assert_eq!(shown.shapes.len(), 2);
        assert!(is_segment(&shown.shapes[0], pt(0.0, 0.0), pt(5.0, 3.0)));
        assert!(is_segment(&shown.shapes[1], pt(5.0, 3.0), pt(10.0, 0.0)));
        assert!(
            shown.caption.starts_with("vertex 1 · moves 3.000"),
            "{}",
            shown.caption
        );
        // x,y,z: the height too.
        assert_eq!(driver.r#type("5,3,7.25").outcome, DONE);
        let after = shape_of(&driver, p);
        assert_eq!(after.vertices[1].position, pt(5.0, 3.0));
        let Some(height) = after.vertices[1].height else {
            panic!("vertex 1 has no height");
        };
        double_eq(height, 7.25);
    }

    #[test]
    fn straighten_marks_what_goes_and_takes_the_shorter_side_of_a_closed_polyline() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(
            &mut driver,
            &[
                pt(0.0, 0.0),
                pt(1.0, 1.0),
                pt(2.0, -1.0),
                pt(3.0, 1.0),
                pt(4.0, 0.0),
            ],
        );
        driver.start("draw.vertex.straighten");
        assert_eq!(driver.pick(p, 0.0, 0.0).outcome, CONTINUE);
        let shown = driver.preview(4.0, 0.1);
        assert_eq!(
            points_of(&shown, FeedbackRole::Removed),
            vec![pt(1.0, 1.0), pt(2.0, -1.0), pt(3.0, 1.0)]
        );
        assert_eq!(labels_of(&shown, FeedbackRole::Target), ["keep", "keep"]);
        assert_eq!(shown.shapes.len(), 1);
        assert!(is_segment(&shown.shapes[0], pt(0.0, 0.0), pt(4.0, 0.0)));
        assert_eq!(
            pieces_of(&shown, FeedbackRole::Removed).len(),
            4,
            "the path that goes"
        );

        // A closed hexagon picked at vertex 3 then vertex 1: forward from 3 is
        // 4, 5 and 0; the other way only 2. The shorter side goes.
        let mut ring = ToolDriver::new();
        ring.set_pick_tolerance(APERTURE);
        let hexagon = [
            pt(0.0, 0.0),
            pt(10.0, 0.0),
            pt(20.0, 0.0),
            pt(20.0, 10.0),
            pt(10.0, 10.0),
            pt(0.0, 10.0),
        ];
        let h = add_polyline_of(&mut ring, &hexagon, true, &[]);
        ring.start("draw.vertex.straighten");
        assert_eq!(ring.pick(h, 20.0, 10.0).outcome, CONTINUE);
        assert!(ring.prompt().ends_with("[Other side]"), "{}", ring.prompt());
        assert_eq!(
            points_of(&ring.preview(10.0, 0.1), FeedbackRole::Removed),
            vec![pt(20.0, 0.0)]
        );
        assert_eq!(ring.click(10.0, 0.1).outcome, DONE);
        assert_eq!(
            positions_of(&ring, h),
            vec![
                pt(0.0, 0.0),
                pt(10.0, 0.0),
                pt(20.0, 10.0),
                pt(10.0, 10.0),
                pt(0.0, 10.0)
            ]
        );

        // O takes the other side instead: 4, 5 and 0 go.
        let mut other = ToolDriver::new();
        other.set_pick_tolerance(APERTURE);
        let o = add_polyline_of(&mut other, &hexagon, true, &[]);
        other.start("draw.vertex.straighten");
        assert_eq!(other.pick(o, 20.0, 10.0).outcome, CONTINUE);
        assert_eq!(other.r#type("O").outcome, CONTINUE);
        assert_eq!(
            points_of(&other.preview(10.0, 0.1), FeedbackRole::Removed).len(),
            3
        );
        assert_eq!(other.click(10.0, 0.1).outcome, DONE);
        assert_eq!(
            positions_of(&other, o),
            vec![pt(10.0, 0.0), pt(20.0, 0.0), pt(20.0, 10.0)]
        );
    }

    #[test]
    fn grade_shows_the_heights_it_will_set() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline_of(
            &mut driver,
            &[pt(0.0, 0.0), pt(5.0, 0.0), pt(10.0, 0.0)],
            false,
            &[Some(100.0), None, Some(101.0)],
        );
        driver.start("draw.vertex.grade");
        // A vertex with no height is refused at the pick.
        let no_height = driver.preview(5.0, 0.1);
        assert!(no_height.refused);
        assert_eq!(
            no_height.caption,
            "vertex 1 has no height; grading needs one at both ends"
        );
        assert_eq!(driver.pick(p, 5.0, 0.1).outcome, REJECTED);
        assert_eq!(driver.pick(p, 0.0, 0.1).outcome, CONTINUE);
        let shown = driver.preview(10.0, 0.1);
        // 1 m over 10 m: vertex 1, halfway, at 100.5; a grade of 10 %.
        assert_eq!(
            labels_of(&shown, FeedbackRole::Target),
            ["z 100.000", "z 101.000", "→ 100.500"]
        );
        assert_eq!(
            shown.caption,
            "grade 0 → 2: 1 vertex, 100.000 to 101.000 (10.000 %)"
        );
        assert_eq!(driver.click(10.0, 0.1).outcome, DONE);
        near(height_of(&driver, p, 1), 100.5, 1.0e-12);
    }

    #[test]
    fn set_height_defaults_to_the_height_it_has_or_would_have() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline_of(
            &mut driver,
            &[pt(0.0, 0.0), pt(5.0, 0.0), pt(10.0, 0.0)],
            false,
            &[Some(0.0), None, Some(10.0)],
        );
        let mut spot = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: pt(30.0, 30.0),
            }),
            ..Entity::default()
        };
        set_heights(&mut spot.properties, &[Some(7.5)]);
        driver.add(create_entities(vec![spot]));
        driver.start("draw.vertex.height");
        // Vertex 1 has none: halfway between 0 and 10 by length is 5.
        assert_eq!(driver.pick(p, 5.0, 0.1).outcome, CONTINUE);
        assert!(driver.prompt().ends_with("<5>"), "{}", driver.prompt());
        // A click takes the height of what it lands on.
        assert_eq!(driver.click(30.1, 30.0).outcome, DONE);
        double_eq(height_of(&driver, p, 1), 7.5);

        // Enter at a vertex's own height changes nothing: no undo step.
        let undo_count = driver.document().history().undo_count();
        assert_eq!(driver.pick(p, 0.0, 0.1).outcome, CONTINUE);
        assert!(driver.prompt().ends_with("<0>"), "{}", driver.prompt());
        assert_eq!(driver.enter().outcome, DONE);
        assert_eq!(driver.document().history().undo_count(), undo_count);
    }

    #[test]
    fn change_start_shows_the_new_first_vertex_and_refuses_an_open_polyline() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline_of(
            &mut driver,
            &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)],
            true,
            &[],
        );
        let open = add_polyline(&mut driver, &[pt(0.0, 20.0), pt(10.0, 20.0)]);
        driver.start("draw.vertex.start");
        let shown = driver.preview(10.1, 10.0);
        assert_eq!(labels_of(&shown, FeedbackRole::Target), ["new 0"]);
        assert_eq!(shown.caption, "vertex 2 becomes vertex 0");
        let open_end = driver.preview(0.1, 20.0);
        assert!(open_end.refused, "an open polyline has no start to change");
        assert_eq!(
            labels_of(&open_end, FeedbackRole::Target),
            ["0"],
            "the vertex refused is named, not labelled with the start it is refused"
        );
        assert_eq!(driver.pick(open, 0.1, 20.0).outcome, REJECTED);
        assert_eq!(driver.pick(p, 10.1, 10.0).outcome, DONE);
        assert_eq!(shape_of(&driver, p).vertices[0].position, pt(10.0, 10.0));
    }

    #[test]
    fn segment_to_arc_shows_the_segment_then_the_arc() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(20.0, 0.0)]);
        driver.start("draw.vertex.arc");
        let segment = driver.preview(10.0, 0.1);
        assert_eq!(pieces_of(&segment, FeedbackRole::Target).len(), 1);
        assert!(
            is_segment(
                &pieces_of(&segment, FeedbackRole::Target)[0],
                pt(0.0, 0.0),
                pt(20.0, 0.0)
            ),
            "the segment, not a vertex"
        );
        assert_eq!(driver.pick(p, 10.0, 0.1).outcome, CONTINUE);
        // Through (10,10): the half circle about (10,0) of radius 10,
        // clockwise from (0,0) over the top, so bulge -1.
        let arc = driver.preview(10.0, 10.0);
        assert_eq!(arc.shapes.len(), 1);
        let Geometry::Arc(shape) = &arc.shapes[0] else {
            panic!("not an arc: {:?}", arc.shapes[0]);
        };
        assert!(close_to(shape.center, pt(10.0, 0.0), 1.0e-9));
        near(shape.radius, 10.0, 1.0e-9);
        assert_eq!(arc.caption, "segment 0 · radius 10.000");
        // In line with the ends: refused, before and at the click.
        assert!(driver.preview(10.0, 0.0).refused);
        assert_eq!(driver.click(10.0, 0.0).outcome, REJECTED);
        assert_eq!(driver.click(10.0, 10.0).outcome, DONE);
        near(shape_of(&driver, p).vertices[0].bulge, -1.0, 1.0e-12);

        // A hot segment answers the pick. Through (5,2) over (0,0)-(10,0): the
        // centre (5,k) with 25 + k^2 = (2 - k)^2, so k = -5.25 and r = 7.25.
        let mut hot = ToolDriver::new();
        hot.set_pick_tolerance(APERTURE);
        let q = add_polyline(&mut hot, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 5.0)]);
        select_only(&mut hot, vec![q]);
        hot.start_with(
            "draw.vertex.arc",
            vec![Grip {
                entity: q,
                kind: GripKind::SegmentMid,
                index: 0,
                position: pt(5.0, 0.0),
            }],
        );
        assert_eq!(hot.expects(), ToolInput::Point);
        let through = hot.preview(5.0, 2.0);
        assert_eq!(through.shapes.len(), 1);
        let Geometry::Arc(bent) = &through.shapes[0] else {
            panic!("not an arc: {:?}", through.shapes[0]);
        };
        assert!(close_to(bent.center, pt(5.0, -5.25), 1.0e-9));
        near(bent.radius, 7.25, 1.0e-9);
    }

    #[test]
    fn segment_to_line_shows_the_chord_and_refuses_a_straight_segment() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let mut shape =
            CurvePolyline2::from_points(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(20.0, 0.0)], false);
        shape.vertices[0].bulge = 1.0; // below the chord, through (5,-5)
        let p = add_curve(&mut driver, &shape);
        driver.start("draw.vertex.line");
        let shown = driver.preview(5.0, -5.1);
        assert_eq!(pieces_of(&shown, FeedbackRole::Target).len(), 1);
        assert!(matches!(
            pieces_of(&shown, FeedbackRole::Target)[0],
            Geometry::Arc(_)
        ));
        assert_eq!(shown.shapes.len(), 1);
        assert!(is_segment(&shown.shapes[0], pt(0.0, 0.0), pt(10.0, 0.0)));
        let straight = driver.preview(15.0, 0.1);
        assert!(straight.refused);
        assert_eq!(straight.caption, "segment 1 is already straight");
        assert_eq!(driver.pick(p, 15.0, 0.1).outcome, REJECTED);
    }

    #[test]
    fn fillet_shows_the_arc_at_the_last_radius_and_refuses_an_end_at_the_pick() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        // Make 1 the remembered radius, as a previous use would.
        let scratch = add_polyline(
            &mut driver,
            &[pt(100.0, 0.0), pt(110.0, 0.0), pt(110.0, 10.0)],
        );
        driver.start("draw.vertex.fillet");
        assert_eq!(driver.pick(scratch, 110.0, 0.1).outcome, CONTINUE);
        assert_eq!(driver.r#type("1").outcome, DONE);

        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        select_only(&mut driver, vec![p]);
        driver.start("draw.vertex.fillet");
        // The corner (10,0) at radius 1: tangent 1 back along each side, at
        // (9,0) and (10,1), the arc about (9,1).
        let shown = driver.preview(10.0, 0.1);
        assert_eq!(
            points_of(&shown, FeedbackRole::Removed),
            vec![pt(10.0, 0.0)]
        );
        let added = points_of(&shown, FeedbackRole::Added);
        assert_eq!(added.len(), 2);
        assert!(has_point(&added, pt(9.0, 0.0), 1.0e-12));
        assert!(has_point(&added, pt(10.0, 1.0), 1.0e-12));
        assert_eq!(shown.shapes.len(), 1);
        let Geometry::Arc(arc) = &shown.shapes[0] else {
            panic!("not an arc: {:?}", shown.shapes[0]);
        };
        assert!(close_to(arc.center, pt(9.0, 1.0), 1.0e-9));
        near(arc.radius, 1.0, 1.0e-9);
        // An end vertex: refused at the pick, and no radius asked for.
        let before = driver.prompt();
        assert!(driver.preview(0.0, 0.1).refused);
        assert_eq!(driver.pick(p, 0.0, 0.1).outcome, REJECTED);
        assert_eq!(driver.prompt(), before);
        assert_eq!(driver.expects(), ToolInput::Entity);
    }

    #[test]
    fn a_fillets_corner_is_marked_as_the_vertex_that_goes_not_as_a_taken_one() {
        // The corner (10,0) of (0,0) (10,0) (10,10) goes, replaced by the arc:
        // its X, with no ring. What the pick acts on is the two segments the
        // arc is tangent to, (0,0)-(10,0) and (10,0)-(10,10), as Chamfer marks
        // its two. It was a Target ring AND the X on the corner: a circled X,
        // the sign for "cancel", and at the default radius - the arc a pixel or
        // two across - all the preview showed; beside the refusal's struck
        // ring it said "no" of a corner that was taken.
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        select_only(&mut driver, vec![p]);
        driver.start("draw.vertex.fillet");
        let shown = driver.preview(10.0, 0.1);
        assert!(!shown.refused);
        assert_eq!(
            points_of(&shown, FeedbackRole::Removed),
            vec![pt(10.0, 0.0)]
        );
        assert!(
            points_of(&shown, FeedbackRole::Target).is_empty(),
            "no ring on the corner that goes"
        );
        let pieces = pieces_of(&shown, FeedbackRole::Target);
        assert_eq!(pieces.len(), 2);
        assert!(is_segment(&pieces[0], pt(0.0, 0.0), pt(10.0, 0.0)));
        assert!(is_segment(&pieces[1], pt(10.0, 0.0), pt(10.0, 10.0)));
        // An end is refused as the vertex itself, named by its number and
        // drawn struck (every Target of a refused preview that flags none).
        let end = driver.preview(0.0, 0.1);
        assert!(end.refused);
        assert_eq!(points_of(&end, FeedbackRole::Target), vec![pt(0.0, 0.0)]);
        assert_eq!(labels_of(&end, FeedbackRole::Target), ["0"]);
        assert!(pieces_of(&end, FeedbackRole::Target).is_empty());
        // Chamfer's end, the same: the vertex struck with its number, where it
        // struck the one segment an end has, labelled with a distance, "d2",
        // that the refusal never asks for.
        driver.start("draw.vertex.chamfer");
        let chamfer_end = driver.preview(0.0, 0.1);
        assert!(chamfer_end.refused);
        assert_eq!(
            points_of(&chamfer_end, FeedbackRole::Target),
            vec![pt(0.0, 0.0)]
        );
        assert_eq!(labels_of(&chamfer_end, FeedbackRole::Target), ["0"]);
        assert!(pieces_of(&chamfer_end, FeedbackRole::Target).is_empty());
    }

    #[test]
    fn chamfer_marks_its_first_and_second_segments() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        driver.start("draw.vertex.chamfer");
        let shown = driver.preview(10.0, 0.1);
        let targets = pieces_of(&shown, FeedbackRole::Target);
        assert_eq!(targets.len(), 2);
        assert!(is_segment(&targets[0], pt(0.0, 0.0), pt(10.0, 0.0)));
        assert!(is_segment(&targets[1], pt(10.0, 0.0), pt(10.0, 10.0)));
        assert_eq!(
            labels_of(&shown, FeedbackRole::Target),
            ["d1", "d2"],
            "the distances along them, not numbers that read as segment numbers"
        );
        assert_eq!(driver.pick(p, 10.0, 0.1).outcome, CONTINUE);
        assert!(
            driver
                .prompt()
                .starts_with("Specify the first distance, along segment 0"),
            "{}",
            driver.prompt()
        );
        assert_eq!(driver.r#type("1").outcome, CONTINUE);
        assert!(
            driver
                .prompt()
                .starts_with("Specify the second distance, along segment 1"),
            "{}",
            driver.prompt()
        );
        // The preview at the second distance's default shows the bevel from
        // (9,0); the commit with 2 is the bevel (9,0)-(10,2).
        assert_eq!(driver.r#type("2").outcome, DONE);
        assert_eq!(
            positions_of(&driver, p),
            vec![pt(0.0, 0.0), pt(9.0, 0.0), pt(10.0, 2.0), pt(10.0, 10.0)]
        );
    }

    // One case of a_preview_never_promises_what_the_click_does_not.
    struct Case {
        tool: &'static str,
        closed: bool,
        bulge: f64,                     // of segment 0
        first: &'static [&'static str], // typed before the grid's step
        hot: &'static [usize],          // vertices chosen before the tool
        hot_segment: Option<usize>,     // a segment's middle chosen before the tool
        heights: bool,                  // 100, 101, 102 and 103 at the vertices
    }

    const CASE: Case = Case {
        tool: "",
        closed: false,
        bulge: 0.0,
        first: &[],
        hot: &[],
        hot_segment: None,
        heights: false,
    };

    #[test]
    fn a_preview_never_promises_what_the_click_does_not() {
        // Every pick tool but Insert - whose own finer grid is
        // the_insert_preview_is_what_the_click_commits - at each step a click
        // answers, over a 9 x 9 grid round a small polyline, with and without
        // grips chosen before the tool:
        //   * a refused preview, one with no mark, or the tool's idle picture
        //     (as far off, where nothing is in reach) is a refused click;
        //   * one captioned "Enter: " is a refused click that leaves the tool
        //     where it was, and Enter then does what was shown;
        //   * ANY other preview is a click the tool takes: it is done, or goes
        //     on to its next step - where that step has a default, Enter takes
        //     it.
        // Once done, the edit is the one shown: every Added point a vertex,
        // every Removed vertex gone, every ghost a piece of the result, "new 0"
        // the start, a "→ z" label the height given, Set Height's caption the
        // height set, Edit Vertices' polyline selected. And no vertex is marked
        // both taken and gone: a ring with an X over it is a circled X,
        // "cancel". It asserted only where Added, Removed or a ghost was
        // shown, and so checked nothing of Change Start, Set Height or Edit
        // Vertices.
        let cases = [
            Case {
                tool: "draw.vertex.delete",
                ..CASE
            },
            Case {
                tool: "draw.vertex.delete",
                closed: true,
                ..CASE
            },
            Case {
                tool: "draw.vertex.delete",
                closed: true,
                hot: &[2],
                ..CASE
            },
            Case {
                tool: "draw.vertex.delete",
                hot: &[1, 2],
                ..CASE
            },
            Case {
                tool: "draw.vertex.move",
                ..CASE
            },
            Case {
                tool: "draw.vertex.move",
                first: &["1"],
                ..CASE
            },
            Case {
                tool: "draw.vertex.move",
                closed: true,
                hot: &[2],
                ..CASE
            },
            Case {
                tool: "draw.vertex.edit",
                ..CASE
            },
            Case {
                tool: "draw.vertex.straighten",
                closed: true,
                ..CASE
            },
            Case {
                tool: "draw.vertex.straighten",
                closed: true,
                first: &["0"],
                ..CASE
            },
            Case {
                tool: "draw.vertex.straighten",
                closed: true,
                hot: &[0, 2],
                ..CASE
            },
            Case {
                tool: "draw.vertex.start",
                closed: true,
                ..CASE
            },
            Case {
                tool: "draw.vertex.start",
                closed: true,
                hot: &[2],
                ..CASE
            },
            Case {
                tool: "draw.vertex.start",
                ..CASE
            },
            Case {
                tool: "draw.vertex.height",
                heights: true,
                ..CASE
            },
            Case {
                tool: "draw.vertex.height",
                first: &["1"],
                heights: true,
                ..CASE
            },
            Case {
                tool: "draw.vertex.grade",
                closed: true,
                ..CASE
            },
            Case {
                tool: "draw.vertex.grade",
                closed: true,
                first: &["0"],
                heights: true,
                ..CASE
            },
            Case {
                tool: "draw.vertex.arc",
                ..CASE
            },
            Case {
                tool: "draw.vertex.arc",
                first: &["0"],
                ..CASE
            },
            Case {
                tool: "draw.vertex.line",
                bulge: 0.5,
                ..CASE
            },
            Case {
                tool: "draw.vertex.line",
                bulge: 0.5,
                hot_segment: Some(0),
                ..CASE
            },
            Case {
                tool: "draw.vertex.fillet",
                ..CASE
            },
            Case {
                tool: "draw.vertex.fillet",
                closed: true,
                ..CASE
            },
            Case {
                tool: "draw.vertex.chamfer",
                closed: true,
                ..CASE
            },
        ];
        let corners = [pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)];
        let zs = [100.0, 101.0, 102.0, 103.0];
        let mut refused_ever = 0;
        let mut enter_ever = 0;
        for c in &cases {
            let mut taken = 0;
            let mut finished = 0;
            let mut refused = 0;
            let mut enter_shown = 0;
            for i in 0..=8 {
                for j in 0..=8 {
                    let x = -1.0 + 12.0 * f64::from(i) / 8.0;
                    let y = -1.0 + 12.0 * f64::from(j) / 8.0;
                    let at = format!(
                        "{} at {},{}",
                        c.tool,
                        format_exact_real(x),
                        format_exact_real(y)
                    );
                    let mut driver = ToolDriver::new();
                    driver.set_pick_tolerance(1.0); // a vertex within 1.5, a segment within 1
                    let mut shape = CurvePolyline2::from_points(&corners, c.closed);
                    shape.vertices[0].bulge = c.bulge;
                    if c.heights {
                        for (vertex, &z) in shape.vertices.iter_mut().zip(&zs) {
                            vertex.height = Some(z);
                        }
                    }
                    let p = add_curve(&mut driver, &shape);
                    select_only(&mut driver, vec![p]);
                    let mut handles: Vec<Grip> = c
                        .hot
                        .iter()
                        .map(|&v| vertex_handle(&driver, p, v))
                        .collect();
                    if let Some(segment) = c.hot_segment {
                        let middle = shape.segment(segment).point_at(0.5);
                        handles.push(Grip {
                            entity: p,
                            kind: GripKind::SegmentMid,
                            index: segment,
                            position: middle,
                        });
                    }
                    driver.start_with(c.tool, handles);
                    for &typed in c.first {
                        assert_eq!(driver.r#type(typed).outcome, CONTINUE, "{at}");
                    }
                    let shown = driver.preview(x, y);
                    let away = driver.preview(1000.0, 1000.0);
                    let prompt = driver.prompt();
                    let mut step = driver.click(x, y);
                    let promised = !shown.caption.starts_with("Enter: ")
                        && !shown.refused
                        && !shown.marks.is_empty()
                        && describe(&shown) != describe(&away);
                    if shown.caption.starts_with("Enter: ") {
                        // Nothing a click takes is here: refused, the tool
                        // where it was, and Enter does what was shown - or is
                        // refused as it showed.
                        assert_eq!(step.outcome, REJECTED, "{at}");
                        assert_eq!(driver.prompt(), prompt, "{at}: the tool is kept");
                        step = driver.enter();
                        enter_shown += 1;
                        if shown.refused {
                            assert_eq!(step.outcome, REJECTED, "{at}");
                            continue;
                        }
                    } else if !promised {
                        assert_eq!(step.outcome, REJECTED, "{at} shows {}", describe(&shown));
                        if shown.refused {
                            refused += 1;
                        }
                        continue;
                    } else {
                        assert_ne!(
                            step.outcome,
                            REJECTED,
                            "{at} showed {} and said {}",
                            describe(&shown),
                            step.message
                        );
                        taken += 1;
                    }
                    // A next step with a default (a radius, two distances, a
                    // height) is Enter's; one that wants a point or a pick
                    // ends the case, the click having been taken.
                    for _ in 0..3 {
                        if step.outcome != CONTINUE || !driver.prompt().ends_with('>') {
                            break;
                        }
                        step = driver.enter();
                    }
                    if step.outcome != DONE {
                        assert_eq!(step.outcome, CONTINUE, "{at}: {}", step.message);
                        continue;
                    }
                    finished += 1;
                    let after = shape_of(&driver, p);
                    let positions = after.positions();
                    for added in points_of(&shown, FeedbackRole::Added) {
                        assert!(has_point(&positions, added, EXACT), "{at}");
                    }
                    let targets = points_of(&shown, FeedbackRole::Target);
                    for gone in points_of(&shown, FeedbackRole::Removed) {
                        assert!(!has_point(&positions, gone, EXACT), "{at}");
                        assert!(
                            !has_point(&targets, gone, EXACT),
                            "{at}: a vertex marked taken and gone, a circled X"
                        );
                    }
                    for ghost in &shown.shapes {
                        assert!(
                            is_piece_of(&after, ghost),
                            "{at}: a ghost not in the result"
                        );
                    }
                    for mark in &shown.marks {
                        let Geometry::Point(point) = &mark.geometry else {
                            continue;
                        };
                        if mark.role != FeedbackRole::Target {
                            continue;
                        }
                        if mark.label == "new 0" {
                            assert!(
                                close_to(after.vertices[0].position, point.position, EXACT),
                                "{at}: the vertex shown as the new start"
                            );
                        }
                        if mark.label.starts_with("→ ") {
                            let Some(vertex) = vertex_at(&after, point.position) else {
                                panic!("{at}");
                            };
                            assert_eq!(
                                format!("→ {}", &height_text(vertex.height)[2..]),
                                mark.label,
                                "{at}: the height a grade label promised"
                            );
                        }
                    }
                    if c.tool == "draw.vertex.edit" {
                        assert_eq!(driver.document().selection().ids(), vec![p], "{at}");
                    }
                    if c.tool == "draw.vertex.height" && !c.first.is_empty() {
                        // The value step's click takes the height nearest
                        // within the vertex reach, 1.5; Enter (nothing in
                        // reach) keeps vertex 1's 101 - worked from the
                        // corners, not the tool.
                        let mut expected = Some(zs[1]);
                        let mut nearest: f64 = 1.5;
                        for (corner, &z) in corners.iter().zip(&zs) {
                            let d = corner.distance_to(pt(x, y));
                            if d <= nearest {
                                nearest = d;
                                expected = Some(z);
                            }
                        }
                        assert_eq!(after.vertices[1].height, expected, "{at}");
                        let arrow = shown.caption.find("→ ");
                        if expected != Some(zs[1]) {
                            let Some(arrow) = arrow else {
                                panic!("{at}: {}", shown.caption);
                            };
                            assert_eq!(
                                &shown.caption[arrow + "→ ".len()..],
                                &height_text(expected)[2..],
                                "{at}: the caption said the height the click set"
                            );
                        } else {
                            assert!(arrow.is_none(), "{at}: {}", shown.caption);
                        }
                    }
                }
            }
            println!(
                "preview grid: {}{}{}: {taken} taken ({finished} finished), {refused} refused, \
                 {enter_shown} Enter's",
                c.tool,
                if c.first.is_empty() {
                    ""
                } else {
                    " after a pick"
                },
                if c.hot.is_empty() && c.hot_segment.is_none() {
                    ""
                } else {
                    " with grips chosen"
                }
            );
            // Every case reaches a click the tool takes or Enter's, except a
            // tool shown all refusals on purpose: Change Start on an open
            // polyline, Grade with no heights.
            let all_refused = (c.tool == "draw.vertex.start" && !c.closed)
                || (c.tool == "draw.vertex.grade" && !c.heights);
            if all_refused {
                assert_eq!(taken + enter_shown, 0, "{}", c.tool);
                assert!(refused > 0, "{}", c.tool);
            } else {
                assert!(taken + enter_shown > 0, "{}", c.tool);
            }
            refused_ever += refused;
            enter_ever += enter_shown;
        }
        assert!(refused_ever > 0);
        assert!(enter_ever > 0);
    }

    // ---- the review of 2026-09-30: what the preview and the click must agree on ------------

    #[test]
    fn with_a_chosen_vertex_a_click_is_previewed_where_it_takes_something_and_enter_elsewhere() {
        // The square (0,0) (10,0) (10,10) (0,10), closed, with vertex 2 chosen
        // before Delete Vertex. Over vertex 0 a click deletes vertex 0, so
        // that is what the preview marks: it marked vertex 2, and the click
        // deleted a vertex nobody had marked. Over nothing a click takes
        // nothing, and the preview is Enter's deletion of vertex 2, saying so.
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline_of(
            &mut driver,
            &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)],
            true,
            &[],
        );
        select_only(&mut driver, vec![p]);
        let handles = vec![vertex_handle(&driver, p, 2)];
        driver.start_with("draw.vertex.delete", handles);
        let ready =
            format!("Press Enter to delete vertex 2 of polyline {p}, or click another vertex");
        assert_eq!(driver.prompt(), ready);

        // (0.2, 0.1) is sqrt(0.05) = 0.22 from vertex 0, within the 0.75.
        let over_vertex0 = driver.preview(0.2, 0.1);
        assert_eq!(
            points_of(&over_vertex0, FeedbackRole::Removed),
            vec![pt(0.0, 0.0)]
        );
        assert_eq!(
            over_vertex0.caption,
            "delete vertex 0 · segments 3 and 0 become one"
        );
        // (5,5) is sqrt(50) = 7.07 from every vertex.
        let over_nothing = driver.preview(5.0, 5.0);
        assert_eq!(
            points_of(&over_nothing, FeedbackRole::Removed),
            vec![pt(10.0, 10.0)]
        );
        assert_eq!(
            over_nothing.caption,
            "Enter: delete vertex 2 · segments 1 and 2 become one"
        );

        // A click on nothing is refused and keeps the choice: it dropped it.
        assert_eq!(driver.click(5.0, 5.0).outcome, REJECTED);
        assert_eq!(driver.prompt(), ready);
        assert_eq!(driver.executed(), 0);
        // The click over vertex 0 deletes vertex 0, as the preview showed.
        assert_eq!(driver.click(0.2, 0.1).outcome, DONE);
        assert_eq!(
            positions_of(&driver, p),
            vec![pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)]
        );
    }

    #[test]
    fn change_start_with_a_chosen_vertex_previews_the_vertex_a_click_would_take() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline_of(
            &mut driver,
            &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)],
            true,
            &[],
        );
        select_only(&mut driver, vec![p]);
        let handles = vec![vertex_handle(&driver, p, 2)];
        driver.start_with("draw.vertex.start", handles);
        let over3 = driver.preview(0.1, 10.1);
        assert_eq!(points_of(&over3, FeedbackRole::Target), vec![pt(0.0, 10.0)]);
        assert_eq!(over3.caption, "vertex 3 becomes vertex 0");
        assert_eq!(
            driver.preview(5.0, 5.0).caption,
            "Enter: vertex 2 becomes vertex 0"
        );
        assert_eq!(driver.click(0.1, 10.1).outcome, DONE);
        assert_eq!(shape_of(&driver, p).vertices[0].position, pt(0.0, 10.0));
    }

    #[test]
    fn beside_a_chosen_vertex_the_place_enter_adds_at_is_marked() {
        // (0,0) (10,0) (10,10) with vertex 1 chosen: Enter adds at the middle
        // of the segment after it, (10,5), whichever segment the cursor is by
        // - it added on an edge the preview never marked - so that place is
        // marked while the click's own place follows the cursor.
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        select_only(&mut driver, vec![p]);
        let handles = vec![vertex_handle(&driver, p, 1)];
        driver.start_with("draw.vertex.insert", handles);
        for cursor in [pt(7.0, 1.0), pt(11.0, 4.0)] {
            let shown = driver.preview(cursor.x, cursor.y);
            assert_eq!(points_of(&shown, FeedbackRole::Enter), vec![pt(10.0, 5.0)]);
            assert_eq!(labels_of(&shown, FeedbackRole::Enter), ["Enter"]);
        }
        // By segment 0 the click's place is on segment 0, and Enter's stays.
        assert!(has_point(
            &points_of(&driver.preview(7.0, 1.0), FeedbackRole::Added),
            pt(7.0, 0.0),
            EXACT
        ));
        assert_eq!(driver.enter().outcome, DONE);
        assert_eq!(
            positions_of(&driver, p),
            vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 5.0), pt(10.0, 10.0)]
        );
    }

    #[test]
    fn back_on_the_vertex_just_picked_the_click_is_refused_and_the_preview_says_so() {
        // A second click on Straighten's first vertex is refused, so the
        // preview there is a refusal (R3): it showed the pick as taken, and a
        // preview that was not refused was a click that was. Straight after
        // the pick the view holds the refusal off
        // (ToolHost.AStraightenPicksKeepRingIsHeld...).
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(
            &mut driver,
            &[
                pt(0.0, 0.0),
                pt(1.0, 1.0),
                pt(2.0, -1.0),
                pt(3.0, 1.0),
                pt(4.0, 0.0),
            ],
        );
        driver.start("draw.vertex.straighten");
        assert_eq!(driver.pick(p, 0.0, 0.1).outcome, CONTINUE);
        let same = driver.preview(0.0, 0.1);
        assert!(same.refused);
        // The first pick's mark stays, which says what was taken; flagged as
        // no reason of its own, it is drawn as the refusal
        // (ToolFeedback::refused).
        assert_eq!(points_of(&same, FeedbackRole::Target), vec![pt(0.0, 0.0)]);
        assert_eq!(labels_of(&same, FeedbackRole::Target), ["keep"]);
        assert_eq!(
            same.caption,
            "vertex 0 is picked already; click the other one"
        );
        let again = driver.click(0.0, 0.1);
        assert_eq!(again.outcome, REJECTED);
        assert_eq!(
            again.message, same.caption,
            "the click says what the preview said"
        );
        // Off the polyline the first pick stays marked, and nothing is
        // refused.
        let away = driver.preview(50.0, 50.0);
        assert!(!away.refused);
        assert_eq!(labels_of(&away, FeedbackRole::Target), ["keep"]);
        assert_eq!(away.focus, Some(p));
    }

    #[test]
    fn fillet_and_chamfer_remember_what_was_typed_for_the_next_use() {
        // 2.5, then 1.5 and 0.75 - none of them the 1 the tools start with, so
        // a tool that forgot would offer and preview 1.
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let first = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        let second = add_polyline(&mut driver, &[pt(20.0, 0.0), pt(30.0, 0.0), pt(30.0, 10.0)]);
        driver.start("draw.vertex.fillet");
        assert_eq!(driver.pick(first, 10.0, 0.1).outcome, CONTINUE);
        assert_eq!(driver.r#type("2.5").outcome, DONE);
        // At a right angle the tangent points are the radius back along each
        // side: (27.5,0) and (30,2.5).
        let round = driver.preview(30.0, 0.1);
        let tangents = points_of(&round, FeedbackRole::Added);
        assert_eq!(tangents.len(), 2);
        assert!(has_point(&tangents, pt(27.5, 0.0), 1.0e-12));
        assert!(has_point(&tangents, pt(30.0, 2.5), 1.0e-12));
        assert_eq!(round.caption, "fillet vertex 1 · radius 2.500");
        assert_eq!(driver.pick(second, 30.0, 0.1).outcome, CONTINUE);
        assert!(driver.prompt().ends_with("<2.5>"), "{}", driver.prompt());
        assert_eq!(driver.r#type("1").outcome, DONE); // as it started

        let third = add_polyline(
            &mut driver,
            &[pt(0.0, 20.0), pt(10.0, 20.0), pt(10.0, 30.0)],
        );
        let fourth = add_polyline(
            &mut driver,
            &[pt(20.0, 20.0), pt(30.0, 20.0), pt(30.0, 30.0)],
        );
        driver.start("draw.vertex.chamfer");
        assert_eq!(driver.pick(third, 10.0, 20.1).outcome, CONTINUE);
        assert_eq!(driver.r#type("1.5").outcome, CONTINUE);
        assert_eq!(driver.r#type("0.75").outcome, DONE);
        // The bevel 1.5 back along the incoming side, 0.75 along the outgoing.
        let cut = points_of(&driver.preview(30.0, 20.1), FeedbackRole::Added);
        assert_eq!(cut.len(), 2);
        assert!(has_point(&cut, pt(28.5, 20.0), 1.0e-12));
        assert!(has_point(&cut, pt(30.0, 20.75), 1.0e-12));
        assert_eq!(driver.pick(fourth, 30.0, 20.1).outcome, CONTINUE);
        assert!(driver.prompt().ends_with("<1.5>"), "{}", driver.prompt());
        assert_eq!(driver.r#type("1").outcome, CONTINUE);
        assert!(driver.prompt().ends_with("<0.75>"), "{}", driver.prompt());
        assert_eq!(driver.r#type("1").outcome, DONE); // as it started
    }

    #[test]
    fn move_vertex_takes_a_relative_dz_as_a_change_of_its_height() {
        // @0,0,1 raises the vertex by 1, as the VERTEX MOVE verb reads it: it
        // set the height to 1, and a surveyed 101.5 was lost.
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline_of(
            &mut driver,
            &[pt(0.0, 0.0), pt(5.0, 0.0), pt(10.0, 0.0)],
            false,
            &[Some(100.0), Some(101.5), Some(100.0)],
        );
        driver.start("draw.vertex.move");
        assert_eq!(driver.pick(p, 5.0, 0.1).outcome, CONTINUE);
        assert_eq!(driver.r#type("@0,0,1").outcome, DONE);
        assert_eq!(shape_of(&driver, p).vertices[1].position, pt(5.0, 0.0));
        double_eq(height_of(&driver, p, 1), 102.5);
        assert_eq!(driver.pick(p, 5.0, 0.1).outcome, CONTINUE);
        assert_eq!(driver.r#type("@1,0,-0.5").outcome, DONE);
        assert_eq!(shape_of(&driver, p).vertices[1].position, pt(6.0, 0.0));
        double_eq(height_of(&driver, p, 1), 102.0);

        // A vertex with no height has none for a dz to change: refused, never
        // read as a height of dz, and the tool still asks where it goes.
        let flat = add_polyline(&mut driver, &[pt(0.0, 5.0), pt(5.0, 5.0), pt(10.0, 5.0)]);
        assert_eq!(driver.pick(flat, 5.0, 5.1).outcome, CONTINUE);
        let refused = driver.r#type("@0,0,1");
        assert_eq!(refused.outcome, REJECTED);
        assert!(refused.message.contains("no height"), "{}", refused.message);
        assert!(shape_of(&driver, flat).vertices[1].height.is_none());
        assert_eq!(driver.expects(), ToolInput::Point);
        // x,y,z is still the height itself.
        assert_eq!(driver.r#type("5,5,7").outcome, DONE);
        double_eq(height_of(&driver, flat, 1), 7.0);
    }

    #[test]
    fn a_three_d_polyline_reads_a_relative_dz_as_the_pline3_d_verb_does() {
        // One line, one polyline, in the window and through katana_cli and
        // katana_mcp: the PLINE3D verb takes the z of @dx,dy,dz as the
        // vertex's height (docs/drawing.md, "The command line": a z is a
        // vertex's height), and the tool reads it as its verb does. The tool
        // once climbed dz from the last vertex - 101.5 from 100 - while the
        // verb put the same vertex at 1.5. Move Vertex's "raise it by dz" is
        // VERTEX MOVE's reading, and stays
        // (move_vertex_takes_a_relative_dz_as_a_change_of_its_height).
        let mut driver = ToolDriver::new();
        driver.start("draw.polyline3d");
        assert_eq!(driver.r#type("0,0,100").outcome, CONTINUE);
        assert_eq!(driver.r#type("@10,0,1.5").outcome, CONTINUE);
        assert_eq!(driver.enter().outcome, DONE);
        let made = driver.document().last_created_entities();
        assert_eq!(made.len(), 1);
        let drawn = shape_of(&driver, made[0]);
        assert_eq!(drawn.vertices.len(), 2);
        assert_eq!(drawn.vertices[1].position, pt(10.0, 0.0));
        let Some(height) = drawn.vertices[1].height else {
            panic!("vertex 1 has no height");
        };
        double_eq(height, 1.5);

        let mut verbs = CommandInterpreter::new();
        if let Err(error) = verbs.run(driver.document_mut(), "PLINE3D 0,0,100 @10,0,1.5") {
            panic!("{}", error.describe());
        }
        let typed = driver.document().last_created_entities();
        assert_eq!(typed.len(), 1);
        let from_verb = shape_of(&driver, typed[0]);
        assert_eq!(from_verb.vertices.len(), 2);
        assert_eq!(from_verb.vertices[1].position, drawn.vertices[1].position);
        assert_eq!(
            from_verb.vertices[1].height, drawn.vertices[1].height,
            "the window's tool and the verb read one line one way"
        );
    }

    #[test]
    fn a_polyline_on_a_hidden_or_locked_layer_is_not_taken_though_it_is_selected() {
        // Selected, then its layer hidden in the drawing, hidden in this view,
        // or locked: the selection is not pruned, and the tools took the
        // polyline through it - marks on ground nobody could see, and an edit;
        // on a locked layer a preview the command then refused.
        for how in ["hidden", "hidden in the view", "locked"] {
            let mut driver = ToolDriver::new();
            driver.set_pick_tolerance(APERTURE);
            let mut layers = CommandInterpreter::new();
            layer_line(&mut layers, &mut driver, "LAYER NEW B");
            let entity = Entity {
                geometry: Geometry::Polyline(Polyline2::new(
                    vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)],
                    false,
                )),
                layer: String::from("B"),
                ..Entity::default()
            };
            let p = driver.add(create_entities(vec![entity]));
            select_only(&mut driver, vec![p]);
            if how == "hidden" {
                layer_line(&mut layers, &mut driver, "LAYER HIDE B");
            } else if how == "hidden in the view" {
                driver.hide_in_view("B");
            } else {
                layer_line(&mut layers, &mut driver, "LAYER LOCK B");
            }
            for tool in ["draw.vertex.delete", "draw.vertex.insert"] {
                driver.start(tool);
                // Vertex 1, and the middle of segment 0.
                assert!(driver.preview(10.0, 0.1).marks.is_empty(), "{how}, {tool}");
                assert!(driver.preview(5.0, 0.1).marks.is_empty(), "{how}, {tool}");
                assert_eq!(driver.click(10.0, 0.1).outcome, REJECTED, "{how}, {tool}");
                assert_eq!(driver.click(5.0, 0.1).outcome, REJECTED, "{how}, {tool}");
                let prompt = driver.prompt();
                if how == "locked" {
                    assert!(
                        prompt.starts_with(&format!(
                            "polyline {p} is on the locked layer B; unlock it to edit it. Click"
                        )),
                        "{tool}: {prompt}"
                    );
                } else {
                    assert!(
                        !prompt.contains(&format!("olyline {p}")),
                        "no prompt names what cannot be seen: {tool}: {prompt}"
                    );
                }
            }
            assert_eq!(driver.executed(), 0, "{how}");
            assert_eq!(shape_of(&driver, p).vertices.len(), 3, "{how}");
        }
    }

    #[test]
    fn set_height_takes_no_height_from_a_point_the_view_hides() {
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let mut layers = CommandInterpreter::new();
        layer_line(&mut layers, &mut driver, "LAYER NEW H");
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(5.0, 0.0), pt(10.0, 0.0)]);
        let mut spot = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: pt(30.0, 30.0),
            }),
            layer: String::from("H"),
            ..Entity::default()
        };
        set_heights(&mut spot.properties, &[Some(7.5)]);
        driver.add(create_entities(vec![spot]));
        driver.hide_in_view("H");
        driver.start("draw.vertex.height");
        assert_eq!(driver.pick(p, 5.0, 0.1).outcome, CONTINUE);
        assert_eq!(driver.click(30.1, 30.0).outcome, REJECTED);
        assert!(shape_of(&driver, p).vertices[1].height.is_none());
    }

    #[test]
    fn insert_takes_no_snap_off_the_arc_it_inserts_into() {
        // The arc from (0,0) to (10,0) of bulge 1: the half circle about
        // (5,0), radius 5, below the chord. The cursor (1.5,-3.6) is
        // sqrt(25.21) - 5 = 0.021 off it, far from its ends and middle, so the
        // only snap there is its Center, (5,0), which the arc offers while it
        // is hovered - 5 off the line, where no pick reaches: Insert showed
        // nothing along an arc but at its very middle.
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let mut shape = CurvePolyline2::from_points(&[pt(0.0, 0.0), pt(10.0, 0.0)], false);
        shape.vertices[0].bulge = 1.0;
        let arc = add_curve(&mut driver, &shape);
        select_only(&mut driver, vec![arc]);
        driver.start("draw.vertex.insert");
        let plain = SnapRequest {
            cursor: pt(1.5, -3.6),
            aperture: 0.5,
            ..SnapRequest::default()
        };
        let Some(centre) = snap(driver.document().model(), &plain, None) else {
            panic!("no snap at 1.5,-3.6");
        };
        assert_eq!(centre.mode, SnapMode::Center, "what took the cursor before");
        assert!(driver.snap_at(1.5, -3.6).is_none());
        let added = points_of(&driver.preview_snapped(1.5, -3.6), FeedbackRole::Added);
        assert_eq!(added.len(), 1);
        // On the arc, by the cursor: its projection from the centre.
        near(added[0].distance_to(pt(5.0, 0.0)), 5.0, 1.0e-12);
        near(
            added[0].distance_to(pt(1.5, -3.6)),
            25.21_f64.sqrt() - 5.0,
            1.0e-12,
        );
        // The arc's Midpoint (5,-5) is on it: taken, and the vertex goes there.
        let Some(middle) = driver.snap_at(5.0, -5.3) else {
            panic!("no snap at 5,-5.3");
        };
        assert_eq!(middle.mode, SnapMode::Midpoint);
        assert_eq!(driver.click_snapped(5.0, -5.3).outcome, DONE);
        assert!(has_point(
            &positions_of(&driver, arc),
            pt(5.0, -5.0),
            1.0e-12
        ));
    }

    #[test]
    fn insert_takes_no_snap_beside_the_line_nor_on_a_vertex_of_it() {
        // (0,0)-(30,0), a pick reach of 0.3 and the snap aperture of 0.5: the
        // view's 8 and 12 px in near enough their proportion. A line's end at
        // (10,0.4) is 0.4 off the polyline: snapped to, the point left the
        // pick's reach, and Insert showed nothing and refused the click. The
        // polyline's own vertex (0,0) is too close to insert at. Both are
        // passed over and the cursor stays; a crossing ON the line is still
        // taken.
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(0.3);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(30.0, 0.0)]);
        driver.add(create_line(
            pt(10.0, 0.4),
            pt(10.0, 5.0),
            EntityAttributes::default(),
        ));
        driver.add(create_line(
            pt(20.0, -5.0),
            pt(20.0, 7.0),
            EntityAttributes::default(),
        ));
        select_only(&mut driver, vec![p]);
        driver.start("draw.vertex.insert");
        assert!(driver.snap_at(10.3, 0.05).is_none());
        assert!(has_point(
            &points_of(&driver.preview_snapped(10.3, 0.05), FeedbackRole::Added),
            pt(10.3, 0.0),
            EXACT
        ));
        assert!(driver.snap_at(0.4, 0.05).is_none());
        assert!(has_point(
            &points_of(&driver.preview_snapped(0.4, 0.05), FeedbackRole::Added),
            pt(0.4, 0.0),
            EXACT
        ));
        let Some(crossing) = driver.snap_at(20.2, 0.1) else {
            panic!("no snap at 20.2,0.1");
        };
        assert_eq!(crossing.mode, SnapMode::Intersection);
        assert_eq!(driver.click_snapped(20.2, 0.1).outcome, DONE);
        assert!(has_point(&positions_of(&driver, p), pt(20.0, 0.0), 1.0e-12));
    }

    #[test]
    fn a_chosen_vertex_the_tool_cannot_use_is_explained_not_dropped() {
        // Fillet with an end chosen, Straighten with three, Delete with so
        // many that too few would be left: each says why before asking as
        // usual.
        {
            let mut driver = ToolDriver::new();
            driver.set_pick_tolerance(APERTURE);
            let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
            select_only(&mut driver, vec![p]);
            let handles = vec![vertex_handle(&driver, p, 0)];
            driver.start_with("draw.vertex.fillet", handles);
            assert_eq!(
                driver.prompt(),
                format!(
                    "vertex 0 is an end of the polyline; a corner is where two segments meet. \
                     Click the corner vertex of polyline {p} to round"
                )
            );
        }
        {
            let mut driver = ToolDriver::new();
            driver.set_pick_tolerance(APERTURE);
            let p = add_polyline(
                &mut driver,
                &[
                    pt(0.0, 0.0),
                    pt(1.0, 1.0),
                    pt(2.0, -1.0),
                    pt(3.0, 1.0),
                    pt(4.0, 0.0),
                ],
            );
            select_only(&mut driver, vec![p]);
            let handles = vec![
                vertex_handle(&driver, p, 0),
                vertex_handle(&driver, p, 2),
                vertex_handle(&driver, p, 4),
            ];
            driver.start_with("draw.vertex.straighten", handles);
            assert_eq!(
                driver.prompt(),
                format!(
                    "3 vertices are chosen, and this tool takes two on one polyline. Click the \
                     first vertex of polyline {p} to keep"
                )
            );
        }
        {
            let mut driver = ToolDriver::new();
            driver.set_pick_tolerance(APERTURE);
            let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
            select_only(&mut driver, vec![p]);
            let handles = vec![vertex_handle(&driver, p, 0), vertex_handle(&driver, p, 1)];
            driver.start_with("draw.vertex.delete", handles);
            let prompt = driver.prompt();
            assert!(
                prompt.starts_with(&format!("polyline {p}: deleting 2 vertices would leave 1")),
                "{prompt}"
            );
            assert!(
                prompt.ends_with(&format!(". Click the vertex of polyline {p} to delete")),
                "{prompt}"
            );
        }
    }

    #[test]
    fn the_tool_stays_on_the_polyline_last_edited_not_the_first() {
        // The first insert selects the polyline it edited (R5); an insert on
        // another then moves the selection to it: kept on the first, the
        // prompt named the first while the user worked on the second.
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let first = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0)]);
        let second = add_polyline(&mut driver, &[pt(0.0, 5.0), pt(10.0, 5.0)]);
        driver.start("draw.vertex.insert");
        assert_eq!(driver.click(4.0, 5.1).outcome, DONE);
        assert_eq!(driver.document().selection().ids(), vec![second]);
        assert_eq!(driver.click(6.0, 0.1).outcome, DONE);
        assert_eq!(driver.document().selection().ids(), vec![first]);
        assert_eq!(
            driver.prompt(),
            format!("Click on polyline {first} where the new vertex goes")
        );
    }

    // ---- the review of 2026-09-30, second round --------------------------------------------

    #[test]
    fn beside_a_chosen_vertex_a_pointer_past_its_segments_shows_what_enter_does_not_a_refusal() {
        // (0,0) (10,0) (10,10) with the corner, vertex 1, chosen. Beyond the
        // corner, (13,-3), both its segments are nearest at the corner itself;
        // past segment 0's far end, (-3,0.5), at vertex 0; and on the chosen
        // vertex, (10.1,0.1), is where the pointer rests after choosing it.
        // Each put the new vertex on a vertex: "too close to vertex N; zoom
        // in", in red, however far the pointer was and whatever the zoom -
        // the first thing Insert showed. A click there takes nothing, so the
        // preview is Enter's: the chosen vertex, Enter's place - the middle of
        // segment 1, (10,5), 5 from vertex 1 - and a caption said as Enter's;
        // the click is refused and the choice kept.
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        select_only(&mut driver, vec![p]);
        let handles = vec![vertex_handle(&driver, p, 1)];
        driver.start_with("draw.vertex.insert", handles);
        let ready = driver.prompt();
        for cursor in [pt(13.0, -3.0), pt(-3.0, 0.5), pt(10.1, 0.1)] {
            let shown = driver.preview(cursor.x, cursor.y);
            assert!(
                !shown.refused,
                "{},{}: {}",
                cursor.x, cursor.y, shown.caption
            );
            assert_eq!(
                shown.caption, "Enter: new vertex between 1 and 2 · 5.000 from 1",
                "{},{}",
                cursor.x, cursor.y
            );
            assert_eq!(points_of(&shown, FeedbackRole::Enter), vec![pt(10.0, 5.0)]);
            assert_eq!(
                points_of(&shown, FeedbackRole::Target),
                vec![pt(10.0, 0.0)],
                "the chosen vertex, and no segment a click would split"
            );
            assert!(points_of(&shown, FeedbackRole::Added).is_empty());
            assert!(refused_points_of(&shown).is_empty());
            assert_eq!(driver.click(cursor.x, cursor.y).outcome, REJECTED);
            assert_eq!(driver.prompt(), ready, "the choice is kept");
        }
        assert_eq!(driver.executed(), 0);
        // Along segment 0 near its far end the pointer IS too near vertex 0:
        // that vertex is struck, and the chosen one is still marked as chosen.
        let near_vertex = driver.preview(0.2, 0.1);
        assert!(near_vertex.refused);
        assert!(
            near_vertex.caption.starts_with("too close to vertex 0"),
            "{}",
            near_vertex.caption
        );
        assert_eq!(refused_points_of(&near_vertex), vec![pt(0.0, 0.0)]);
        assert!(has_point(
            &points_of(&near_vertex, FeedbackRole::Target),
            pt(10.0, 0.0),
            EXACT
        ));
        // And Enter adds at the place it showed.
        assert_eq!(driver.enter().outcome, DONE);
        assert_eq!(
            positions_of(&driver, p),
            vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 5.0), pt(10.0, 10.0)]
        );
    }

    #[test]
    fn with_chosen_vertices_the_click_preview_still_marks_what_enter_takes() {
        // The closed square with vertex 2, (10,10), chosen before Delete
        // Vertex and the pointer over vertex 0: the preview is the click's -
        // vertex 0 goes - and vertex 2, which the prompt says Enter deletes,
        // is marked as Enter's with its number. It was a plain square, and the
        // prompt's "vertex 2" was nowhere on screen.
        let square = [pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)];
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline_of(&mut driver, &square, true, &[]);
        select_only(&mut driver, vec![p]);
        let handles = vec![vertex_handle(&driver, p, 2)];
        driver.start_with("draw.vertex.delete", handles);
        let shown = driver.preview(0.2, 0.1);
        assert_eq!(points_of(&shown, FeedbackRole::Removed), vec![pt(0.0, 0.0)]);
        assert_eq!(points_of(&shown, FeedbackRole::Enter), vec![pt(10.0, 10.0)]);
        assert_eq!(labels_of(&shown, FeedbackRole::Enter), ["2 · Enter"]);
        // Over the chosen vertex the click and Enter do the same: one mark.
        let over = driver.preview(10.1, 10.1);
        assert_eq!(
            points_of(&over, FeedbackRole::Removed),
            vec![pt(10.0, 10.0)]
        );
        assert!(points_of(&over, FeedbackRole::Enter).is_empty());

        // Change Start: the click's "new 0" on vertex 3, and vertex 2 as
        // Enter's.
        let mut start = ToolDriver::new();
        start.set_pick_tolerance(APERTURE);
        let q = add_polyline_of(&mut start, &square, true, &[]);
        select_only(&mut start, vec![q]);
        let handles = vec![vertex_handle(&start, q, 2)];
        start.start_with("draw.vertex.start", handles);
        let other = start.preview(0.1, 10.1);
        assert_eq!(points_of(&other, FeedbackRole::Target), vec![pt(0.0, 10.0)]);
        assert_eq!(points_of(&other, FeedbackRole::Enter), vec![pt(10.0, 10.0)]);

        // Straighten with two chosen: both are Enter's beside the click's
        // "keep".
        let mut straight = ToolDriver::new();
        straight.set_pick_tolerance(APERTURE);
        let r = add_polyline(
            &mut straight,
            &[
                pt(0.0, 0.0),
                pt(1.0, 1.0),
                pt(2.0, -1.0),
                pt(3.0, 1.0),
                pt(4.0, 0.0),
            ],
        );
        select_only(&mut straight, vec![r]);
        let handles = vec![
            vertex_handle(&straight, r, 0),
            vertex_handle(&straight, r, 2),
        ];
        straight.start_with("draw.vertex.straighten", handles);
        let keep = straight.preview(3.0, 1.1);
        assert_eq!(points_of(&keep, FeedbackRole::Target), vec![pt(3.0, 1.0)]);
        assert_eq!(
            labels_of(&keep, FeedbackRole::Enter),
            ["0 · Enter", "2 · Enter"]
        );
    }

    #[test]
    fn enter_and_a_typed_place_are_taken_on_a_segment_shorter_than_two_apertures() {
        // Segment 0 of (0,0) (1,0) (10,0) is 1 long: two pick apertures of
        // 0.5, 16 px at the view's 8. Its middle is 0.5 from each end, "too
        // close" by the rule for a POINTER - the view could not tell a click
        // there from one on the vertex. Enter beside vertex 0, which the prompt
        // offers and the preview marks, a typed segment number and a typed
        // point say exactly where, and all three were refused. A pointed click
        // there still is.
        let short_first = [pt(0.0, 0.0), pt(1.0, 0.0), pt(10.0, 0.0)];
        {
            let mut driver = ToolDriver::new();
            driver.set_pick_tolerance(APERTURE);
            let p = add_polyline(&mut driver, &short_first);
            select_only(&mut driver, vec![p]);
            let handles = vec![vertex_handle(&driver, p, 0)];
            driver.start_with("draw.vertex.insert", handles);
            assert!(
                driver
                    .prompt()
                    .starts_with("Press Enter for a vertex at the marked middle of segment 0"),
                "{}",
                driver.prompt()
            );
            // Past the polyline's start the click takes nothing: Enter's place
            // is shown, and not as refused.
            let shown = driver.preview(-3.0, 0.2);
            assert_eq!(points_of(&shown, FeedbackRole::Enter), vec![pt(0.5, 0.0)]);
            assert!(refused_points_of(&shown).is_empty());
            assert_eq!(
                shown.caption,
                "Enter: new vertex between 0 and 1 · 0.500 from 0"
            );
            let step = driver.enter();
            assert_eq!(step.outcome, DONE, "{}", step.message);
            assert_eq!(
                positions_of(&driver, p),
                vec![pt(0.0, 0.0), pt(0.5, 0.0), pt(1.0, 0.0), pt(10.0, 0.0)]
            );
        }
        {
            let mut driver = ToolDriver::new();
            driver.set_pick_tolerance(APERTURE);
            let p = add_polyline(&mut driver, &short_first);
            select_only(&mut driver, vec![p]);
            driver.start("draw.vertex.insert");
            let step = driver.r#type("0");
            assert_eq!(step.outcome, DONE, "{}", step.message);
            assert_eq!(positions_of(&driver, p)[1], pt(0.5, 0.0));
        }
        {
            let mut driver = ToolDriver::new();
            driver.set_pick_tolerance(APERTURE);
            let p = add_polyline(&mut driver, &short_first);
            select_only(&mut driver, vec![p]);
            driver.start("draw.vertex.insert");
            // 0.3 from vertex 0: pointed at, too close; typed, exactly there.
            let pointed = driver.click(0.3, 0.05);
            assert_eq!(pointed.outcome, REJECTED);
            assert!(
                pointed.message.starts_with("too close to vertex 0"),
                "{}",
                pointed.message
            );
            let typed = driver.r#type("0.3,0");
            assert_eq!(typed.outcome, DONE, "{}", typed.message);
            assert_eq!(positions_of(&driver, p)[1], pt(0.3, 0.0));
            // On a vertex exactly, geometry itself refuses it.
            assert_eq!(driver.r#type("1,0").outcome, REJECTED);
        }
    }

    #[test]
    fn a_pick_edited_since_is_shown_refused_and_the_next_input_drops_it() {
        // Vertex 1 of (0,0) (10,0) (10,10) chosen for Insert, then another
        // edit - the Vertices panel, an agent, another view - moves vertex 2 to
        // (20,10). The preview was drawn from the copy the choice kept: a green
        // segment and a new vertex on the edge x = 10 that is no longer there,
        // and the click was then refused as stale. Now the preview says so,
        // with no marks, and the click is refused with the same words and
        // drops the choice; the next click picks the polyline as it is.
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        select_only(&mut driver, vec![p]);
        let handles = vec![vertex_handle(&driver, p, 1)];
        driver.start_with("draw.vertex.insert", handles);
        ok(driver.document_mut().execute(edit_polyline(
            p,
            "VERTEX_MOVE",
            |shape: &CurvePolyline2| move_vertex(shape, 2, pt(20.0, 10.0)),
        )));
        let stale = format!("polyline {p} changed since it was picked; pick it again");
        let shown = driver.preview(10.2, 5.0);
        assert!(shown.refused);
        assert_eq!(shown.caption, stale);
        assert!(shown.marks.is_empty(), "nothing drawn where the edge was");
        let click = driver.click(10.2, 5.0);
        assert_eq!(click.outcome, REJECTED);
        assert_eq!(click.message, stale);
        assert_eq!(
            driver.prompt(),
            format!("Click on polyline {p} where the new vertex goes"),
            "the stale choice is dropped"
        );
        // Segment 1 is now (10,0)-(20,10), through (15,5).
        assert_eq!(driver.click(15.0, 5.0).outcome, DONE);
        assert_eq!(
            positions_of(&driver, p),
            vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(15.0, 5.0), pt(20.0, 10.0)]
        );

        // Move Vertex's first pick, then its vertex moved by another edit: the
        // preview at the point step is the refusal, not a move from where it
        // was.
        let mut moving = ToolDriver::new();
        moving.set_pick_tolerance(APERTURE);
        let q = add_polyline(&mut moving, &[pt(0.0, 0.0), pt(5.0, 0.0), pt(10.0, 0.0)]);
        moving.start("draw.vertex.move");
        assert_eq!(moving.pick(q, 5.0, 0.1).outcome, CONTINUE);
        ok(moving.document_mut().execute(edit_polyline(
            q,
            "VERTEX_MOVE",
            |shape: &CurvePolyline2| move_vertex(shape, 1, pt(5.0, 4.0)),
        )));
        assert!(moving.preview(5.0, 8.0).refused);
        assert_eq!(moving.r#type("5,8").outcome, REJECTED);
        assert_eq!(
            positions_of(&moving, q)[1],
            pt(5.0, 4.0),
            "the other edit stands"
        );
        assert_eq!(moving.expects(), ToolInput::Entity, "back to the pick");
    }

    #[test]
    fn a_chosen_vertex_on_a_layer_hidden_or_locked_since_is_not_edited_by_enter() {
        // Vertex 2 chosen for Delete Vertex, then its layer hidden - in the
        // drawing or in this view - or locked. Enter deleted the vertex of a
        // polyline the view no longer drew, and on a locked layer the command
        // was refused after the preview had promised it.
        for how in ["hidden", "hidden in the view", "locked"] {
            let mut driver = ToolDriver::new();
            driver.set_pick_tolerance(APERTURE);
            let mut layers = CommandInterpreter::new();
            let p = on_layer_b(
                &mut driver,
                &mut layers,
                &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)],
                true,
            );
            select_only(&mut driver, vec![p]);
            let handles = vec![vertex_handle(&driver, p, 2)];
            driver.start_with("draw.vertex.delete", handles);
            take_away_layer_b(&mut driver, &mut layers, how);
            let shown = driver.preview(5.0, 5.0);
            assert!(shown.refused, "{how}");
            assert!(shown.marks.is_empty(), "{how}");
            let expected = if how == "locked" {
                format!("polyline {p} is on the locked layer B; unlock it to edit it")
            } else {
                format!("polyline {p} is on a layer hidden since it was picked; show it to edit it")
            };
            assert_eq!(shown.caption, expected, "{how}");
            let step = driver.enter();
            assert_eq!(step.outcome, REJECTED, "{how}");
            assert_eq!(step.message, shown.caption, "{how}");
            assert_eq!(driver.executed(), 0, "{how}");
            assert_eq!(shape_of(&driver, p).vertices.len(), 4, "{how}");
        }
    }

    #[test]
    fn the_selection_tools_take_no_selected_polyline_on_a_hidden_or_locked_layer() {
        // Selected, then its layer hidden or locked: Close or Open opened it,
        // and Weed weeded it, where nobody could see it (R1 said it could not
        // happen). Now the selection answers nothing, and Enter says why.
        let square = [pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)];
        for how in ["hidden", "hidden in the view", "locked"] {
            for tool in ["draw.vertex.close", "draw.vertex.weed"] {
                let mut driver = ToolDriver::new();
                let mut layers = CommandInterpreter::new();
                let p = on_layer_b(&mut driver, &mut layers, &square, true);
                select_only(&mut driver, vec![p]);
                take_away_layer_b(&mut driver, &mut layers, how);
                driver.start(tool);
                let prompt = driver.prompt();
                assert!(
                    !prompt.contains("Press Enter to apply"),
                    "{how}, {tool}: {prompt}"
                );
                assert_eq!(driver.expects(), ToolInput::Selection, "{how}, {tool}");
                if how == "locked" {
                    assert!(
                        prompt.starts_with(&format!(
                            "polyline {p} is on the locked layer B; unlock it to edit it. Select"
                        )),
                        "{tool}: {prompt}"
                    );
                }
                let step = driver.enter();
                assert_eq!(step.outcome, REJECTED, "{how}, {tool}");
                assert_eq!(
                    step.message,
                    "the selected polylines are on hidden or locked layers; select others, then \
                     press Enter"
                );
                assert_eq!(driver.executed(), 0, "{how}, {tool}");
                assert!(shape_of(&driver, p).closed, "{how}, {tool}");
            }
        }
        // Beside one the view shows, only that one is taken.
        let mut driver = ToolDriver::new();
        let mut layers = CommandInterpreter::new();
        let hidden = on_layer_b(
            &mut driver,
            &mut layers,
            &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)],
            false,
        );
        let shown = add_polyline(
            &mut driver,
            &[pt(0.0, 20.0), pt(10.0, 20.0), pt(10.0, 30.0)],
        );
        select_only(&mut driver, vec![hidden, shown]);
        layer_line(&mut layers, &mut driver, "LAYER HIDE B");
        driver.start("draw.vertex.close");
        assert_eq!(
            driver.prompt(),
            "Press Enter to apply to the 1 selected polyline"
        );
        assert_eq!(driver.enter().outcome, DONE);
        assert!(!shape_of(&driver, hidden).closed);
        assert!(shape_of(&driver, shown).closed);
    }

    #[test]
    fn edit_vertices_takes_no_selected_polyline_on_a_hidden_or_locked_layer() {
        // Every other vertex tool passes a selected polyline through the
        // view's rule; Edit Vertices answered with its own look through the
        // selection and said a hidden or locked one was "in the Vertices
        // panel".
        for how in ["hidden", "hidden in the view", "locked"] {
            let mut driver = ToolDriver::new();
            driver.set_pick_tolerance(APERTURE);
            let mut layers = CommandInterpreter::new();
            let p = on_layer_b(
                &mut driver,
                &mut layers,
                &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)],
                false,
            );
            select_only(&mut driver, vec![p]);
            take_away_layer_b(&mut driver, &mut layers, how);
            driver.start("draw.vertex.edit");
            let expected = if how == "locked" {
                format!(
                    "polyline {p} is on the locked layer B; unlock it to edit it. Click the \
                     polyline whose vertices to edit"
                )
            } else {
                String::from("Click the polyline whose vertices to edit")
            };
            assert_eq!(driver.prompt(), expected, "{how}");
            assert_eq!(driver.expects(), ToolInput::Entity, "{how}");
        }
    }

    #[test]
    fn edit_vertices_says_enter_shows_a_grips_polyline_the_panel_does_not_show_yet() {
        // Two polylines selected: the panel shows the first (vertex_panel.cpp).
        // A grip of the second chosen before Edit Vertices is shown by Enter,
        // which selects it; the prompt said it was in the panel already.
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let first = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0)]);
        let second = add_polyline(&mut driver, &[pt(0.0, 5.0), pt(10.0, 5.0)]);
        select_only(&mut driver, vec![first, second]);
        let handles = vec![vertex_handle(&driver, second, 0)];
        driver.start_with("draw.vertex.edit", handles);
        assert_eq!(
            driver.prompt(),
            format!(
                "Press Enter to show polyline {second} in the Vertices panel, or click another \
                 polyline"
            )
        );
        assert_eq!(driver.enter().outcome, DONE);
        assert_eq!(driver.document().selection().ids(), vec![second]);
        // Its own polyline's grip: in the panel already.
        select_only(&mut driver, vec![first, second]);
        let handles = vec![vertex_handle(&driver, first, 1)];
        driver.start_with("draw.vertex.edit", handles);
        assert_eq!(
            driver.prompt(),
            format!(
                "Polyline {first} is in the Vertices panel; click another polyline, or press \
                 Enter"
            )
        );
    }

    #[test]
    fn segment_to_arc_refuses_a_segment_with_no_length_at_the_pick() {
        // (0,0) (10,0) (20,10) (30,0) with vertex 1 moved onto vertex 0:
        // segment 0 has no length, so no arc passes through any point from it.
        // It was taken, and then every point was refused and only Esc got
        // out.
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(
            &mut driver,
            &[pt(0.0, 0.0), pt(10.0, 0.0), pt(20.0, 10.0), pt(30.0, 0.0)],
        );
        ok(driver.document_mut().execute(edit_polyline(
            p,
            "VERTEX_MOVE",
            |shape: &CurvePolyline2| move_vertex(shape, 1, pt(0.0, 0.0)),
        )));
        select_only(&mut driver, vec![p]);
        driver.start("draw.vertex.arc");
        let why = "segment 0 has no length; an arc needs its two ends apart";
        // (-0.3,-0.1) is as near segment 0 as segment 1, and the first is
        // taken (geometry's nearest segment).
        let shown = driver.preview(-0.3, -0.1);
        assert!(shown.refused);
        assert_eq!(shown.caption, why);
        let typed = driver.r#type("0");
        assert_eq!(typed.outcome, REJECTED);
        assert_eq!(typed.message, why);
        assert_eq!(driver.expects(), ToolInput::Entity, "still the pick");
        // Segment 1 is fine.
        assert_eq!(driver.r#type("1").outcome, CONTINUE);
    }

    #[test]
    fn a_typed_pick_number_is_read_as_the_verbs_read_an_index() {
        // core's parse_integer, as the drawing verbs' index_of reads one: "1.5"
        // and "-1" are no index, and "+1" is 1.
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(20.0, 0.0)]);
        select_only(&mut driver, vec![p]);
        driver.start("draw.vertex.delete");
        assert_eq!(driver.r#type("1.5").outcome, REJECTED);
        assert_eq!(driver.r#type("-1").outcome, REJECTED);
        assert_eq!(driver.r#type("+1").outcome, DONE);
        assert_eq!(positions_of(&driver, p), vec![pt(0.0, 0.0), pt(20.0, 0.0)]);
    }

    #[test]
    fn a_dz_typed_where_the_step_takes_no_height_is_dropped_not_refused() {
        // Segment to Arc's point has no height to take: "@0,2,0" there is the
        // point 2 above where segment 0 was picked - its middle (5,0), picked
        // by number - with the dz dropped, as the z of x,y,z is. Answered as a
        // step that takes heights for being a point step, it was refused for
        // want of a height to change. Through (5,2) over (0,0)-(10,0) the arc
        // has its centre (5,k) with 25 + k^2 = (2 - k)^2: k = -5.25, radius
        // 7.25.
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        select_only(&mut driver, vec![p]);
        driver.start("draw.vertex.arc");
        assert_eq!(driver.r#type("0").outcome, CONTINUE);
        let step = driver.r#type("@0,2,0");
        assert_eq!(step.outcome, DONE, "{}", step.message);
        let after = shape_of(&driver, p);
        let Some(arc) = arc_from_bulge(
            after.vertices[0].position,
            after.vertices[1].position,
            after.vertices[0].bulge,
        ) else {
            panic!("segment 0 is not an arc");
        };
        assert!(close_to(arc.center, pt(5.0, -5.25), 1.0e-9));
        near(arc.radius, 7.25, 1.0e-9);
    }

    // ---- the review of 2026-09-30, third round ---------------------------------------------

    #[test]
    fn a_typed_point_out_of_reach_says_so_rather_than_repeating_the_prompt() {
        // A typed x,y at a pick is taken as a click there is (docs/drawing.md,
        // R1): within the vertex reach, here 1.5 x the aperture of 0.5 = 0.75,
        // the view's 12 px. (10.9,0) is 0.9 from vertex 1, (10.6,0) is 0.6.
        // The miss was answered with the prompt alone, though the typist
        // cannot see the reach; it says so now, and how to name the vertex
        // instead.
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(20.0, 0.0)]);
        select_only(&mut driver, vec![p]);
        driver.start("draw.vertex.delete");
        let prompt = driver.prompt();
        let missed = driver.r#type("10.9,0");
        assert_eq!(missed.outcome, REJECTED);
        assert!(
            missed
                .message
                .starts_with("10.9,0 is not within 0.750 of a vertex"),
            "{}",
            missed.message
        );
        assert!(missed.message.contains("12 px"), "{}", missed.message);
        assert!(
            missed.message.contains("vertex's number"),
            "{}",
            missed.message
        );
        assert!(missed.message.ends_with(&prompt), "{}", missed.message);
        assert_eq!(positions_of(&driver, p).len(), 3, "nothing deleted");
        // A click there is refused with the prompt, as a click on nothing is.
        assert_eq!(driver.click(10.9, 0.0).message, prompt);
        assert_eq!(driver.r#type("10.6,0").outcome, DONE);
        assert_eq!(positions_of(&driver, p), vec![pt(0.0, 0.0), pt(20.0, 0.0)]);
    }

    #[test]
    fn straighten_and_grade_say_the_side_enter_takes_and_other_side_flips_it() {
        // The hexagon (0,0) (10,0) (20,0) (20,10) (10,10) (0,10) with vertices
        // 3 and 1 chosen, in that order. From 3 forward to 1 is 4, 5 and 0;
        // the other way, 1 to 3, only 2: the shorter side is walked from 1 to
        // 3, and O walks 3 to 1. The prompt named the picks in the order chosen
        // - "from vertex 3 to 1" - before O and after it, while the caption
        // and the marks said otherwise.
        for tool in ["draw.vertex.straighten", "draw.vertex.grade"] {
            let mut driver = ToolDriver::new();
            driver.set_pick_tolerance(APERTURE);
            let h = add_polyline_of(
                &mut driver,
                &[
                    pt(0.0, 0.0),
                    pt(10.0, 0.0),
                    pt(20.0, 0.0),
                    pt(20.0, 10.0),
                    pt(10.0, 10.0),
                    pt(0.0, 10.0),
                ],
                true,
                &[
                    Some(0.0),
                    Some(1.0),
                    Some(2.0),
                    Some(3.0),
                    Some(4.0),
                    Some(5.0),
                ],
            );
            select_only(&mut driver, vec![h]);
            let handles = vec![vertex_handle(&driver, h, 3), vertex_handle(&driver, h, 1)];
            driver.start_with(tool, handles);
            let verb = if tool == "draw.vertex.grade" {
                "grade"
            } else {
                "straighten"
            };
            assert_eq!(
                driver.prompt(),
                format!(
                    "Press Enter to {verb} polyline {h} from vertex 1 to 3, or click another \
                     vertex [Other side]"
                ),
                "{tool}"
            );
            assert_eq!(driver.r#type("O").outcome, CONTINUE);
            assert_eq!(
                driver.prompt(),
                format!(
                    "Press Enter to {verb} polyline {h} from vertex 3 to 1, or click another \
                     vertex [Other side]"
                ),
                "{tool}"
            );
            // And Enter walks as the prompt says: Straighten takes out 4, 5
            // and 0, leaving (10,0) (20,0) (20,10).
            if verb == "straighten" {
                assert_eq!(driver.enter().outcome, DONE);
                assert_eq!(
                    positions_of(&driver, h),
                    vec![pt(10.0, 0.0), pt(20.0, 0.0), pt(20.0, 10.0)]
                );
            }
        }
    }

    #[test]
    fn edit_vertices_marks_no_vertex_for_enter_where_the_click_takes_the_same_polyline() {
        // Edit Vertices with a polyline selected is answered by the selection;
        // hovering that polyline previews the click - the whole polyline - and
        // a mark at its vertex 0, "polyline 2 · Enter", read as a chosen vertex
        // the tool does not act on. Nothing is marked for Enter now; the
        // prompt names the polyline.
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        select_only(&mut driver, vec![p]);
        driver.start("draw.vertex.edit");
        let shown = driver.preview(10.1, 5.0);
        assert_eq!(pieces_of(&shown, FeedbackRole::Target).len(), 1);
        assert!(points_of(&shown, FeedbackRole::Enter).is_empty());
        assert_eq!(shown.caption, format!("polyline {p} · 3 vertices, open"));
    }

    #[test]
    fn set_heights_value_step_previews_the_height_a_click_takes_and_enters_elsewhere() {
        // Heights 0, none and 10 at x = 0, 5 and 10, and a point at (30,30)
        // with a height of 7.5. Vertex 1 picked, the height is asked for with
        // the default 5, halfway by length. Over the point a click takes 7.5,
        // and the preview says so; over nothing a click is refused, and the
        // preview is Enter's, the default. It was "vertex 1 · no height"
        // wherever the cursor was: neither the height a click took nor the
        // refusal of a click that was refused (R3).
        let mut driver = ToolDriver::new();
        driver.set_pick_tolerance(APERTURE);
        let p = add_polyline_of(
            &mut driver,
            &[pt(0.0, 0.0), pt(5.0, 0.0), pt(10.0, 0.0)],
            false,
            &[Some(0.0), None, Some(10.0)],
        );
        let mut spot = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: pt(30.0, 30.0),
            }),
            ..Entity::default()
        };
        set_heights(&mut spot.properties, &[Some(7.5)]);
        driver.add(create_entities(vec![spot]));
        driver.start("draw.vertex.height");
        assert_eq!(driver.pick(p, 5.0, 0.1).outcome, CONTINUE);
        let over = driver.preview(30.1, 30.0);
        assert!(!over.refused);
        assert_eq!(over.caption, "vertex 1 · no height → 7.500");
        let nothing = driver.preview(20.0, 20.0);
        assert!(!nothing.refused);
        assert_eq!(nothing.caption, "Enter: vertex 1 · no height → 5.000");
        let refused = driver.click(20.0, 20.0);
        assert_eq!(refused.outcome, REJECTED);
        assert!(
            refused
                .message
                .starts_with("nothing with a height is within reach there"),
            "{}",
            refused.message
        );
        assert_eq!(driver.click(30.1, 30.0).outcome, DONE);
        double_eq(height_of(&driver, p, 1), 7.5);
    }
}
