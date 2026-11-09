// The Draw > Lines tools (tools/draw_lines.rs; tests/cad/tools/test_draw_lines.cpp):
// Point, Line, Polyline, Rectangle and Polygon, each driven through ToolDriver
// as a user would drive it - clicks, typed points, options, Undo, Enter - and
// the entities it leaves in the document compared with geometry worked out by
// hand in the comments. Inputs are small integers and halves wherever the
// geometry allows, so most results are exact in binary; where a sine or a
// square root is involved the comparison is to 1e-12, a few ulps at these
// magnitudes.

use katana_cad::interactive_tool::{
    ToolInput, ToolStepOutcome as Outcome, route_typed_input_with_drafting, tool_catalog,
};
use katana_cad::{Document, DraftingSettings};
use katana_commands::create_layer;
use katana_entity::{Geometry, Layer, PointGeometry};
use katana_geometry::{Point2, Polyline2, Segment2};

use crate::tool_driver::ToolDriver;
use crate::{entities_of, near, point, points, polyline, refused, segment, vertices_near};

const ROOT2: f64 = std::f64::consts::SQRT_2;

fn root3() -> f64 {
    3.0_f64.sqrt()
}

// The document's points, lines and polylines, in creation (id) order.
fn points_of(document: &Document) -> Vec<Point2> {
    entities_of(document)
        .into_iter()
        .filter_map(|entity| match entity.geometry {
            Geometry::Point(point) => Some(point.position),
            _ => None,
        })
        .collect()
}

fn lines_of(document: &Document) -> Vec<Segment2> {
    entities_of(document)
        .into_iter()
        .filter_map(|entity| match entity.geometry {
            Geometry::Line(line) => Some(line),
            _ => None,
        })
        .collect()
}

fn polylines_of(document: &Document) -> Vec<Polyline2> {
    entities_of(document)
        .into_iter()
        .filter_map(|entity| match entity.geometry {
            Geometry::Polyline(polyline) => Some(polyline),
            _ => None,
        })
        .collect()
}

// The document's only polyline; fails the test when there is not exactly one.
#[track_caller]
fn only_polyline(document: &Document) -> Polyline2 {
    assert_eq!(entities_of(document).len(), 1);
    let mut polylines = polylines_of(document);
    assert_eq!(polylines.len(), 1);
    polylines.remove(0)
}

fn kerb_layer(driver: &mut ToolDriver, name: &str) {
    crate::ok(driver.document_mut().execute(create_layer(Layer {
        name: String::from(name),
        ..Layer::default()
    })));
    crate::ok(driver.document_mut().set_current_layer(name));
}

// Line and Polyline answer text that is neither a point nor an option the
// same way, and both offer Close from the third point on, so one check serves
// both.
fn expect_refusals_to_name_only_what_the_prompt_takes(id: &str) {
    let mut driver = ToolDriver::new();
    driver.start(id);
    // Nothing to measure from yet, so no relative input and no Close.
    let why = driver.r#type("X").message;
    assert!(why.contains("x,y"), "{id}: {why}");
    assert!(!why.contains('@'), "{id}: {why}");
    assert!(!why.contains("close"), "{id}: {why}");
    driver.click(0.0, 0.0);
    driver.click(4.0, 0.0);
    let why = driver.r#type("X").message;
    assert!(why.contains("@dx,dy"), "{id}: {why}");
    assert!(!why.contains("close"), "{id}: {why}");
    driver.click(4.0, 3.0);
    let why = driver.r#type("X").message;
    assert!(why.contains("C to close"), "{id}: {why}");
}

// ---- the catalogue -----------------------------------------------------------------

mod draw_line_tools {
    use super::*;

    #[test]
    fn are_listed_under_draw_lines_in_menu_order() {
        let mut ids = Vec::new();
        for tool in tool_catalog().all() {
            if tool.category == "Draw" && tool.group == "Lines" {
                ids.push(tool.id.clone());
                assert!(!tool.tip.is_empty(), "{}", tool.id);
                assert!(tool.shortcut.is_empty(), "{}", tool.id);
            }
        }
        // The drawing system added the 3D polyline, construction line and
        // ray, double line, sketch and revision cloud (docs/drawing.md).
        assert_eq!(
            ids,
            [
                "draw.point",
                "draw.line",
                "draw.polyline",
                "draw.polyline3d",
                "draw.rectangle",
                "draw.polygon",
                "draw.xline",
                "draw.ray",
                "draw.dline",
                "draw.sketch",
                "draw.revcloud",
            ]
        );
    }

    #[test]
    fn start_from_their_auto_cad_verbs_and_the_interpreters_spellings() {
        // AutoCAD's command names and aliases, and the interpreter's own RECT,
        // RECTANGLE and POLYLINE (command_interpreter), which name the same
        // tools there.
        let verbs = [
            ("POINT", "draw.point"),
            ("PO", "draw.point"),
            ("LINE", "draw.line"),
            ("L", "draw.line"),
            ("PLINE", "draw.polyline"),
            ("PL", "draw.polyline"),
            ("POLYLINE", "draw.polyline"),
            ("RECTANG", "draw.rectangle"),
            ("REC", "draw.rectangle"),
            ("RECT", "draw.rectangle"),
            ("RECTANGLE", "draw.rectangle"),
            ("POLYGON", "draw.polygon"),
            ("POL", "draw.polygon"),
        ];
        for (verb, id) in verbs {
            let Some(tool) = tool_catalog().find_by_alias(verb) else {
                panic!("{verb}");
            };
            assert_eq!(tool.id, id, "{verb}");
        }
    }

    #[test]
    fn every_one_draws_on_the_current_layer() {
        let mut driver = ToolDriver::new();
        kerb_layer(&mut driver, "Kerb");

        driver.start("draw.point");
        driver.click(1.0, 1.0);
        driver.start("draw.line");
        driver.click(0.0, 0.0);
        driver.click(1.0, 0.0);
        driver.click(1.0, 1.0);
        driver.enter();
        driver.start("draw.polyline");
        driver.click(0.0, 0.0);
        driver.click(2.0, 0.0);
        driver.enter();
        driver.start("draw.rectangle");
        driver.click(0.0, 0.0);
        driver.click(2.0, 1.0);
        driver.start("draw.polygon");
        driver.r#type("3");
        driver.click(0.0, 0.0);
        driver.click(1.0, 0.0);

        // 1 point + 2 lines + 1 polyline + 1 rectangle + 1 polygon.
        let entities = entities_of(driver.document());
        assert_eq!(entities.len(), 6);
        for entity in &entities {
            assert_eq!(entity.layer, "Kerb", "{}", entity.id);
        }
    }
}

// ---- Point -------------------------------------------------------------------------

mod draw_point_tool {
    use super::*;

    #[test]
    fn places_a_point_at_each_click_and_keeps_going_until_esc() {
        let mut driver = ToolDriver::new();
        driver.start("draw.point");
        assert_eq!(driver.prompt(), "Specify a point (x,y,z) or [Height]");
        assert_eq!(driver.expects(), ToolInput::Point);
        assert_eq!(driver.click(1.5, 2.25).outcome, Outcome::Done);
        assert_eq!(driver.click(-3.0, 4.0).outcome, Outcome::Done);

        let placed = points_of(driver.document());
        assert_eq!(placed, [point(1.5, 2.25), point(-3.0, 4.0)]);
        assert_eq!(driver.executed(), 2);
        assert!(!driver.finished());
        assert_eq!(driver.prompt(), "Specify a point (x,y,z) or [Height]");
    }

    #[test]
    fn takes_typed_coordinates() {
        let mut driver = ToolDriver::new();
        driver.start("draw.point");
        assert_eq!(driver.r#type("12.5, -7.25").outcome, Outcome::Done);
        assert_eq!(points_of(driver.document()), [point(12.5, -7.25)]);
    }

    #[test]
    fn each_point_is_its_own_undo_step() {
        let mut driver = ToolDriver::new();
        driver.start("draw.point");
        driver.click(1.0, 1.0);
        driver.click(2.0, 2.0);
        crate::ok(driver.document_mut().undo());
        assert_eq!(points_of(driver.document()), [point(1.0, 1.0)]);
    }

    #[test]
    fn refuses_a_number_on_its_own_and_relative_input_with_nothing_to_measure_from() {
        let mut driver = ToolDriver::new();
        driver.start("draw.point");
        refused(&driver.r#type("12"));
        // Each point starts the tool afresh, so there is never a last point
        // here.
        refused(&driver.r#type("@1,1"));
        assert!(entities_of(driver.document()).is_empty());
        assert_eq!(driver.executed(), 0);
    }

    #[test]
    fn enter_ends_the_tool_without_placing_anything() {
        let mut driver = ToolDriver::new();
        driver.start("draw.point");
        let step = driver.enter();
        assert_eq!(step.outcome, Outcome::Done);
        assert!(driver.finished());
        assert_eq!(driver.executed(), 0);
    }

    #[test]
    fn previews_the_point_at_the_cursor() {
        let mut driver = ToolDriver::new();
        driver.start("draw.point");
        let feedback = driver.preview(3.0, 4.0);
        assert_eq!(
            feedback.shapes,
            [Geometry::Point(PointGeometry {
                position: point(3.0, 4.0)
            })]
        );
    }
}

// ---- Line --------------------------------------------------------------------------

mod draw_line_tool {
    use super::*;

    #[test]
    fn a_chain_of_points_makes_separate_lines_in_one_command() {
        let mut driver = ToolDriver::new();
        driver.start("draw.line");
        assert_eq!(driver.click(0.0, 0.0).outcome, Outcome::Continue);
        assert_eq!(driver.click(4.0, 0.0).outcome, Outcome::Continue);
        assert_eq!(driver.click(4.0, 3.0).outcome, Outcome::Continue);
        assert!(
            entities_of(driver.document()).is_empty(),
            "nothing is drawn before Enter"
        );
        let step = driver.enter();
        assert_eq!(step.outcome, Outcome::Done);
        assert_eq!(step.message, "2 lines");

        assert_eq!(
            entities_of(driver.document()).len(),
            2,
            "separate lines, not a polyline"
        );
        assert_eq!(
            lines_of(driver.document()),
            [
                segment([0.0, 0.0], [4.0, 0.0]),
                segment([4.0, 0.0], [4.0, 3.0])
            ]
        );
        assert_eq!(driver.executed(), 1);
    }

    #[test]
    fn one_undo_of_the_document_removes_the_whole_chain() {
        let mut driver = ToolDriver::new();
        driver.start("draw.line");
        driver.click(0.0, 0.0);
        driver.click(4.0, 0.0);
        driver.click(4.0, 3.0);
        driver.click(0.0, 3.0);
        driver.enter();
        assert_eq!(entities_of(driver.document()).len(), 3);
        crate::ok(driver.document_mut().undo());
        assert!(entities_of(driver.document()).is_empty());
    }

    #[test]
    fn a_single_segment_is_one_line() {
        let mut driver = ToolDriver::new();
        driver.start("draw.line");
        driver.click(1.0, 2.0);
        driver.click(3.0, 5.0);
        let step = driver.enter();
        assert_eq!(step.message, "1 line");
        assert_eq!(
            lines_of(driver.document()),
            [segment([1.0, 2.0], [3.0, 5.0])]
        );
    }

    #[test]
    fn close_joins_the_last_point_back_to_the_first() {
        let mut driver = ToolDriver::new();
        driver.start("draw.line");
        driver.click(0.0, 0.0);
        driver.click(4.0, 0.0);
        driver.click(4.0, 3.0);
        let step = driver.r#type("C");
        assert_eq!(step.outcome, Outcome::Done);
        assert_eq!(step.message, "3 lines");

        let lines = lines_of(driver.document());
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[2], segment([4.0, 3.0], [0.0, 0.0]));
        assert_eq!(driver.executed(), 1);
    }

    #[test]
    fn takes_close_whole_abbreviated_or_in_any_case() {
        for spelling in ["c", "Cl", "close", "CLOSE", " Close "] {
            let mut driver = ToolDriver::new();
            driver.start("draw.line");
            driver.click(0.0, 0.0);
            driver.click(4.0, 0.0);
            driver.click(4.0, 3.0);
            assert_eq!(driver.r#type(spelling).outcome, Outcome::Done, "{spelling}");
            assert_eq!(lines_of(driver.document()).len(), 3, "{spelling}");
        }
    }

    #[test]
    fn undo_takes_back_the_last_point_and_the_chain_goes_on_from_the_one_before() {
        let mut driver = ToolDriver::new();
        driver.start("draw.line");
        driver.click(0.0, 0.0);
        driver.click(4.0, 0.0);
        driver.click(9.0, 9.0);
        assert_eq!(driver.undo().outcome, Outcome::Continue);
        assert_eq!(driver.last_point(), Some(point(4.0, 0.0)));
        driver.click(7.0, 7.0);
        // U typed at the prompt is the same as the Undo button.
        assert_eq!(driver.r#type("u").outcome, Outcome::Continue);
        assert_eq!(driver.last_point(), Some(point(4.0, 0.0)));
        driver.click(4.0, 3.0);
        driver.enter();

        assert_eq!(
            lines_of(driver.document()),
            [
                segment([0.0, 0.0], [4.0, 0.0]),
                segment([4.0, 0.0], [4.0, 3.0])
            ]
        );
    }

    #[test]
    fn takes_relative_and_polar_points_from_the_last_point() {
        let mut driver = ToolDriver::new();
        driver.start("draw.line");
        driver.r#type("1,1");
        // (1, 1) + (3, 0) = (4, 1).
        driver.r#type("@3,0");
        // 2 at 90 degrees is straight up: (4, 1) + (0, 2) = (4, 3).
        driver.r#type("@2<90");
        driver.enter();

        let lines = lines_of(driver.document());
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], segment([1.0, 1.0], [4.0, 1.0]));
        crate::near_point(lines[1].start, point(4.0, 1.0), 1e-12);
        crate::near_point(lines[1].end, point(4.0, 3.0), 1e-12);
    }

    #[test]
    fn a_relative_point_with_a_dz_is_taken_in_plan() {
        // A line has no heights: the dz of @dx,dy,dz is dropped, as the z of
        // x,y,z always was, through both routes typed text takes - the plain
        // one and the command line's with the drafting aids. Refused as "no
        // height for a dz to change", "@10,0,0" drew no line at all.
        let mut driver = ToolDriver::new();
        driver.start("draw.line");
        assert_eq!(driver.r#type("0,0").outcome, Outcome::Continue);
        let flat = driver.r#type("@10,0,0");
        assert_eq!(flat.outcome, Outcome::Continue, "{}", flat.message);
        let mut drafting = DraftingSettings::default();
        let raised = driver.with_tool(|tool, cx| {
            route_typed_input_with_drafting(tool, cx, "@0,5,1", &mut drafting, None)
        });
        assert_eq!(raised.outcome, Outcome::Continue, "{}", raised.message);
        assert_eq!(driver.last_point(), Some(point(10.0, 5.0)));
        driver.enter();
        assert_eq!(
            lines_of(driver.document()),
            [
                segment([0.0, 0.0], [10.0, 0.0]),
                segment([10.0, 0.0], [10.0, 5.0])
            ]
        );
    }

    #[test]
    fn refuses_a_point_on_top_of_the_last_one_and_carries_on() {
        let mut driver = ToolDriver::new();
        driver.start("draw.line");
        driver.click(2.0, 2.0);
        refused(&driver.click(2.0, 2.0));
        // Within the geometric tolerance (1e-7) is the same point.
        refused(&driver.click(2.0 + 5e-8, 2.0));
        assert_eq!(driver.prompt(), "Specify next point or [Undo]");
        driver.click(5.0, 6.0);
        driver.enter();
        assert_eq!(
            lines_of(driver.document()),
            [segment([2.0, 2.0], [5.0, 6.0])]
        );
    }

    #[test]
    fn refuses_to_close_before_there_are_two_lines() {
        let mut driver = ToolDriver::new();
        driver.start("draw.line");
        driver.click(0.0, 0.0);
        driver.click(4.0, 0.0);
        refused(&driver.r#type("C"));
        assert_eq!(driver.last_point(), Some(point(4.0, 0.0)));
        assert!(entities_of(driver.document()).is_empty());
    }

    #[test]
    fn refuses_to_close_a_chain_that_already_ends_at_its_start() {
        let mut driver = ToolDriver::new();
        driver.start("draw.line");
        driver.click(0.0, 0.0);
        driver.click(4.0, 0.0);
        driver.click(4.0, 3.0);
        driver.click(0.0, 0.0);
        // Closing would add a line of zero length.
        refused(&driver.r#type("C"));
        assert_eq!(driver.enter().message, "3 lines");
    }

    #[test]
    fn refuses_undo_before_any_point_and_input_that_is_neither_a_point_nor_an_option() {
        let mut driver = ToolDriver::new();
        driver.start("draw.line");
        refused(&driver.undo());
        refused(&driver.r#type("C"));
        driver.click(0.0, 0.0);
        refused(&driver.r#type("12"));
        refused(&driver.r#type("X"));
        assert_eq!(driver.last_point(), Some(point(0.0, 0.0)));
    }

    #[test]
    fn a_refusal_names_only_the_inputs_the_prompt_would_take() {
        expect_refusals_to_name_only_what_the_prompt_takes("draw.line");
    }

    #[test]
    fn enter_before_the_first_line_ends_the_tool_with_nothing_drawn() {
        let mut driver = ToolDriver::new();
        driver.start("draw.line");
        driver.click(0.0, 0.0);
        assert_eq!(driver.enter().outcome, Outcome::Done);
        assert!(driver.finished());
        assert_eq!(driver.executed(), 0);
        assert!(entities_of(driver.document()).is_empty());
    }

    #[test]
    fn starts_a_new_chain_after_finishing_one() {
        let mut driver = ToolDriver::new();
        driver.start("draw.line");
        driver.click(0.0, 0.0);
        driver.click(4.0, 0.0);
        driver.enter();
        assert!(!driver.finished());
        assert_eq!(driver.prompt(), "Specify first point");
        assert!(driver.last_point().is_none());
        driver.click(10.0, 10.0);
        driver.click(12.0, 10.0);
        driver.enter();
        assert_eq!(lines_of(driver.document()).len(), 2);
        assert_eq!(driver.executed(), 2);
        // Enter with nothing started ends the tool.
        driver.enter();
        assert!(driver.finished());
    }

    #[test]
    fn offers_close_only_once_there_are_two_lines() {
        let mut driver = ToolDriver::new();
        driver.start("draw.line");
        assert_eq!(driver.prompt(), "Specify first point");
        driver.click(0.0, 0.0);
        assert_eq!(driver.prompt(), "Specify next point or [Undo]");
        driver.click(4.0, 0.0);
        assert_eq!(driver.prompt(), "Specify next point or [Undo]");
        driver.click(4.0, 3.0);
        assert_eq!(driver.prompt(), "Specify next point or [Close/Undo]");
    }

    #[test]
    fn previews_the_chain_so_far_and_the_rubber_band_to_the_cursor() {
        let mut driver = ToolDriver::new();
        driver.start("draw.line");
        assert!(driver.preview(1.0, 1.0).shapes.is_empty());
        driver.click(0.0, 0.0);
        driver.click(4.0, 0.0);
        let feedback = driver.preview(4.0, 3.0);
        assert_eq!(
            feedback.shapes,
            [
                Geometry::Line(segment([0.0, 0.0], [4.0, 0.0])),
                Geometry::Line(segment([4.0, 0.0], [4.0, 3.0]))
            ]
        );
        assert_eq!(feedback.markers, points(&[[0.0, 0.0], [4.0, 0.0]]));
    }
}

// ---- Polyline ----------------------------------------------------------------------

mod draw_polyline_tool {
    use super::*;

    #[test]
    fn enter_makes_one_open_polyline_through_the_vertices() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polyline");
        assert_eq!(driver.prompt(), "Specify start point");
        driver.click(0.0, 0.0);
        driver.click(4.0, 0.0);
        driver.click(4.0, 3.0);
        let step = driver.enter();
        assert_eq!(step.outcome, Outcome::Done);
        assert_eq!(step.message, "polyline of 3 vertices");

        assert_eq!(
            only_polyline(driver.document()),
            polyline(&[[0.0, 0.0], [4.0, 0.0], [4.0, 3.0]], false)
        );
        assert_eq!(driver.executed(), 1);
    }

    #[test]
    fn close_makes_it_closed() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polyline");
        driver.click(0.0, 0.0);
        driver.click(4.0, 0.0);
        driver.click(4.0, 3.0);
        assert_eq!(driver.prompt(), "Specify next point or [Arc/Close/Undo]");
        let step = driver.r#type("close");
        assert_eq!(step.outcome, Outcome::Done);
        assert_eq!(step.message, "closed polyline of 3 vertices");
        assert_eq!(
            only_polyline(driver.document()),
            polyline(&[[0.0, 0.0], [4.0, 0.0], [4.0, 3.0]], true)
        );
    }

    #[test]
    fn closing_after_snapping_back_to_the_start_does_not_double_the_first_vertex() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polyline");
        driver.click(0.0, 0.0);
        driver.click(4.0, 0.0);
        driver.click(4.0, 3.0);
        driver.click(0.0, 0.0);
        driver.r#type("C");
        assert_eq!(
            only_polyline(driver.document()),
            polyline(&[[0.0, 0.0], [4.0, 0.0], [4.0, 3.0]], true)
        );
    }

    #[test]
    fn undo_takes_back_the_last_vertex() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polyline");
        driver.click(0.0, 0.0);
        driver.click(4.0, 0.0);
        driver.click(9.0, 9.0);
        assert_eq!(driver.r#type("U").outcome, Outcome::Continue);
        assert_eq!(driver.last_point(), Some(point(4.0, 0.0)));
        driver.click(4.0, 3.0);
        driver.undo();
        driver.undo();
        assert_eq!(driver.last_point(), Some(point(0.0, 0.0)));
        driver.click(0.0, 5.0);
        driver.enter();
        assert_eq!(
            only_polyline(driver.document()),
            polyline(&[[0.0, 0.0], [0.0, 5.0]], false)
        );
    }

    #[test]
    fn takes_relative_and_polar_vertices() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polyline");
        driver.r#type("0,0");
        driver.r#type("@5,0"); // (5, 0)
        driver.r#type("@5<90"); // (5, 0) + 5 straight up = (5, 5)
        driver.r#type("@-5,0"); // (0, 5)
        driver.r#type("C");
        let shape = only_polyline(driver.document());
        assert!(shape.closed);
        vertices_near(
            &shape.vertices,
            &points(&[[0.0, 0.0], [5.0, 0.0], [5.0, 5.0], [0.0, 5.0]]),
            1e-12,
        );
    }

    #[test]
    fn one_undo_of_the_document_removes_the_polyline() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polyline");
        driver.click(0.0, 0.0);
        driver.click(4.0, 0.0);
        driver.click(4.0, 3.0);
        driver.enter();
        assert_eq!(entities_of(driver.document()).len(), 1);
        crate::ok(driver.document_mut().undo());
        assert!(entities_of(driver.document()).is_empty());
    }

    #[test]
    fn refuses_a_coincident_vertex_and_close_with_fewer_than_three_vertices() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polyline");
        refused(&driver.undo());
        driver.click(0.0, 0.0);
        refused(&driver.click(0.0, 0.0));
        driver.click(4.0, 0.0);
        refused(&driver.r#type("C"));
        // Back to the start and Close would leave two vertices: a line, not a
        // closed shape.
        driver.click(0.0, 0.0);
        refused(&driver.r#type("C"));
        refused(&driver.r#type("12"));
        assert!(entities_of(driver.document()).is_empty());
        assert_eq!(driver.last_point(), Some(point(0.0, 0.0)));
    }

    #[test]
    fn enter_with_one_vertex_ends_the_tool_with_nothing_drawn() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polyline");
        driver.click(3.0, 3.0);
        assert_eq!(driver.enter().outcome, Outcome::Done);
        assert!(driver.finished());
        assert!(entities_of(driver.document()).is_empty());
    }

    #[test]
    fn starts_a_new_polyline_after_finishing_one() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polyline");
        driver.click(0.0, 0.0);
        driver.click(1.0, 0.0);
        driver.enter();
        assert!(!driver.finished());
        assert_eq!(driver.prompt(), "Specify start point");
        driver.click(0.0, 1.0);
        driver.click(1.0, 1.0);
        driver.enter();
        assert_eq!(polylines_of(driver.document()).len(), 2);
    }

    #[test]
    fn a_refusal_names_only_the_inputs_the_prompt_would_take() {
        expect_refusals_to_name_only_what_the_prompt_takes("draw.polyline");
    }

    #[test]
    fn previews_the_polyline_running_on_to_the_cursor() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polyline");
        driver.click(0.0, 0.0);
        let feedback = driver.preview(3.0, 4.0);
        assert_eq!(
            feedback.shapes,
            [Geometry::Polyline(polyline(
                &[[0.0, 0.0], [3.0, 4.0]],
                false
            ))]
        );

        driver.click(4.0, 0.0);
        let feedback = driver.preview(4.0, 3.0);
        assert_eq!(
            feedback.shapes,
            [Geometry::Polyline(polyline(
                &[[0.0, 0.0], [4.0, 0.0], [4.0, 3.0]],
                false
            ))]
        );
        assert_eq!(feedback.markers, points(&[[0.0, 0.0], [4.0, 0.0]]));

        // The cursor on the last vertex adds nothing to what is already there.
        let feedback = driver.preview(4.0, 0.0);
        assert_eq!(
            feedback.shapes,
            [Geometry::Polyline(polyline(
                &[[0.0, 0.0], [4.0, 0.0]],
                false
            ))]
        );
    }
}

// ---- Rectangle ---------------------------------------------------------------------

mod draw_rectangle_tool {
    use super::*;

    #[test]
    fn two_corners_make_a_closed_rectangle_counter_clockwise_from_the_lower_left() {
        let mut driver = ToolDriver::new();
        driver.start("draw.rectangle");
        assert_eq!(driver.prompt(), "Specify first corner point");
        assert_eq!(driver.click(0.0, 0.0).outcome, Outcome::Continue);
        assert_eq!(
            driver.prompt(),
            "Specify other corner point or [Dimensions/Undo]"
        );
        let step = driver.click(4.0, 3.0);
        assert_eq!(step.outcome, Outcome::Done);
        assert_eq!(step.message, "rectangle");
        assert_eq!(
            only_polyline(driver.document()),
            polyline(&[[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]], true)
        );
        assert_eq!(driver.executed(), 1);
    }

    #[test]
    fn the_corners_may_be_either_pair_in_either_order() {
        let expected = polyline(&[[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]], true);
        let corners = [
            (point(4.0, 3.0), point(0.0, 0.0)),
            (point(0.0, 3.0), point(4.0, 0.0)),
            (point(4.0, 0.0), point(0.0, 3.0)),
        ];
        for (first, second) in corners {
            let mut driver = ToolDriver::new();
            driver.start("draw.rectangle");
            driver.click(first.x, first.y);
            driver.click(second.x, second.y);
            assert_eq!(
                only_polyline(driver.document()),
                expected,
                "{first:?} {second:?}"
            );
        }
    }

    #[test]
    fn the_other_corner_can_be_typed_as_width_and_height_from_the_first() {
        let mut driver = ToolDriver::new();
        driver.start("draw.rectangle");
        driver.r#type("1,2");
        // (1, 2) + (4, 3) = (5, 5).
        driver.r#type("@4,3");
        // From (4, 3) back by (4, 3) is the origin; the outline still starts
        // at the lower left corner.
        driver.r#type("4,3");
        driver.r#type("@-4,-3");
        assert_eq!(
            polylines_of(driver.document()),
            [
                polyline(&[[1.0, 2.0], [5.0, 2.0], [5.0, 5.0], [1.0, 5.0]], true),
                polyline(&[[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]], true)
            ]
        );
    }

    #[test]
    fn refuses_corners_in_line_and_waits_for_another() {
        let mut driver = ToolDriver::new();
        driver.start("draw.rectangle");
        driver.click(0.0, 0.0);
        refused(&driver.click(4.0, 0.0)); // no height
        refused(&driver.click(0.0, 3.0)); // no width
        refused(&driver.click(0.0, 0.0)); // neither
        refused(&driver.r#type("@5,0"));
        refused(&driver.r#type("12"));
        assert!(entities_of(driver.document()).is_empty());
        assert_eq!(driver.last_point(), Some(point(0.0, 0.0)));
        assert_eq!(driver.click(4.0, 3.0).outcome, Outcome::Done);
    }

    #[test]
    fn undo_takes_back_the_first_corner() {
        let mut driver = ToolDriver::new();
        driver.start("draw.rectangle");
        refused(&driver.undo());
        driver.click(0.0, 0.0);
        assert_eq!(driver.undo().outcome, Outcome::Continue);
        assert_eq!(driver.prompt(), "Specify first corner point");
        assert!(driver.last_point().is_none());
        driver.click(1.0, 1.0);
        driver.click(2.0, 3.0);
        assert_eq!(
            only_polyline(driver.document()),
            polyline(&[[1.0, 1.0], [2.0, 1.0], [2.0, 3.0], [1.0, 3.0]], true)
        );
    }

    #[test]
    fn dimensions_takes_typed_sizes_and_a_click_on_the_side_it_goes() {
        let mut driver = ToolDriver::new();
        driver.start("draw.rectangle");
        driver.click(10.0, 10.0);
        assert_eq!(driver.r#type("d").outcome, Outcome::Continue);
        assert_eq!(driver.expects(), ToolInput::Value);
        assert_eq!(
            driver.prompt(),
            "Specify length of the rectangle along x or [Undo]"
        );
        driver.r#type("4");
        assert_eq!(
            driver.prompt(),
            "Specify width of the rectangle along y or [Undo]"
        );
        driver.r#type("3");
        assert_eq!(driver.expects(), ToolInput::Point);
        // Below and to the left of (10, 10): the far corner is (10 - 4, 10 - 3).
        assert_eq!(driver.click(0.0, 0.0).outcome, Outcome::Done);
        assert_eq!(
            only_polyline(driver.document()),
            polyline(&[[6.0, 7.0], [10.0, 7.0], [10.0, 10.0], [6.0, 10.0]], true)
        );
    }

    #[test]
    fn dimensions_goes_up_and_right_on_enter() {
        let mut driver = ToolDriver::new();
        driver.start("draw.rectangle");
        driver.click(10.0, 10.0);
        driver.r#type("Dimensions");
        driver.r#type("4");
        driver.r#type("2.5");
        assert_eq!(driver.enter().outcome, Outcome::Done);
        assert_eq!(
            only_polyline(driver.document()),
            polyline(
                &[[10.0, 10.0], [14.0, 10.0], [14.0, 12.5], [10.0, 12.5]],
                true
            )
        );
    }

    #[test]
    fn dimensions_refuses_a_size_that_is_not_a_positive_number() {
        let mut driver = ToolDriver::new();
        driver.start("draw.rectangle");
        driver.click(0.0, 0.0);
        driver.r#type("D");
        refused(&driver.r#type("0"));
        refused(&driver.r#type("-2"));
        refused(&driver.r#type("abc"));
        refused(&driver.click(5.0, 5.0));
        refused(&driver.enter());
        driver.r#type("4");
        refused(&driver.r#type("0"));
        refused(&driver.enter());
        assert_eq!(
            driver.prompt(),
            "Specify width of the rectangle along y or [Undo]"
        );
        assert!(entities_of(driver.document()).is_empty());
    }

    #[test]
    fn dimensions_steps_back_with_undo() {
        let mut driver = ToolDriver::new();
        driver.start("draw.rectangle");
        driver.click(10.0, 10.0);
        driver.r#type("D");
        driver.r#type("4");
        driver.r#type("U");
        assert_eq!(
            driver.prompt(),
            "Specify length of the rectangle along x or [Undo]"
        );
        driver.undo();
        assert_eq!(
            driver.prompt(),
            "Specify other corner point or [Dimensions/Undo]"
        );
        driver.click(12.0, 13.0);
        assert_eq!(
            only_polyline(driver.document()),
            polyline(
                &[[10.0, 10.0], [12.0, 10.0], [12.0, 13.0], [10.0, 13.0]],
                true
            )
        );
    }

    #[test]
    fn one_undo_removes_it_and_the_tool_starts_again() {
        let mut driver = ToolDriver::new();
        driver.start("draw.rectangle");
        driver.click(0.0, 0.0);
        driver.click(4.0, 3.0);
        assert!(!driver.finished());
        assert_eq!(driver.prompt(), "Specify first corner point");
        crate::ok(driver.document_mut().undo());
        assert!(entities_of(driver.document()).is_empty());
        // Enter at the first corner ends the tool.
        driver.enter();
        assert!(driver.finished());
    }

    #[test]
    fn previews_the_rectangle_to_the_cursor() {
        let mut driver = ToolDriver::new();
        driver.start("draw.rectangle");
        assert!(driver.preview(4.0, 3.0).shapes.is_empty());
        driver.click(0.0, 0.0);
        let feedback = driver.preview(4.0, 3.0);
        assert_eq!(
            feedback.shapes,
            [Geometry::Polyline(polyline(
                &[[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]],
                true
            ))]
        );
        assert_eq!(feedback.markers, [point(0.0, 0.0)]);
        // In line with the corner the rectangle is only its one side.
        let feedback = driver.preview(4.0, 0.0);
        assert_eq!(
            feedback.shapes,
            [Geometry::Line(segment([0.0, 0.0], [4.0, 0.0]))]
        );
        assert!(driver.preview(0.0, 0.0).shapes.is_empty());

        // With typed sizes, the rectangle on the cursor's side of the corner.
        driver.r#type("D");
        driver.r#type("4");
        driver.r#type("3");
        let feedback = driver.preview(-1.0, -1.0);
        assert_eq!(
            feedback.shapes,
            [Geometry::Polyline(polyline(
                &[[-4.0, -3.0], [0.0, -3.0], [0.0, 0.0], [-4.0, 0.0]],
                true
            ))]
        );
    }
}

// ---- Polygon -----------------------------------------------------------------------

mod draw_polygon_tool {
    use super::*;

    const INSCRIBED: &str = "Specify a vertex or type the radius, or [Circumscribed/Undo]";
    const CIRCUMSCRIBED: &str =
        "Specify the middle of a side or type the radius, or [Inscribed/Undo]";

    #[test]
    fn asks_for_the_number_of_sides_first() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polygon");
        assert_eq!(driver.prompt(), "Enter number of sides");
        assert_eq!(driver.expects(), ToolInput::Value);
        assert_eq!(driver.r#type("4").outcome, Outcome::Continue);
        assert_eq!(driver.prompt(), "Specify centre of polygon or [Edge/Undo]");
        driver.click(0.0, 0.0);
        assert_eq!(driver.prompt(), INSCRIBED);
        let step = driver.click(1.0, 0.0);
        assert_eq!(step.outcome, Outcome::Done);
        assert_eq!(step.message, "polygon of 4 sides");
        // Inscribed: (1, 0) is a vertex; the others are it turned by 90, 180
        // and 270 degrees about the centre.
        let shape = only_polyline(driver.document());
        assert!(shape.closed);
        vertices_near(
            &shape.vertices,
            &points(&[[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], [0.0, -1.0]]),
            1e-12,
        );
    }

    #[test]
    fn an_inscribed_hexagon_has_a_vertex_at_the_picked_point() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polygon");
        driver.r#type("6");
        driver.click(0.0, 0.0);
        driver.click(2.0, 0.0);
        // Radius 2 at 0, 60, ... 300 degrees: (2cos60, 2sin60) = (1, sqrt3).
        let shape = only_polyline(driver.document());
        assert!(shape.closed);
        assert_eq!(
            shape.vertices[0],
            point(2.0, 0.0),
            "the picked point itself"
        );
        let r3 = root3();
        vertices_near(
            &shape.vertices,
            &points(&[
                [2.0, 0.0],
                [1.0, r3],
                [-1.0, r3],
                [-2.0, 0.0],
                [-1.0, -r3],
                [1.0, -r3],
            ]),
            1e-12,
        );
    }

    #[test]
    fn a_circumscribed_polygon_has_the_middle_of_a_side_at_the_picked_point() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polygon");
        driver.r#type("4");
        driver.click(0.0, 0.0);
        assert_eq!(driver.r#type("C").outcome, Outcome::Continue);
        assert_eq!(driver.prompt(), CIRCUMSCRIBED);
        driver.click(0.0, -1.0);
        // The square about the unit circle: the side through (0, -1) runs from
        // (-1, -1) to (1, -1), and the first vertex is 45 degrees clockwise of
        // the picked direction at radius sqrt2, (-1, -1).
        let shape = only_polyline(driver.document());
        vertices_near(
            &shape.vertices,
            &points(&[[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]),
            1e-12,
        );
    }

    #[test]
    fn a_circumscribed_triangle_or_hexagon_has_every_side_tangent_to_the_circle() {
        // A square cannot tell the apothem-to-corner factor 1/cos(pi/n) from
        // 1/sin(pi/n), nor a turn of pi/n from one of pi/2 - pi/n: at n = 4
        // they are equal. A hexagon and a triangle can.
        let mut driver = ToolDriver::new();
        driver.start("draw.polygon");
        driver.r#type("6");
        driver.click(0.0, 0.0);
        driver.r#type("C");
        driver.click(0.0, -1.0);
        // About the unit circle the hexagon's corners are 1/cos30 = 2/sqrt3
        // out. The first is the picked direction, -90 degrees, turned 30
        // degrees clockwise to -120: (2/sqrt3)(cos -120, sin -120) =
        // (-1/sqrt3, -1). The rest are 60 degrees apart: -60, 0, 60, 120 and
        // 180.
        driver.r#type("3");
        driver.click(0.0, 0.0);
        driver.r#type("C");
        driver.r#type("1");
        // A typed radius puts the bottom side's middle at (0, -1). The corners
        // are 1/cos60 = 2 out, the first at -90 - 60 = -150 degrees:
        // 2(cos -150, sin -150) = (-sqrt3, -1); then -30 and 90.
        let polygons = polylines_of(driver.document());
        assert_eq!(polygons.len(), 2);
        let r3 = root3();
        let third = 1.0 / r3;
        vertices_near(
            &polygons[0].vertices,
            &points(&[
                [-third, -1.0],
                [third, -1.0],
                [2.0 * third, 0.0],
                [third, 1.0],
                [-third, 1.0],
                [-2.0 * third, 0.0],
            ]),
            1e-12,
        );
        vertices_near(
            &polygons[1].vertices,
            &points(&[[-r3, -1.0], [r3, -1.0], [0.0, 2.0]]),
            1e-12,
        );
        // Tangent: the middle of every side is on the unit circle.
        for polygon in &polygons {
            let n = polygon.vertices.len();
            for i in 0..n {
                let middle = (polygon.vertices[i] + polygon.vertices[(i + 1) % n]) * 0.5;
                near(middle.length(), 1.0, 1e-12);
            }
        }
    }

    #[test]
    fn a_typed_radius_draws_the_bottom_side_level() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polygon");
        driver.r#type("4");
        driver.click(0.0, 0.0);
        driver.r#type("2");
        // Inscribed in radius 2 with the bottom side level: vertices at -135,
        // -45, 45 and 135 degrees, 2(cos -135, sin -135) = (-sqrt2, -sqrt2).
        driver.r#type("3");
        driver.click(0.0, 0.0);
        driver.r#type("2");
        // A triangle the same way: -150, -30 and 90 degrees;
        // 2(cos -150, sin -150) = (-sqrt3, -1).
        driver.r#type("4");
        driver.click(0.0, 0.0);
        driver.r#type("c");
        driver.r#type("1");
        // About the unit circle: the 2 x 2 square centred on the origin.
        let polygons = polylines_of(driver.document());
        assert_eq!(polygons.len(), 3);
        let r3 = root3();
        vertices_near(
            &polygons[0].vertices,
            &points(&[
                [-ROOT2, -ROOT2],
                [ROOT2, -ROOT2],
                [ROOT2, ROOT2],
                [-ROOT2, ROOT2],
            ]),
            1e-12,
        );
        vertices_near(
            &polygons[1].vertices,
            &points(&[[-r3, -1.0], [r3, -1.0], [0.0, 2.0]]),
            1e-12,
        );
        vertices_near(
            &polygons[2].vertices,
            &points(&[[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]),
            1e-12,
        );
    }

    #[test]
    fn takes_the_radius_point_as_polar_input_from_the_centre() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polygon");
        driver.r#type("3");
        driver.r#type("1,1");
        assert_eq!(driver.last_point(), Some(point(1.0, 1.0)));
        driver.r#type("@2<90");
        // A vertex 2 straight up from (1, 1) at (1, 3); the others are (0, 2)
        // turned by 120 and 240 degrees, (-sqrt3, -1) and (sqrt3, -1), from
        // (1, 1).
        let shape = only_polyline(driver.document());
        let r3 = root3();
        vertices_near(
            &shape.vertices,
            &points(&[[1.0, 3.0], [1.0 - r3, 0.0], [1.0 + r3, 0.0]]),
            1e-12,
        );
    }

    #[test]
    fn edge_draws_the_polygon_on_the_left_of_the_edge_from_its_first_end() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polygon");
        driver.r#type("4");
        assert_eq!(driver.r#type("e").outcome, Outcome::Continue);
        assert_eq!(driver.prompt(), "Specify first endpoint of edge or [Undo]");
        driver.click(0.0, 0.0);
        assert_eq!(driver.prompt(), "Specify second endpoint of edge or [Undo]");
        driver.click(2.0, 0.0);
        // Reversed, the square falls on the other side.
        driver.r#type("4");
        driver.r#type("E");
        driver.click(2.0, 0.0);
        driver.click(0.0, 0.0);
        // A hexagon on the unit edge: each side turns 60 degrees left of the
        // one before, (1, 0) + (cos60, sin60) = (1.5, sqrt3/2), and so on.
        driver.r#type("6");
        driver.r#type("E");
        driver.click(0.0, 0.0);
        driver.click(1.0, 0.0);
        let polygons = polylines_of(driver.document());
        assert_eq!(polygons.len(), 3);
        let r3 = root3();
        vertices_near(
            &polygons[0].vertices,
            &points(&[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]]),
            1e-12,
        );
        vertices_near(
            &polygons[1].vertices,
            &points(&[[2.0, 0.0], [0.0, 0.0], [0.0, -2.0], [2.0, -2.0]]),
            1e-12,
        );
        vertices_near(
            &polygons[2].vertices,
            &points(&[
                [0.0, 0.0],
                [1.0, 0.0],
                [1.5, r3 / 2.0],
                [1.0, r3],
                [0.0, r3],
                [-0.5, r3 / 2.0],
            ]),
            1e-12,
        );
        for polygon in &polygons {
            assert!(polygon.closed);
        }
    }

    #[test]
    fn takes_its_options_whole_abbreviated_or_in_any_case() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polygon");
        driver.r#type("5");
        driver.click(0.0, 0.0);
        driver.r#type("circ");
        assert_eq!(driver.prompt(), CIRCUMSCRIBED);
        driver.r#type("INSCRIBED");
        assert_eq!(driver.prompt(), INSCRIBED);
        driver.r#type("Circumscribed");
        assert_eq!(driver.prompt(), CIRCUMSCRIBED);
        driver.r#type("i");
        assert_eq!(driver.prompt(), INSCRIBED);
    }

    #[test]
    fn refuses_a_number_of_sides_outside_three_to1024() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polygon");
        for sides in ["2", "1025", "0", "-5", "4.5", "six", "C"] {
            refused(&driver.r#type(sides));
            assert_eq!(driver.prompt(), "Enter number of sides", "{sides}");
        }
        refused(&driver.click(1.0, 1.0));
        assert_eq!(driver.r#type("3").outcome, Outcome::Continue);
        driver.undo();
        assert_eq!(driver.r#type("1024").outcome, Outcome::Continue);
        assert_eq!(driver.prompt(), "Specify centre of polygon or [Edge/Undo]");
    }

    #[test]
    fn refuses_a_radius_that_is_not_greater_than_zero() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polygon");
        driver.r#type("5");
        refused(&driver.r#type("12")); // a number is not a centre
        driver.click(0.0, 0.0);
        refused(&driver.click(0.0, 0.0));
        refused(&driver.click(5e-8, 0.0)); // within the geometric tolerance
        refused(&driver.r#type("0"));
        refused(&driver.r#type("-3"));
        refused(&driver.r#type("abc"));
        assert_eq!(driver.last_point(), Some(point(0.0, 0.0)));
        assert!(entities_of(driver.document()).is_empty());
    }

    #[test]
    fn refuses_an_edge_whose_ends_are_one_point() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polygon");
        driver.r#type("5");
        driver.r#type("E");
        driver.click(3.0, 3.0);
        refused(&driver.click(3.0, 3.0));
        refused(&driver.r#type("7"));
        assert_eq!(driver.prompt(), "Specify second endpoint of edge or [Undo]");
        assert!(entities_of(driver.document()).is_empty());
    }

    #[test]
    fn undo_steps_back_through_the_centre_and_the_number_of_sides() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polygon");
        refused(&driver.undo());
        driver.r#type("6");
        driver.click(0.0, 0.0);
        assert_eq!(driver.undo().outcome, Outcome::Continue);
        assert_eq!(driver.prompt(), "Specify centre of polygon or [Edge/Undo]");
        driver.r#type("E");
        driver.click(1.0, 1.0);
        driver.r#type("U");
        assert_eq!(driver.prompt(), "Specify first endpoint of edge or [Undo]");
        driver.undo();
        assert_eq!(driver.prompt(), "Specify centre of polygon or [Edge/Undo]");
        driver.undo();
        assert_eq!(driver.prompt(), "Enter number of sides");
        refused(&driver.undo());
        // Back at the start: a new count, and a square from it.
        driver.r#type("4");
        driver.click(0.0, 0.0);
        driver.click(0.0, 1.0);
        assert_eq!(only_polyline(driver.document()).vertices.len(), 4);
    }

    #[test]
    fn undo_takes_back_an_inscribed_or_circumscribed_switch_before_the_centre() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polygon");
        driver.r#type("4");
        driver.click(0.0, 0.0);
        // Each switch is an input, so Undo takes them back one at a time,
        // latest first, and only then the centre.
        driver.r#type("C");
        driver.r#type("I");
        driver.r#type("C");
        assert_eq!(driver.undo().outcome, Outcome::Continue);
        assert_eq!(driver.prompt(), INSCRIBED);
        assert_eq!(
            driver.last_point(),
            Some(point(0.0, 0.0)),
            "the centre is kept"
        );
        driver.r#type("U");
        assert_eq!(driver.prompt(), CIRCUMSCRIBED);
        driver.undo();
        assert_eq!(driver.prompt(), INSCRIBED);
        assert_eq!(driver.last_point(), Some(point(0.0, 0.0)));
        // Inscribed again, so (1, 0) is a vertex of the square round the kept
        // centre: (1, 0) turned by 90, 180 and 270 degrees.
        driver.click(1.0, 0.0);
        let shape = only_polyline(driver.document());
        vertices_near(
            &shape.vertices,
            &points(&[[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], [0.0, -1.0]]),
            1e-12,
        );
    }

    #[test]
    fn undo_after_every_mode_switch_is_taken_back_goes_on_to_the_centre() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polygon");
        driver.r#type("4");
        driver.click(0.0, 0.0);
        driver.r#type("C");
        driver.undo();
        driver.undo();
        assert_eq!(driver.prompt(), "Specify centre of polygon or [Edge/Undo]");
        assert!(driver.last_point().is_none());
        // The mode is the one the centre was given in: every switch after it
        // was undone, so a new centre starts Inscribed.
        driver.click(5.0, 5.0);
        assert_eq!(driver.prompt(), INSCRIBED);
    }

    #[test]
    fn enter_after_a_finished_polygon_ends_the_tool_as_in_the_other_line_tools() {
        // The tool restarts after each polygon, so Enter (or a right-click) at
        // the fresh prompt must be the way out, as it is in Point, Line,
        // Polyline and Rectangle - not a default count that arms the next two
        // clicks to draw a square.
        let mut driver = ToolDriver::new();
        driver.start("draw.polygon");
        driver.r#type("5");
        driver.click(0.0, 0.0);
        driver.click(1.0, 0.0);
        assert_eq!(driver.executed(), 1);
        assert!(!driver.finished());
        assert_eq!(driver.enter().outcome, Outcome::Done);
        assert!(driver.finished());
        assert_eq!(driver.executed(), 1);
        assert_eq!(polylines_of(driver.document()).len(), 1);
    }

    #[test]
    fn enter_at_the_sides_or_the_centre_prompt_ends_the_tool_with_nothing_drawn() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polygon");
        assert_eq!(driver.enter().outcome, Outcome::Done);
        assert!(driver.finished());

        driver.start("draw.polygon");
        driver.r#type("6");
        assert_eq!(driver.prompt(), "Specify centre of polygon or [Edge/Undo]");
        assert_eq!(driver.enter().outcome, Outcome::Done);
        assert!(driver.finished());
        assert_eq!(driver.executed(), 0);
        assert!(entities_of(driver.document()).is_empty());
    }

    #[test]
    fn one_undo_removes_the_polygon_and_the_tool_asks_for_the_sides_again() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polygon");
        driver.r#type("8");
        driver.click(0.0, 0.0);
        driver.click(3.0, 0.0);
        assert_eq!(driver.executed(), 1);
        assert!(!driver.finished());
        assert_eq!(driver.prompt(), "Enter number of sides");
        crate::ok(driver.document_mut().undo());
        assert!(entities_of(driver.document()).is_empty());
    }

    #[test]
    fn previews_the_polygon_for_the_cursor() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polygon");
        driver.r#type("6");
        assert!(driver.preview(2.0, 0.0).shapes.is_empty());
        driver.click(0.0, 0.0);
        let feedback = driver.preview(2.0, 0.0);
        assert_eq!(feedback.shapes.len(), 1);
        let Geometry::Polyline(hexagon) = &feedback.shapes[0] else {
            panic!("{:?} is not a polyline", feedback.shapes[0]);
        };
        assert!(hexagon.closed);
        let r3 = root3();
        vertices_near(
            &hexagon.vertices,
            &points(&[
                [2.0, 0.0],
                [1.0, r3],
                [-1.0, r3],
                [-2.0, 0.0],
                [-1.0, -r3],
                [1.0, -r3],
            ]),
            1e-12,
        );
        assert_eq!(feedback.markers, [point(0.0, 0.0)]);
        assert!(driver.preview(0.0, 0.0).shapes.is_empty());

        driver.undo();
        driver.r#type("E");
        driver.click(0.0, 0.0);
        let feedback = driver.preview(1.0, 0.0);
        assert_eq!(feedback.shapes.len(), 1);
        let Geometry::Polyline(on_edge) = &feedback.shapes[0] else {
            panic!("{:?} is not a polyline", feedback.shapes[0]);
        };
        vertices_near(
            &on_edge.vertices,
            &points(&[
                [0.0, 0.0],
                [1.0, 0.0],
                [1.5, r3 / 2.0],
                [1.0, r3],
                [0.0, r3],
                [-0.5, r3 / 2.0],
            ]),
            1e-12,
        );
    }

    #[test]
    fn every_vertex_of_a1024_gon_is_on_the_circle() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polygon");
        driver.r#type("1024");
        driver.click(0.0, 0.0);
        driver.click(1000.0, 0.0);
        let shape = only_polyline(driver.document());
        assert_eq!(shape.vertices.len(), 1024);
        // Each vertex is the first turned about the centre, so none carries the
        // rounding of the ones before it: one cos, one sin and a product each,
        // a few ulps. An ulp of 1000 is 2^-43, about 1.1e-13, so 1e-12 is some
        // nine ulps. Turning the previous vertex again instead adds a turn's
        // rounding at each of 1023 steps, and that walks tens of times past
        // it. Vertex 512 is half a turn round, (-1000, 0).
        for vertex in &shape.vertices {
            near(vertex.length(), 1000.0, 1e-12);
        }
        crate::near_point(shape.vertices[512], point(-1000.0, 0.0), 1e-12);
    }
}
