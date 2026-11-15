// Arranging a sheet, and the paper, scale and rotation advice
// (include/katana/cad/plotting/arrange.hpp); tests/cad/plotting/test_arrange.cpp.
//
// The rectangles are worked by hand from the A3 frame's tiling area -
// 24..409 x 36..286 mm, 385 x 250 - and, for the other sizes, from the A3
// drawing area 23..410 x 35..287 scaled by the frame's factor (A4 210 / 297,
// A2 420 / 297) and inset by 1 mm; a portrait or frameless sheet is the paper
// less 10 mm, inset by 1. The rotations are solved by hand in the comments.

use std::f64::consts::PI;

use crate::{expect_box, near, ok, refused};
use katana_cad::LayerOverrides;
use katana_cad::drawing::construction::CONSTRUCTION_LAYER;
use katana_cad::plot::PaperSize;
use katana_cad::plotting::arrange::{
    AUTO_SCALE_SPARE, AlignEdge, ArrangeResult, DistributeAxis, PaperAdvice, PaperRequest,
    align_edge_from, align_viewports, apply_paper, auto_arrange, best_fit_rotation,
    best_standard_rotation, distribute_axis_from, distribute_viewports, drawn_outline, draws_in,
    fit_at_rotation, fit_paper_to_viewport, fit_viewport_to_content, least_rotation_to_fit,
    match_scale, rotate_to_best_fit, smart_layout_rotated, suggest_paper_for_extent, suggest_scale,
    suggest_scale_on_sheet, viewport_content, viewport_content_in_set,
};
use katana_cad::plotting::frame::BUILT_IN_FRAME_ID;
use katana_cad::plotting::generators::smart_layout;
use katana_cad::plotting::layout::{
    TILING_GUTTER_MM, minimum_size, preset_cells, tile_viewports, tiling_area,
};
use katana_cad::plotting::sheet_set::drawing_area;
use katana_cad::plotting::{
    LayoutRequest, Sheet, SheetSet, SizeMm, TilingPreset, Viewport, ViewportKind, WorldMark,
    WorldMarkKind,
};
use katana_core::ErrorCode;
use katana_entity::{
    Alignment, Entity, EnumName, Geometry, HatchLineFamily, HatchPattern, Layer, Model,
};
use katana_geometry::{AlignmentPi, Arc2, Box2, Circle2, Point2, Polyline2, Segment2, Vec2};

const DEGREE: f64 = PI / 180.0;

// A `width` x `height` rectangle's corners, turned `turn` about its centre.
fn turned_rectangle(centre: Point2, width: f64, height: f64, turn: f64) -> Vec<Point2> {
    [
        Vec2::new(-width / 2.0, -height / 2.0),
        Vec2::new(width / 2.0, -height / 2.0),
        Vec2::new(width / 2.0, height / 2.0),
        Vec2::new(-width / 2.0, height / 2.0),
    ]
    .iter()
    .map(|corner| centre + corner.rotated(turn))
    .collect()
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Box2 {
    Box2::new(Point2::new(x0, y0), Point2::new(x1, y1))
}

fn viewport_at(id: &str, kind: ViewportKind, rect: Box2, locked: bool) -> Viewport {
    Viewport {
        id: String::from(id),
        kind,
        rect,
        locked,
        ..Viewport::default()
    }
}

fn by_id<'s>(sheet: &'s Sheet, id: &str) -> &'s Viewport {
    sheet
        .viewports
        .iter()
        .find(|viewport| viewport.id == id)
        .unwrap_or_else(|| panic!("no viewport {id}"))
}

fn overlap(a: &Box2, b: &Box2) -> bool {
    a.min.x < b.max.x - 1e-9
        && b.min.x < a.max.x - 1e-9
        && a.min.y < b.max.y - 1e-9
        && b.min.y < a.max.y - 1e-9
}

// No two placed viewports of `sheet` overlap, and every one is on the drawing
// area.
#[track_caller]
fn expect_tidy(sheet: &Sheet) {
    let area = drawing_area(sheet);
    for (i, viewport) in sheet.viewports.iter().enumerate() {
        let a = &viewport.rect;
        assert!(
            area.contains_box(*a),
            "{} is off the drawing area",
            viewport.id
        );
        for other in &sheet.viewports[i + 1..] {
            assert!(
                !overlap(a, &other.rect),
                "{} overlaps {}",
                viewport.id,
                other.id
            );
        }
    }
}

fn area_of(rect: &Box2) -> f64 {
    rect.width() * rect.height()
}

// The A3 tiling area as a size.
const A3_ROOM: SizeMm = SizeMm {
    width: 385.0,
    height: 250.0,
};

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|id| String::from(*id)).collect()
}

fn add_to(model: &mut Model, geometry: Geometry, layer: &str) {
    let entity = Entity {
        geometry,
        layer: String::from(layer),
        ..Entity::default()
    };
    ok(model.entities.add(entity));
}

fn segment(x0: f64, y0: f64, x1: f64, y1: f64) -> Geometry {
    Geometry::Line(Segment2::new(Point2::new(x0, y0), Point2::new(x1, y1)))
}

fn straight_road(x0: f64, y0: f64, x1: f64, y1: f64) -> Alignment {
    let mut road = Alignment {
        name: String::from("ROAD"),
        ..Alignment::default()
    };
    road.horizontal.pis = vec![
        AlignmentPi {
            point: Point2::new(x0, y0),
            ..AlignmentPi::default()
        },
        AlignmentPi {
            point: Point2::new(x1, y1),
            ..AlignmentPi::default()
        },
    ];
    road
}

fn box_of(points: &[Point2]) -> Box2 {
    let mut extent = Box2::default();
    for &point in points {
        extent.expand(point);
    }
    extent
}

fn three_views() -> Sheet {
    Sheet {
        viewports: vec![
            viewport_at(
                "a",
                ViewportKind::Legend,
                rect(30.0, 40.0, 80.0, 80.0),
                false,
            ),
            viewport_at(
                "b",
                ViewportKind::Notes,
                rect(100.0, 50.0, 130.0, 120.0),
                false,
            ),
            viewport_at(
                "c",
                ViewportKind::Image,
                rect(200.0, 45.0, 260.0, 60.0),
                false,
            ),
        ],
        ..Sheet::default()
    }
}

fn abc() -> Vec<String> {
    ids(&["a", "b", "c"])
}

mod sheet_arrange {
    use super::*;

    // ---- The best rotation --------------------------------------------------------------

    #[test]
    fn a_rectangle_turned_thirty_degrees_is_turned_back_to_lie_across_the_paper() {
        // 300 x 100 m lying along 30 degrees: square to it, it needs
        // 1 : 300000 / 385 = 779.22 across and 100000 / 250 = 400 up. Any
        // other turn widens it (the width grows at 100 m a radian either way),
        // and a quarter turn needs 1 : 1200.
        let content = turned_rectangle(Point2::new(1000.0, 2000.0), 300.0, 100.0, 30.0 * DEGREE);
        let fit = ok(best_fit_rotation(&content, A3_ROOM));
        near(fit.rotation, 30.0 * DEGREE, 1e-9);
        near(fit.width, 300.0, 1e-9);
        near(fit.height, 100.0, 1e-9);
        near(fit.scale, 300000.0 / 385.0, 1e-6);
        assert_eq!(fit.standard_scale, 1000.0);
        near(fit.centre.x, 1000.0, 1e-9);
        near(fit.centre.y, 2000.0, 1e-9);

        // Turned a half turn further it needs the same room upside down, and
        // the answer stays within a quarter turn of north up: 120 - 180 = -60.
        let again = ok(best_fit_rotation(
            &turned_rectangle(Point2::new(0.0, 0.0), 300.0, 100.0, 120.0 * DEGREE),
            A3_ROOM,
        ));
        near(again.rotation, -60.0 * DEGREE, 1e-9);
        near(again.scale, 300000.0 / 385.0, 1e-6);
    }

    #[test]
    fn a_thin_triangle_fits_best_where_its_width_and_height_ask_the_same_scale() {
        // (0, 0), (100, 10), (100, -10) is 100 m long and 20 m high: 1 : 259.7
        // square. Turned by t, its width is 100 cos t + 10 sin t (to the
        // vertex at (100, 10)) and its height 100 sin t + 10 cos t (from the
        // origin down to (100, -10)). Both ask the same scale where
        // 250 (100 c + 10 s) = 385 (100 s + 10 c): tan t = 21150 / 36000 =
        // 0.5875, t = 30.43 degrees, at 1 : 237.1. Mirrored, -30.43 is as
        // good; the positive turn wins the tie.
        let triangle = [
            Point2::new(0.0, 0.0),
            Point2::new(100.0, 10.0),
            Point2::new(100.0, -10.0),
        ];
        let fit = ok(best_fit_rotation(&triangle, A3_ROOM));
        let t: f64 = 0.5875_f64.atan();
        near(fit.rotation, t, 1e-9);
        near(
            fit.scale,
            (100.0 * t.cos() + 10.0 * t.sin()) * 1000.0 / 385.0,
            1e-6,
        );
        assert!(fit.scale < 100.0 * 1000.0 / 385.0);
        assert_eq!(fit.standard_scale, 250.0);
    }

    #[test]
    fn a_line_fits_along_the_rectangles_diagonal() {
        // A line as long as the tiling area's diagonal, sqrt(385^2 + 250^2) =
        // 459.05 m, fills it corner to corner at exactly 1 : 1000, turned by
        // atan(250 / 385) = 33.0 degrees either way; the positive way wins.
        let diagonal = 385.0_f64.hypot(250.0);
        let line = [Point2::new(0.0, 0.0), Point2::new(diagonal, 0.0)];
        let fit = ok(best_fit_rotation(&line, A3_ROOM));
        near(fit.rotation, (250.0_f64 / 385.0).atan(), 1e-9);
        near(fit.scale, 1000.0, 1e-6);
        assert_eq!(fit.standard_scale, 1000.0);
    }

    #[test]
    fn a_rotation_within_a_degree_of_square_is_squared() {
        // Half a degree off east: snapped to 0, at the scale that costs.
        let nearly = ok(best_fit_rotation(
            &turned_rectangle(Point2::new(0.0, 0.0), 300.0, 100.0, 0.5 * DEGREE),
            A3_ROOM,
        ));
        assert_eq!(nearly.rotation, 0.0);
        assert!(nearly.scale > 300000.0 / 385.0);
        // Half a degree off north, either side: snapped to a quarter turn.
        for turn in [89.5, -89.5, 90.4] {
            let fit = ok(best_fit_rotation(
                &turned_rectangle(Point2::new(0.0, 0.0), 300.0, 100.0, turn * DEGREE),
                A3_ROOM,
            ));
            assert_eq!(fit.rotation, PI / 2.0, "{turn}");
        }
        // A square in a square room is as good square as turned a quarter: 0.
        let square = ok(best_fit_rotation(
            &turned_rectangle(Point2::new(5.0, 5.0), 10.0, 10.0, 0.0),
            SizeMm {
                width: 100.0,
                height: 100.0,
            },
        ));
        assert_eq!(square.rotation, 0.0);
        // Two degrees off is not snapped.
        let off = ok(best_fit_rotation(
            &turned_rectangle(Point2::new(0.0, 0.0), 300.0, 100.0, 2.0 * DEGREE),
            A3_ROOM,
        ));
        near(off.rotation, 2.0 * DEGREE, 1e-9);
    }

    #[test]
    fn the_least_rotation_to_fit_is_the_turn_nearest_north_up_that_holds() {
        // The 300 x 100 m rectangle at 30 degrees. Square to the paper its box
        // is 309.8 x 236.6 m: 1 : 946.4 - so 1 : 1000 needs no turn.
        let content = turned_rectangle(Point2::new(0.0, 0.0), 300.0, 100.0, 30.0 * DEGREE);
        let square = ok(least_rotation_to_fit(&content, A3_ROOM, 1000.0));
        assert_eq!(square.rotation, 0.0);

        // 1 : 900 allows 225 m up. Turned by t, the height is twice the corner
        // (150, 50)'s reach up the paper, 50 cos(t - 30) - 150 sin(t - 30):
        // sqrt(25000) cos(t - 30 + atan(150 / 50)) = 112.5, so t = 30 -
        // atan2(150, 50) + acos(112.5 / sqrt(25000)) = 3.08 degrees.
        let turned = ok(least_rotation_to_fit(&content, A3_ROOM, 900.0));
        let t = 30.0 * DEGREE - 150.0_f64.atan2(50.0) + (112.5 / 25000.0_f64.sqrt()).acos();
        near(turned.rotation, t, 1e-9);
        near(turned.scale, 900.0, 1e-6);
        // Nothing nearer north up fits.
        for i in -99..=99 {
            let nearer = ok(fit_at_rotation(&content, A3_ROOM, t * f64::from(i) / 100.0));
            assert!(nearer.scale > 900.0, "{i}");
        }

        // Exactly the best scale fits only at the best rotation; a larger
        // scale fits at none, and says what the best rotation needs.
        let exact = ok(least_rotation_to_fit(&content, A3_ROOM, 300000.0 / 385.0));
        near(exact.rotation, 30.0 * DEGREE, 1e-6);
        let none = refused(least_rotation_to_fit(&content, A3_ROOM, 700.0));
        assert_eq!(none.code, ErrorCode::InvalidArgument);
        assert!(none.message.contains("1:1000"), "{}", none.message);
    }

    #[test]
    fn the_standard_rotation_turns_only_when_that_buys_a_larger_scale() {
        // The 300 x 100 m rectangle at 30 degrees fits 1 : 779 turned, but
        // 1 : 1000 is the standard scale either way, and square it is
        // 1 : 946: it is not turned.
        let rectangle = ok(best_standard_rotation(
            &turned_rectangle(Point2::new(0.0, 0.0), 300.0, 100.0, 30.0 * DEGREE),
            A3_ROOM,
        ));
        assert_eq!(rectangle.rotation, 0.0);
        assert_eq!(rectangle.standard_scale, 1000.0);

        // A 450 x 20 m strip at 45 degrees: square, its box is 332.3 m each
        // way, 1 : 1329, so 1 : 2000; along it, 450000 / 385 = 1 : 1168.8, so
        // 1 : 1250. Turned by t the height is 2 (10 cos a - 225 sin a), a = t
        // - 45, which 1 : 1250 allows up to 312.5: t = 45 - atan2(225, 10) +
        // acos(156.25 / sqrt(225^2 + 10^2)) = 3.62 degrees, the least turn
        // that reaches 1 : 1250.
        let strip = turned_rectangle(Point2::new(0.0, 0.0), 450.0, 20.0, 45.0 * DEGREE);
        let fit = ok(best_standard_rotation(&strip, A3_ROOM));
        let t = 45.0 * DEGREE - 225.0_f64.atan2(10.0) + (156.25 / 225.0_f64.hypot(10.0)).acos();
        near(fit.rotation, t, 1e-9);
        assert_eq!(fit.standard_scale, 1250.0);
        assert!(fit.scale <= 1250.0 + 1e-6);
    }

    #[test]
    fn content_with_no_size_or_a_room_with_none_is_refused() {
        let point = [Point2::new(1.0, 1.0), Point2::new(1.0, 1.0)];
        assert_eq!(
            refused(best_fit_rotation(&point, A3_ROOM)).code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            refused(best_fit_rotation(&[], A3_ROOM)).code,
            ErrorCode::InvalidArgument
        );
        let nan = [Point2::new(0.0, 0.0), Point2::new(f64::NAN, 1.0)];
        assert_eq!(
            refused(fit_at_rotation(&nan, A3_ROOM, 0.0)).code,
            ErrorCode::InvalidArgument
        );
        let line = [Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)];
        assert_eq!(
            refused(best_fit_rotation(
                &line,
                SizeMm {
                    width: 0.0,
                    height: 10.0
                }
            ))
            .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            refused(least_rotation_to_fit(&line, A3_ROOM, 0.0)).code,
            ErrorCode::InvalidArgument
        );
        // A line along an axis has a size: its length.
        let along = ok(fit_at_rotation(
            &line,
            SizeMm {
                width: 100.0,
                height: 50.0,
            },
            0.0,
        ));
        near(along.scale, 100.0, 1e-9);
    }

    #[test]
    fn the_drawn_outline_is_the_hull_of_what_the_view_draws() {
        let mut model = Model::new();
        ok(model.layers.add(Layer {
            name: String::from("far"),
            ..Layer::default()
        }));
        add_to(&mut model, segment(0.0, 0.0, 100.0, 0.0), "0");
        add_to(
            &mut model,
            Geometry::Circle(Circle2::new(Point2::new(50.0, 50.0), 10.0)),
            "0",
        );
        add_to(&mut model, segment(1000.0, 1000.0, 2000.0, 1000.0), "far");

        let mut view = LayerOverrides::default();
        assert!(view.hide("far"));
        let outline = drawn_outline(&model, &view);
        // The line and the circle, traced a 32nd of a turn apart (its top and
        // sides are traced points), and nothing of the hidden layer.
        expect_box(&box_of(&outline), 0.0, 0.0, 100.0, 60.0, 1e-9);
        // Shown, the far layer counts.
        let all = drawn_outline(&model, &LayerOverrides::default());
        assert_eq!(box_of(&all).max, Point2::new(2000.0, 1000.0));
        assert!(drawn_outline(&Model::new(), &view).is_empty());
    }

    #[test]
    fn a_box_is_drawn_in_where_a_plan_draws_something_in_it_not_where_a_bounding_box_reaches() {
        let mut model = Model::new();
        ok(model.layers.add(Layer {
            name: String::from("far"),
            ..Layer::default()
        }));
        ok(model.layers.add(Layer {
            name: String::from(CONSTRUCTION_LAYER),
            ..Layer::default()
        }));
        ok(model.hatch_patterns.add(HatchPattern {
            name: String::from("rock"),
            families: vec![HatchLineFamily {
                angle: 0.0,
                spacing: 1.0,
                offset: 0.0,
            }],
            ..HatchPattern::default()
        }));
        ok(model.layers.add(Layer {
            name: String::from("ground"),
            hatch_pattern: String::from("rock"),
            ..Layer::default()
        }));
        let square = |x0: f64, y0: f64, size: f64| {
            Geometry::Polyline(Polyline2::new(
                vec![
                    Point2::new(x0, y0),
                    Point2::new(x0 + size, y0),
                    Point2::new(x0 + size, y0 + size),
                    Point2::new(x0, y0 + size),
                ],
                true,
            ))
        };
        let all = LayerOverrides::default();

        // A diagonal: a box it crosses is drawn in; one inside its bounding
        // box but below the line is not, and neither is one beyond it, or no
        // box.
        add_to(&mut model, segment(0.0, 0.0, 100.0, 100.0), "0");
        assert!(draws_in(&model, &all, &rect(40.0, 40.0, 60.0, 60.0), None));
        assert!(!draws_in(&model, &all, &rect(60.0, 0.0, 100.0, 30.0), None));
        assert!(!draws_in(
            &model,
            &all,
            &rect(200.0, 200.0, 300.0, 300.0),
            None
        ));
        assert!(!draws_in(&model, &all, &Box2::default(), None));

        // A hatched square is drawn on inside, where a box meets none of its
        // edges; an unhatched one is drawn along its edges alone.
        add_to(&mut model, square(1000.0, 0.0, 1000.0), "ground");
        add_to(&mut model, square(3000.0, 0.0, 1000.0), "0");
        assert!(draws_in(
            &model,
            &all,
            &rect(1400.0, 400.0, 1600.0, 600.0),
            None
        ));
        assert!(!draws_in(
            &model,
            &all,
            &rect(3400.0, 400.0, 3600.0, 600.0),
            None
        ));
        assert!(draws_in(
            &model,
            &all,
            &rect(3900.0, 400.0, 4100.0, 600.0),
            None
        ));

        // A layer the view hides draws nothing there, and a construction line,
        // an aid to drawing, is no part of what a sheet is laid out over.
        add_to(&mut model, segment(5000.0, 0.0, 6000.0, 0.0), "far");
        add_to(
            &mut model,
            segment(5000.0, 500.0, 6000.0, 500.0),
            CONSTRUCTION_LAYER,
        );
        let mut far_hidden = LayerOverrides::default();
        assert!(far_hidden.hide("far"));
        let on_far = rect(5400.0, -10.0, 5600.0, 10.0);
        assert!(draws_in(&model, &all, &on_far, None));
        assert!(!draws_in(&model, &far_hidden, &on_far, None));
        assert!(!draws_in(
            &model,
            &all,
            &rect(5400.0, 490.0, 5600.0, 510.0),
            None
        ));

        // An alignment is drawn, though it is no entity.
        ok(model
            .alignments
            .add(straight_road(7000.0, 0.0, 8000.0, 0.0)));
        assert!(draws_in(
            &model,
            &all,
            &rect(7400.0, -10.0, 7600.0, 10.0),
            None
        ));
        assert!(!draws_in(
            &model,
            &all,
            &rect(7400.0, 50.0, 7600.0, 100.0),
            None
        ));
    }

    #[test]
    fn the_drawn_outline_holds_the_whole_of_a_curve() {
        // A circle of radius 10 is traced by a polygon round it: 32 corners at
        // 10 / cos(5.625 degrees) = 10.0484 out. Its width is never less than
        // the circle's 20 in any direction (the chords alone are 19.90 across
        // at their narrowest) and never more than the corners' 20.0968.
        let mut model = Model::new();
        add_to(
            &mut model,
            Geometry::Circle(Circle2::new(Point2::new(500.0, 500.0), 10.0)),
            "0",
        );
        let outline = drawn_outline(&model, &LayerOverrides::default());
        let corner = 10.0 / (PI / 32.0).cos();
        let unit = SizeMm {
            width: 1.0,
            height: 1.0,
        };
        for degrees in 0..180 {
            let fit = ok(fit_at_rotation(&outline, unit, f64::from(degrees) * DEGREE));
            assert!(fit.width >= 20.0 - 1e-9, "{degrees}");
            assert!(fit.width <= 2.0 * corner + 1e-9, "{degrees}");
        }
        // A quarter arc from east to north is held too, its ends exactly.
        let mut quarter = Model::new();
        add_to(
            &mut quarter,
            Geometry::Arc(Arc2::new(Point2::new(0.0, 0.0), 100.0, 0.0, PI / 2.0)),
            "0",
        );
        let hull = drawn_outline(&quarter, &LayerOverrides::default());
        expect_box(&box_of(&hull), 0.0, 0.0, 100.0, 100.0, 1e-9);
        // Across the diagonal the arc reaches 100 from its centre; the hull
        // does too.
        let diagonal = ok(fit_at_rotation(&hull, unit, PI / 4.0));
        assert!(
            diagonal
                .centre
                .dot(Vec2::new(0.5_f64.sqrt(), 0.5_f64.sqrt()))
                + diagonal.width / 2.0
                >= 100.0 - 1e-9
        );
    }

    #[test]
    fn a_view_shows_its_stretch_of_its_alignment_or_the_drawing() {
        let mut model = Model::new();
        ok(model.alignments.add(straight_road(0.0, 0.0, 1000.0, 0.0)));
        add_to(&mut model, segment(0.0, -50.0, 10.0, 40.0), "0");

        // A strip from CH 100 to 300: that stretch and nothing else.
        let mut strip = viewport_at(
            "vp1",
            ViewportKind::Plan,
            rect(30.0, 40.0, 200.0, 200.0),
            false,
        );
        strip.source.alignment = String::from("ROAD");
        strip.source.chainage_from = 100.0;
        strip.source.chainage_to = 300.0;
        expect_box(
            &box_of(&viewport_content(&model, &strip)),
            100.0,
            0.0,
            300.0,
            0.0,
            1e-9,
        );
        // No range: the whole alignment; a range past its end stops at the
        // end.
        strip.source.chainage_to = 0.0;
        strip.source.chainage_from = 0.0;
        expect_box(
            &box_of(&viewport_content(&model, &strip)),
            0.0,
            0.0,
            1000.0,
            0.0,
            1e-9,
        );
        strip.source.chainage_from = 900.0;
        strip.source.chainage_to = 1500.0;
        expect_box(
            &box_of(&viewport_content(&model, &strip)),
            900.0,
            0.0,
            1000.0,
            0.0,
            1e-9,
        );
        // A range wholly past the end shows none of the alignment: the
        // drawing, as the painter then fits it.
        strip.source.chainage_from = 1200.0;
        strip.source.chainage_to = 1500.0;
        expect_box(
            &box_of(&viewport_content(&model, &strip)),
            0.0,
            -50.0,
            1000.0,
            40.0,
            1e-9,
        );

        // A plan of the drawing: the line and the alignment. An alignment the
        // model does not have leaves the drawing.
        let mut plan = viewport_at(
            "vp2",
            ViewportKind::Plan,
            rect(30.0, 40.0, 200.0, 200.0),
            false,
        );
        expect_box(
            &box_of(&viewport_content(&model, &plan)),
            0.0,
            -50.0,
            1000.0,
            40.0,
            1e-9,
        );
        plan.source.alignment = String::from("NOWHERE");
        expect_box(
            &box_of(&viewport_content(&model, &plan)),
            0.0,
            -50.0,
            1000.0,
            40.0,
            1e-9,
        );

        // A key plan's stored outlines are stale copies the painter no longer
        // draws: alone, it shows the drawing.
        let mut key = viewport_at(
            "vp3",
            ViewportKind::KeyPlan,
            rect(30.0, 40.0, 120.0, 110.0),
            false,
        );
        key.marks.push(WorldMark {
            kind: WorldMarkKind::SheetOutline,
            points: vec![
                Point2::new(-100.0, -100.0),
                Point2::new(1200.0, -100.0),
                Point2::new(1200.0, 300.0),
            ],
            ..WorldMark::default()
        });
        expect_box(
            &box_of(&viewport_content(&model, &key)),
            0.0,
            -50.0,
            1000.0,
            40.0,
            1e-9,
        );
        // In a set, it shows the live outlines of the sheets' plans, as the
        // painter fits it: here one plan, 100 x 50 mm at 1:1000 about
        // (2000, 0).
        let mut far = viewport_at(
            "vp6",
            ViewportKind::Plan,
            rect(0.0, 0.0, 100.0, 50.0),
            false,
        );
        far.scale = 1000.0;
        far.centre = Point2::new(2000.0, 0.0);
        far.auto_scale = false;
        far.auto_centre = false;
        let set = SheetSet {
            sheets: vec![
                Sheet {
                    id: String::from("s1"),
                    viewports: vec![far],
                    ..Sheet::default()
                },
                Sheet {
                    id: String::from("s2"),
                    viewports: vec![key.clone()],
                    ..Sheet::default()
                },
            ],
            ..SheetSet::default()
        };
        expect_box(
            &box_of(&viewport_content_in_set(&model, &set, &key, None)),
            1950.0,
            -25.0,
            2050.0,
            25.0,
            1e-9,
        );

        // Nothing for a view that is not a plan.
        assert!(
            viewport_content(
                &model,
                &viewport_at("vp4", ViewportKind::Legend, Box2::default(), false)
            )
            .is_empty()
        );
        assert!(
            viewport_content(
                &model,
                &viewport_at("vp5", ViewportKind::LongSection, Box2::default(), false)
            )
            .is_empty()
        );
    }

    // ---- Paper and scale ----------------------------------------------------------------

    #[test]
    fn the_smallest_sheet_that_holds_the_content_is_suggested() {
        // A4 landscape: its tiling area is 387 x 252 x 210 / 297, less 2:
        // 271.64 x 176.18 mm. 100 x 60 m at 1 : 500 is 200 x 120 mm: A4.
        let a4 = 210.0 / 297.0;
        let mut request = PaperRequest {
            scale: 500.0,
            ..PaperRequest::default()
        };
        let small = ok(suggest_paper_for_extent(
            &rect(0.0, 0.0, 100.0, 60.0),
            &request,
        ));
        assert_eq!(small.paper, PaperSize::A4);
        assert!(small.landscape);
        assert_eq!(small.frame, BUILT_IN_FRAME_ID);
        expect_box(
            &small.area,
            23.0 * a4 + 1.0,
            35.0 * a4 + 1.0,
            410.0 * a4 - 1.0,
            287.0 * a4 - 1.0,
            1e-9,
        );
        near(small.fill, 200.0 / (387.0 * a4 - 2.0), 1e-9);

        // 200 x 100 m is 400 x 200 mm: wider than A3 landscape (385), A3
        // portrait (297 - 22 = 275) and A4; A2 landscape is 387 x 420 / 297 -
        // 2 = 545.27 wide and 354.36 high.
        let larger = ok(suggest_paper_for_extent(
            &rect(0.0, 0.0, 200.0, 100.0),
            &request,
        ));
        assert_eq!(larger.paper, PaperSize::A2);
        assert!(larger.landscape);

        // 100 x 300 m at 1 : 1000 is 300 mm high: taller than A4 portrait
        // (297 - 22 = 275) and A3 landscape (250); A3 portrait is 275 x 398,
        // and has no frame.
        request.scale = 1000.0;
        let tall = rect(0.0, 0.0, 100.0, 300.0);
        let portrait = ok(suggest_paper_for_extent(&tall, &request));
        assert_eq!(portrait.paper, PaperSize::A3);
        assert!(!portrait.landscape);
        assert!(portrait.frame.is_empty());
        expect_box(&portrait.area, 11.0, 11.0, 286.0, 409.0, 1e-9);

        // Turned a quarter, it runs 300 mm across: A3 landscape.
        request.rotation = PI / 2.0;
        let turned = ok(suggest_paper_for_extent(&tall, &request));
        assert_eq!(turned.paper, PaperSize::A3);
        assert!(turned.landscape);
        assert_eq!(turned.frame, BUILT_IN_FRAME_ID);
    }

    #[test]
    fn a_paper_suggestion_honours_the_frame_and_the_share_of_the_sheet() {
        // 200 x 100 m at 1 : 1000 is 200 x 100 mm: A4 landscape. Beside panels
        // taking 36% of the width it has 0.64 x 271.64 = 173.8 mm on A4 and
        // 0.64 x 188 = 120.3 on A4 portrait: A3 landscape (246.4).
        let mut request = PaperRequest {
            scale: 1000.0,
            ..PaperRequest::default()
        };
        let extent = rect(0.0, 0.0, 200.0, 100.0);
        let whole = ok(suggest_paper_for_extent(&extent, &request));
        assert_eq!(whole.paper, PaperSize::A4);
        request.share_across = 0.64;
        let beside = ok(suggest_paper_for_extent(&extent, &request));
        assert_eq!(beside.paper, PaperSize::A3);
        assert!(beside.landscape);

        // 272 m across is just too wide for A4 landscape's frame (271.64) and
        // just fits without one: 297 - 22 = 275.
        let mut frameless = PaperRequest {
            scale: 1000.0,
            ..PaperRequest::default()
        };
        let wide = rect(0.0, 0.0, 272.0, 100.0);
        let framed = ok(suggest_paper_for_extent(&wide, &frameless));
        assert_eq!(framed.paper, PaperSize::A3);
        frameless.frame.clear();
        let plain = ok(suggest_paper_for_extent(&wide, &frameless));
        assert_eq!(plain.paper, PaperSize::A4);
        assert!(plain.landscape);
        assert!(plain.frame.is_empty());
    }

    #[test]
    fn content_too_large_for_a0_says_the_scale_a0_would_take() {
        // 5 km square at 1 : 500 is 10 m of paper. A0 portrait, frameless,
        // has 819 x 1167 mm: 1 : 6105 would do, so 1 : 10000.
        let mut request = PaperRequest {
            scale: 500.0,
            ..PaperRequest::default()
        };
        let huge = refused(suggest_paper_for_extent(
            &rect(0.0, 0.0, 5000.0, 5000.0),
            &request,
        ));
        assert_eq!(huge.code, ErrorCode::NotFound);
        assert!(huge.message.contains("1:10000"), "{}", huge.message);

        request.scale = 0.0;
        assert_eq!(
            refused(suggest_paper_for_extent(
                &rect(0.0, 0.0, 1.0, 1.0),
                &request
            ))
            .code,
            ErrorCode::InvalidArgument
        );
        request.scale = 500.0;
        request.share_up = 1.5;
        assert_eq!(
            refused(suggest_paper_for_extent(
                &rect(0.0, 0.0, 1.0, 1.0),
                &request
            ))
            .code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn the_suggested_scale_is_the_largest_standard_one_that_fits() {
        // 300 x 100 m on A3: 1 : 779 is needed, so 1 : 1000; turned a quarter
        // it is 100 wide and 300 high, 1 : 1200, so 1 : 1250.
        let content = turned_rectangle(Point2::new(0.0, 0.0), 300.0, 100.0, 0.0);
        let sheet = Sheet::default();
        assert_eq!(ok(suggest_scale_on_sheet(&content, &sheet, 0.0)), 1000.0);
        assert_eq!(
            ok(suggest_scale_on_sheet(&content, &sheet, PI / 2.0)),
            1250.0
        );
        // 100 x 40 m in a 200 x 100 mm viewport: 1 : max(500, 400).
        let small = turned_rectangle(Point2::new(0.0, 0.0), 100.0, 40.0, 0.0);
        assert_eq!(
            ok(suggest_scale(&small, &rect(0.0, 0.0, 200.0, 100.0), 0.0)),
            500.0
        );
        assert!(suggest_scale(&small, &Box2::default(), 0.0).is_err());
    }

    #[test]
    fn applying_paper_keeps_the_layout_in_proportion() {
        // A3 to A1: the tiling area 24..409 x 36..286 becomes the doubled
        // frame's 46..820 x 70..574 inset by 1, 47..819 x 71..573.
        let mut sheet = Sheet {
            viewports: vec![
                viewport_at(
                    "vp1",
                    ViewportKind::Plan,
                    rect(24.0, 36.0, 409.0, 286.0),
                    false,
                ),
                viewport_at(
                    "vp2",
                    ViewportKind::Legend,
                    rect(300.0, 50.0, 400.0, 150.0),
                    false,
                ),
                viewport_at("vp3", ViewportKind::Notes, Box2::default(), false),
            ],
            ..Sheet::default()
        };
        let mut advice = PaperAdvice {
            paper: PaperSize::A1,
            landscape: true,
            frame: String::from(BUILT_IN_FRAME_ID),
            ..PaperAdvice::default()
        };
        let moved = apply_paper(&mut sheet, &advice);
        assert_eq!(moved, ids(&["vp1", "vp2"]));
        assert_eq!(sheet.paper, PaperSize::A1);
        expect_box(&by_id(&sheet, "vp1").rect, 47.0, 71.0, 819.0, 573.0, 1e-9);
        let kx = 772.0 / 385.0;
        let ky = 502.0 / 250.0;
        expect_box(
            &by_id(&sheet, "vp2").rect,
            47.0 + 276.0 * kx,
            71.0 + 14.0 * ky,
            47.0 + 376.0 * kx,
            71.0 + 114.0 * ky,
            1e-9,
        );
        assert!(by_id(&sheet, "vp3").rect.empty());

        // Portrait paper has no frame, whatever the advice names.
        advice.paper = PaperSize::A3;
        advice.landscape = false;
        apply_paper(&mut sheet, &advice);
        assert!(sheet.frame.is_empty());
        expect_box(&by_id(&sheet, "vp1").rect, 11.0, 11.0, 286.0, 409.0, 1e-9);
    }

    #[test]
    fn the_paper_for_a_scale_holds_the_main_view_in_the_share_it_takes() {
        // 500 x 300 m at 1 : 1000, with the painter's 4% to spare, is 520 x
        // 312 mm. Filling the sheet it needs A2 landscape: 387 x 420 / 297 -
        // 2 = 545.27 mm across (A3 either way round and A4 are too small).
        let content = turned_rectangle(Point2::new(1000.0, 2000.0), 500.0, 300.0, 0.0);
        let mut sheet = Sheet::default();
        let mut plan = viewport_at("vp1", ViewportKind::Plan, tiling_area(&sheet), false);
        plan.auto_scale = true;
        plan.auto_centre = true;
        sheet.viewports.push(plan);
        let change = ok(fit_paper_to_viewport(&mut sheet, "vp1", &content, 1000.0));
        assert_eq!(change.advice.paper, PaperSize::A2);
        assert!(change.advice.landscape);
        near(
            change.advice.fill,
            520.0 / (387.0 * 420.0 / 297.0 - 2.0),
            1e-9,
        );
        assert_eq!(change.moved, ids(&["vp1"]));
        assert_eq!(sheet.paper, PaperSize::A2);
        assert_eq!(sheet.frame, BUILT_IN_FRAME_ID);
        let fitted = by_id(&sheet, "vp1");
        assert_eq!(fitted.rect, tiling_area(&sheet));
        // The scale asked for is kept, and the view centred on what it shows.
        assert_eq!(fitted.scale, 1000.0);
        assert!(!fitted.auto_scale);
        assert!(!fitted.auto_centre);
        near(fitted.centre.x, 1000.0, 1e-9);
        near(fitted.centre.y, 2000.0, 1e-9);

        // Beside a panel it has only its 251.1 of the 385 mm across: 520 mm
        // then needs 797 of tiling width, more than A1's 772 (so A0), and the
        // legend keeps its column.
        let mut beside = Sheet::default();
        let cells = preset_cells(
            TilingPreset::MainRight,
            &tiling_area(&beside),
            TILING_GUTTER_MM,
        );
        beside
            .viewports
            .push(viewport_at("vp1", ViewportKind::Plan, cells[0], false));
        beside
            .viewports
            .push(viewport_at("vp2", ViewportKind::Legend, cells[1], false));
        let wide = ok(fit_paper_to_viewport(&mut beside, "vp1", &content, 1000.0));
        assert_eq!(wide.advice.paper, PaperSize::A0);
        assert!(wide.advice.landscape);
        assert_eq!(wide.moved, ids(&["vp1", "vp2"]));
        let area = tiling_area(&beside);
        near(
            by_id(&beside, "vp1").rect.width() / area.width(),
            251.1 / 385.0,
            1e-9,
        );
        near(
            by_id(&beside, "vp2").rect.max.x,
            area.max.x - 1.5 * area.width() / 385.0,
            1e-9,
        );

        // A panel has no scale to choose paper for; an unknown id is not
        // found.
        assert_eq!(
            refused(fit_paper_to_viewport(&mut beside, "vp2", &content, 1000.0)).code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            refused(fit_paper_to_viewport(&mut beside, "vp9", &content, 1000.0)).code,
            ErrorCode::NotFound
        );
        assert_eq!(
            refused(fit_paper_to_viewport(&mut beside, "vp1", &content, 0.0)).code,
            ErrorCode::InvalidArgument
        );
        // Too big for A0: refused, and the sheet is left as it was.
        let before = beside.clone();
        assert_eq!(
            refused(fit_paper_to_viewport(&mut beside, "vp1", &content, 200.0)).code,
            ErrorCode::NotFound
        );
        assert_eq!(beside, before);
    }

    // ---- auto_arrange --------------------------------------------------------------------

    #[test]
    fn auto_arrange_leaves_a_tidy_sheet_alone() {
        let mut sheet = Sheet {
            viewports: vec![
                viewport_at("vp1", ViewportKind::Plan, Box2::default(), false),
                viewport_at("vp2", ViewportKind::Model3D, Box2::default(), false),
                viewport_at("vp3", ViewportKind::Legend, Box2::default(), false),
            ],
            ..Sheet::default()
        };
        tile_viewports(&mut sheet, TilingPreset::MainTwoRight, TILING_GUTTER_MM);
        let tiled = sheet.clone();
        let result = auto_arrange(&mut sheet, TILING_GUTTER_MM);
        assert_eq!(result, ArrangeResult::default());
        assert_eq!(sheet, tiled);
    }

    #[test]
    fn panels_over_the_main_view_are_moved_beside_it_and_locked_ones_stay() {
        // The plan in "Main and panel right"'s main cell, 25.5..276.6 x
        // 37.5..284.5; a locked note at the bottom of the right column; a key
        // plan and a legend dropped on the plan. They go, one gutter clear of
        // everything, highest first: the key plan (rank 4) at the top of the
        // column, 279.6 = 276.6 + 3 from the left, and the legend under it,
        // 3 mm below its 214.5.
        let mut sheet = Sheet::default();
        let main = preset_cells(
            TilingPreset::MainRight,
            &tiling_area(&sheet),
            TILING_GUTTER_MM,
        )[0];
        expect_box(&main, 25.5, 37.5, 276.6, 284.5, 1e-9);
        sheet
            .viewports
            .push(viewport_at("vp1", ViewportKind::Plan, main, false));
        sheet.viewports.push(viewport_at(
            "vp2",
            ViewportKind::Legend,
            rect(100.0, 100.0, 180.0, 200.0),
            false,
        ));
        sheet.viewports.push(viewport_at(
            "vp3",
            ViewportKind::Notes,
            rect(300.0, 40.0, 400.0, 90.0),
            true,
        ));
        sheet.viewports.push(viewport_at(
            "vp4",
            ViewportKind::KeyPlan,
            rect(120.0, 120.0, 210.0, 190.0),
            false,
        ));

        let result = auto_arrange(&mut sheet, TILING_GUTTER_MM);
        assert_eq!(result.moved, ids(&["vp2", "vp4"]));
        assert!(result.overlapping.is_empty());
        assert!(result.unplaced.is_empty());
        assert!(!result.main_shrunk);
        expect_tidy(&sheet);
        assert_eq!(by_id(&sheet, "vp1").rect, main);
        assert_eq!(by_id(&sheet, "vp3").rect, rect(300.0, 40.0, 400.0, 90.0));
        expect_box(&by_id(&sheet, "vp4").rect, 279.6, 214.5, 369.6, 284.5, 1e-9);
        expect_box(&by_id(&sheet, "vp2").rect, 279.6, 111.5, 359.6, 211.5, 1e-9);

        // Arranged again, nothing moves.
        let arranged = sheet.clone();
        assert!(auto_arrange(&mut sheet, TILING_GUTTER_MM).moved.is_empty());
        assert_eq!(sheet, arranged);
    }

    #[test]
    fn the_main_view_is_the_lowest_rank_wherever_it_is_on_the_sheet() {
        // A long section first on the sheet, over a plan: the plan (rank 0)
        // is the main view and stays; the section gives way.
        let plan = rect(25.5, 100.0, 300.0, 284.5);
        let mut sheet = Sheet {
            viewports: vec![
                viewport_at(
                    "vp1",
                    ViewportKind::LongSection,
                    rect(25.5, 37.5, 300.0, 150.0),
                    false,
                ),
                viewport_at("vp2", ViewportKind::Plan, plan, false),
            ],
            ..Sheet::default()
        };
        let result = auto_arrange(&mut sheet, TILING_GUTTER_MM);
        assert_eq!(result.moved, ids(&["vp1"]));
        assert_eq!(by_id(&sheet, "vp2").rect, plan);
        expect_tidy(&sheet);
        assert!(area_of(&by_id(&sheet, "vp2").rect) > area_of(&by_id(&sheet, "vp1").rect));
        // Never below a long section's 70 x 45 mm.
        assert!(by_id(&sheet, "vp1").rect.width() >= 70.0 - 1e-9);
        assert!(by_id(&sheet, "vp1").rect.height() >= 45.0 - 1e-9);
    }

    #[test]
    fn unplaced_views_take_the_cells_tiling_would_give_them() {
        // A plan and a legend with no place yet: "Main and panel right", 66%
        // and 34%, packed left to right: exactly the tiled cells.
        let mut sheet = Sheet {
            viewports: vec![
                viewport_at("vp1", ViewportKind::Legend, Box2::default(), false),
                viewport_at("vp2", ViewportKind::Plan, Box2::default(), false),
            ],
            ..Sheet::default()
        };
        let result = auto_arrange(&mut sheet, TILING_GUTTER_MM);
        assert_eq!(result.moved, ids(&["vp1", "vp2"]));
        let cells = preset_cells(
            TilingPreset::MainRight,
            &tiling_area(&sheet),
            TILING_GUTTER_MM,
        );
        expect_box(
            &by_id(&sheet, "vp2").rect,
            cells[0].min.x,
            cells[0].min.y,
            cells[0].max.x,
            cells[0].max.y,
            1e-9,
        );
        expect_box(
            &by_id(&sheet, "vp1").rect,
            cells[1].min.x,
            cells[1].min.y,
            cells[1].max.x,
            cells[1].max.y,
            1e-9,
        );
    }

    #[test]
    fn a_main_view_under_a_locked_one_is_moved_off_it_at_its_full_size() {
        // The plan sits under a locked note, with a legend over it. The plan
        // is moved off the note at its full size, not shrunk with the panels,
        // and stays the largest view; the note does not move.
        let note = rect(30.0, 40.0, 80.0, 70.0);
        let mut sheet = Sheet {
            viewports: vec![
                viewport_at(
                    "vp1",
                    ViewportKind::Plan,
                    rect(25.5, 37.5, 200.0, 180.0),
                    false,
                ),
                viewport_at("vp2", ViewportKind::Notes, note, true),
                viewport_at(
                    "vp3",
                    ViewportKind::Legend,
                    rect(90.0, 90.0, 380.0, 280.0),
                    false,
                ),
            ],
            ..Sheet::default()
        };
        let result = auto_arrange(&mut sheet, TILING_GUTTER_MM);
        expect_tidy(&sheet);
        assert!(result.overlapping.is_empty());
        assert_eq!(by_id(&sheet, "vp2").rect, note);
        near(by_id(&sheet, "vp1").rect.width(), 174.5, 1e-9);
        near(by_id(&sheet, "vp1").rect.height(), 142.5, 1e-9);
        assert!(area_of(&by_id(&sheet, "vp1").rect) >= area_of(&by_id(&sheet, "vp3").rect));
        assert!(!result.main_shrunk);
    }

    #[test]
    fn when_nothing_else_makes_room_the_main_view_shrinks_from_its_top_left_corner() {
        // The plan fills the sheet and a legend sits on it: no room anywhere
        // until the plan is 90% of its 382 x 247 mm, which leaves a column
        // 407.5 - (25.5 + 343.8 + 3) = 35.2 mm wide on the right.
        let blank = Sheet::default();
        let full = preset_cells(TilingPreset::Full, &tiling_area(&blank), TILING_GUTTER_MM)[0];
        let mut sheet = Sheet {
            viewports: vec![
                viewport_at("vp1", ViewportKind::Plan, full, false),
                viewport_at(
                    "vp2",
                    ViewportKind::Legend,
                    rect(100.0, 100.0, 180.0, 200.0),
                    false,
                ),
            ],
            ..Sheet::default()
        };
        let result = auto_arrange(&mut sheet, TILING_GUTTER_MM);
        assert!(result.main_shrunk);
        assert!(result.overlapping.is_empty());
        expect_tidy(&sheet);
        let plan = by_id(&sheet, "vp1").rect;
        assert_eq!(plan.min.x, full.min.x);
        assert_eq!(plan.max.y, full.max.y);
        near(plan.width(), 0.9 * full.width(), 1e-9);
        near(plan.height(), 0.9 * full.height(), 1e-9);
        let legend = by_id(&sheet, "vp2").rect;
        assert!(legend.width() <= 35.2 + 1e-9);
        assert!(legend.width() >= 16.0);
        assert!(area_of(&plan) > area_of(&legend));
    }

    #[test]
    fn a_stray_view_is_brought_back_onto_the_sheet() {
        let mut sheet = Sheet {
            viewports: vec![viewport_at(
                "vp1",
                ViewportKind::Legend,
                rect(500.0, 100.0, 580.0, 160.0),
                false,
            )],
            ..Sheet::default()
        };
        let result = auto_arrange(&mut sheet, TILING_GUTTER_MM);
        assert_eq!(result.moved, ids(&["vp1"]));
        // Against the right of the tiling area less half a gutter: 407.5.
        expect_box(&by_id(&sheet, "vp1").rect, 327.5, 100.0, 407.5, 160.0, 1e-9);
    }

    #[test]
    fn a_crowded_sheet_is_arranged_the_same_way_every_time_and_never_below_the_minimum() {
        // A plan and seven panels all dropped in one spot on A4.
        let mut sheet = Sheet {
            paper: PaperSize::A4,
            ..Sheet::default()
        };
        let spot = rect(60.0, 60.0, 160.0, 140.0);
        sheet.viewports.push(viewport_at(
            "vp1",
            ViewportKind::Plan,
            rect(20.0, 30.0, 250.0, 180.0),
            false,
        ));
        let panels = [
            ViewportKind::Legend,
            ViewportKind::KeyPlan,
            ViewportKind::Notes,
            ViewportKind::Image,
            ViewportKind::Model3D,
            ViewportKind::Legend,
            ViewportKind::Notes,
        ];
        for (n, kind) in (2..).zip(panels) {
            sheet
                .viewports
                .push(viewport_at(&format!("vp{n}"), kind, spot, false));
        }
        let mut again = sheet.clone();
        let result = auto_arrange(&mut sheet, TILING_GUTTER_MM);
        assert_eq!(auto_arrange(&mut again, TILING_GUTTER_MM), result);
        assert_eq!(again, sheet);
        assert!(result.overlapping.is_empty());
        expect_tidy(&sheet);
        for viewport in &sheet.viewports {
            let least = minimum_size(viewport.kind);
            assert!(
                viewport.rect.width() >= least.width - 1e-9,
                "{}",
                viewport.id
            );
            assert!(
                viewport.rect.height() >= least.height - 1e-9,
                "{}",
                viewport.id
            );
            if viewport.id != "vp1" {
                assert!(
                    area_of(&viewport.rect) <= area_of(&by_id(&sheet, "vp1").rect),
                    "{}",
                    viewport.id
                );
            }
        }
        // And a sheet arranged completely stays as it is.
        let arranged = sheet.clone();
        assert!(auto_arrange(&mut sheet, TILING_GUTTER_MM).moved.is_empty());
        assert_eq!(sheet, arranged);
    }

    #[test]
    fn a_panel_larger_than_the_main_view_gives_way_to_it() {
        // A 100 x 100 mm plan in the top-left corner and a 250 x 240 mm legend
        // beside it, overlapping nothing: the legend is the larger, so it is
        // packed again, no larger than the plan.
        let plan = rect(25.5, 184.5, 125.5, 284.5);
        let mut sheet = Sheet {
            viewports: vec![
                viewport_at("vp1", ViewportKind::Plan, plan, false),
                viewport_at(
                    "vp2",
                    ViewportKind::Legend,
                    rect(150.0, 40.0, 400.0, 280.0),
                    false,
                ),
            ],
            ..Sheet::default()
        };
        let result = auto_arrange(&mut sheet, TILING_GUTTER_MM);
        assert_eq!(result.moved, ids(&["vp2"]));
        assert_eq!(by_id(&sheet, "vp1").rect, plan);
        expect_tidy(&sheet);
        assert!(area_of(&by_id(&sheet, "vp2").rect) <= area_of(&plan) + 1e-9);
        // A legend no larger than the plan stays where it is.
        let legend = rect(150.0, 40.0, 230.0, 140.0);
        let mut small = Sheet {
            viewports: vec![
                viewport_at("vp1", ViewportKind::Plan, plan, false),
                viewport_at("vp2", ViewportKind::Legend, legend, false),
            ],
            ..Sheet::default()
        };
        assert!(auto_arrange(&mut small, TILING_GUTTER_MM).moved.is_empty());
        assert_eq!(by_id(&small, "vp2").rect, legend);
    }

    #[test]
    fn an_unplaced_main_view_is_packed_first_and_stays_the_largest() {
        // The plan has no place yet and a large 3D view fills most of the
        // sheet: the plan takes the first cell, and the 3D view is made no
        // larger.
        let mut sheet = Sheet {
            viewports: vec![
                viewport_at(
                    "vp1",
                    ViewportKind::Model3D,
                    rect(30.0, 40.0, 400.0, 280.0),
                    false,
                ),
                viewport_at("vp2", ViewportKind::Plan, Box2::default(), false),
            ],
            ..Sheet::default()
        };
        let result = auto_arrange(&mut sheet, TILING_GUTTER_MM);
        assert!(result.unplaced.is_empty());
        assert!(result.overlapping.is_empty());
        expect_tidy(&sheet);
        assert!(!by_id(&sheet, "vp2").rect.empty());
        assert!(area_of(&by_id(&sheet, "vp2").rect) >= area_of(&by_id(&sheet, "vp1").rect) - 1e-9);
    }

    #[test]
    fn a_view_in_the_tiling_areas_margin_is_brought_inside_it() {
        // Inside the drawing area (23..410) but over the tiling area's inset:
        // it is moved to 25.5, half a gutter inside the tiling area's 24.
        let mut sheet = Sheet {
            viewports: vec![viewport_at(
                "vp1",
                ViewportKind::Legend,
                rect(23.5, 100.0, 103.5, 160.0),
                false,
            )],
            ..Sheet::default()
        };
        let result = auto_arrange(&mut sheet, TILING_GUTTER_MM);
        assert_eq!(result.moved, ids(&["vp1"]));
        expect_box(&by_id(&sheet, "vp1").rect, 25.5, 100.0, 105.5, 160.0, 1e-9);
    }

    #[test]
    fn a_view_as_large_as_the_space_is_brought_in_without_failing() {
        // On A2 and A4 the packing space's width less a view's own can round
        // to just below its left edge; the view is brought in, not refused.
        for paper in [PaperSize::A4, PaperSize::A2] {
            let mut sheet = Sheet {
                paper,
                ..Sheet::default()
            };
            let space = tiling_area(&sheet).inflated(-TILING_GUTTER_MM / 2.0);
            let wide = Box2::new(
                Point2::new(space.min.x + 40.0, space.min.y - 30.0),
                Point2::new(space.max.x + 40.0, space.max.y - 30.0),
            );
            sheet
                .viewports
                .push(viewport_at("vp1", ViewportKind::Plan, wide, false));
            auto_arrange(&mut sheet, TILING_GUTTER_MM);
            expect_tidy(&sheet);
            near(by_id(&sheet, "vp1").rect.width(), space.width(), 1e-9);
            near(by_id(&sheet, "vp1").rect.min.x, space.min.x, 1e-9);
        }
    }

    #[test]
    fn a_view_with_no_room_at_all_is_reported_and_left_where_it_was() {
        // A locked panel covers the whole tiling area; the plan under it has
        // nowhere to go.
        let blank = Sheet::default();
        let area = tiling_area(&blank);
        let plan = rect(50.0, 50.0, 150.0, 150.0);
        let mut sheet = Sheet {
            viewports: vec![
                viewport_at("vp1", ViewportKind::Image, area, true),
                viewport_at("vp2", ViewportKind::Plan, plan, false),
                viewport_at("vp3", ViewportKind::Legend, Box2::default(), false),
            ],
            ..Sheet::default()
        };
        let result = auto_arrange(&mut sheet, TILING_GUTTER_MM);
        assert!(result.moved.is_empty());
        assert_eq!(result.overlapping, ids(&["vp2"]));
        assert_eq!(result.unplaced, ids(&["vp3"]));
        assert_eq!(by_id(&sheet, "vp2").rect, plan);
        assert_eq!(by_id(&sheet, "vp1").rect, area);
    }

    // ---- Align, distribute, match, fit --------------------------------------------------

    #[test]
    fn views_align_on_their_outermost_edge_or_the_middle_of_their_box() {
        // The box round them is 30..260 x 40..120, its middle (145, 80).
        let cases: [(AlignEdge, &[&str], Box2, Box2, Box2); 6] = [
            (
                AlignEdge::Left,
                &["b", "c"],
                rect(30.0, 40.0, 80.0, 80.0),
                rect(30.0, 50.0, 60.0, 120.0),
                rect(30.0, 45.0, 90.0, 60.0),
            ),
            (
                AlignEdge::Right,
                &["a", "b"],
                rect(210.0, 40.0, 260.0, 80.0),
                rect(230.0, 50.0, 260.0, 120.0),
                rect(200.0, 45.0, 260.0, 60.0),
            ),
            (
                AlignEdge::Top,
                &["a", "c"],
                rect(30.0, 80.0, 80.0, 120.0),
                rect(100.0, 50.0, 130.0, 120.0),
                rect(200.0, 105.0, 260.0, 120.0),
            ),
            (
                AlignEdge::Bottom,
                &["b", "c"],
                rect(30.0, 40.0, 80.0, 80.0),
                rect(100.0, 40.0, 130.0, 110.0),
                rect(200.0, 40.0, 260.0, 55.0),
            ),
            (
                AlignEdge::HorizontalCentre,
                &["a", "b", "c"],
                rect(120.0, 40.0, 170.0, 80.0),
                rect(130.0, 50.0, 160.0, 120.0),
                rect(115.0, 45.0, 175.0, 60.0),
            ),
            (
                AlignEdge::VerticalCentre,
                &["a", "b", "c"],
                rect(30.0, 60.0, 80.0, 100.0),
                rect(100.0, 45.0, 130.0, 115.0),
                rect(200.0, 72.5, 260.0, 87.5),
            ),
        ];
        for (edge, moved, a, b, c) in cases {
            let mut sheet = three_views();
            let result = ok(align_viewports(&mut sheet, &abc(), edge));
            assert_eq!(result, ids(moved), "{}", edge.name());
            assert_eq!(by_id(&sheet, "a").rect, a, "{}", edge.name());
            assert_eq!(by_id(&sheet, "b").rect, b, "{}", edge.name());
            assert_eq!(by_id(&sheet, "c").rect, c, "{}", edge.name());
            assert_eq!(align_edge_from(edge.name()), Some(edge));
        }
        assert!(align_edge_from("middle").is_none());
    }

    #[test]
    fn a_locked_view_counts_where_it_is_and_one_view_aligns_to_the_tiling_area() {
        let mut sheet = three_views();
        sheet.viewports[0].locked = true;
        // Right: the locked "a" does not move; the others meet the box's
        // right.
        let moved = ok(align_viewports(&mut sheet, &abc(), AlignEdge::Right));
        assert_eq!(moved, ids(&["b"]));
        assert_eq!(by_id(&sheet, "a").rect, three_views().viewports[0].rect);

        // One alone goes to the tiling area's edge, 24..409 x 36..286.
        let mut one = three_views();
        let b = ids(&["b"]);
        ok(align_viewports(&mut one, &b, AlignEdge::Top));
        assert_eq!(by_id(&one, "b").rect, rect(100.0, 216.0, 130.0, 286.0));
        ok(align_viewports(&mut one, &b, AlignEdge::HorizontalCentre));
        assert_eq!(by_id(&one, "b").rect.center().x, 216.5);

        // An unknown id is NotFound; views none of which is placed are
        // refused.
        let unknown = ids(&["a", "zz"]);
        assert_eq!(
            refused(align_viewports(&mut one, &unknown, AlignEdge::Left)).code,
            ErrorCode::NotFound
        );
        let mut unplaced = Sheet {
            viewports: vec![viewport_at(
                "x",
                ViewportKind::Legend,
                Box2::default(),
                false,
            )],
            ..Sheet::default()
        };
        assert_eq!(
            refused(align_viewports(
                &mut unplaced,
                &ids(&["x"]),
                AlignEdge::Left
            ))
            .code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn distributing_leaves_equal_gaps_between_the_first_and_the_last() {
        // Left edges 10, 40, 150, 300; the first ends at 30 and the last
        // starts at 300, and the middle two are 60 + 20 wide: (270 - 80) / 3 =
        // 63.33 between each.
        let mut sheet = Sheet {
            viewports: vec![
                viewport_at(
                    "d",
                    ViewportKind::Legend,
                    rect(300.0, 50.0, 320.0, 60.0),
                    false,
                ),
                viewport_at(
                    "a",
                    ViewportKind::Legend,
                    rect(10.0, 50.0, 30.0, 60.0),
                    false,
                ),
                viewport_at(
                    "c",
                    ViewportKind::Legend,
                    rect(150.0, 90.0, 170.0, 100.0),
                    false,
                ),
                viewport_at(
                    "b",
                    ViewportKind::Legend,
                    rect(40.0, 70.0, 100.0, 80.0),
                    false,
                ),
            ],
            ..Sheet::default()
        };
        let mut reversed = sheet.clone();
        let moved = ok(distribute_viewports(
            &mut sheet,
            &ids(&["a", "b", "c", "d"]),
            DistributeAxis::Horizontal,
        ));
        assert_eq!(moved, ids(&["c", "b"]));
        let gap = 190.0 / 3.0;
        near(by_id(&sheet, "b").rect.min.x, 30.0 + gap, 1e-9);
        near(by_id(&sheet, "c").rect.min.x, 90.0 + 2.0 * gap, 1e-9);
        assert_eq!(by_id(&sheet, "a").rect.min.x, 10.0);
        assert_eq!(by_id(&sheet, "d").rect.min.x, 300.0);
        assert_eq!(
            by_id(&sheet, "c").rect.min.y,
            90.0,
            "a horizontal spacing leaves the heights"
        );
        // The order the ids come in does not matter.
        ok(distribute_viewports(
            &mut reversed,
            &ids(&["d", "c", "b", "a"]),
            DistributeAxis::Horizontal,
        ));
        assert_eq!(reversed, sheet);

        assert_eq!(
            distribute_axis_from("vertical"),
            Some(DistributeAxis::Vertical)
        );
        assert_eq!(
            distribute_axis_from(DistributeAxis::Horizontal.name()),
            Some(DistributeAxis::Horizontal)
        );
    }

    #[test]
    fn distributing_needs_three_views_that_can_move() {
        let mut sheet = three_views();
        sheet.viewports[1].locked = true;
        let locked = refused(distribute_viewports(
            &mut sheet,
            &abc(),
            DistributeAxis::Vertical,
        ));
        assert_eq!(locked.code, ErrorCode::InvalidArgument);
        assert!(locked.message.contains("there are 2"));
        assert_eq!(
            refused(distribute_viewports(
                &mut sheet,
                &ids(&["a", "q"]),
                DistributeAxis::Vertical
            ))
            .code,
            ErrorCode::NotFound
        );

        // Vertically: bottoms 40 (a, ends 80), 45 (c, 15 high) and 50 (b,
        // starts 50 ends 120): c's 15 mm would have to fit between 80 and 50,
        // a gap of (50 - 80 - 15) / 2 = -22.5 mm that would lay the views over
        // each other. Refused, and nothing moves.
        let mut free = three_views();
        let unmoved = free.clone();
        let moved = refused(distribute_viewports(
            &mut free,
            &abc(),
            DistributeAxis::Vertical,
        ));
        assert_eq!(moved.code, ErrorCode::InvalidArgument);
        assert!(
            moved
                .message
                .contains("15.0 mm high together, and there is -30.0 mm"),
            "{}",
            moved.message
        );
        assert_eq!(free, unmoved);
    }

    #[test]
    fn matching_a_scale_copies_it_to_every_scaled_view_on_any_sheet() {
        let mut plan = viewport_at(
            "vp1",
            ViewportKind::Plan,
            rect(30.0, 40.0, 200.0, 200.0),
            false,
        );
        plan.scale = 500.0;
        let legend = viewport_at(
            "vp4",
            ViewportKind::Legend,
            rect(300.0, 40.0, 380.0, 100.0),
            false,
        );
        let mut long_section = viewport_at(
            "vp2",
            ViewportKind::LongSection,
            rect(30.0, 40.0, 400.0, 150.0),
            false,
        );
        long_section.scale = 1000.0;
        long_section.vertical_exaggeration = 10.0;
        long_section.auto_scale = true;
        let mut cross_sections = viewport_at(
            "vp3",
            ViewportKind::CrossSections,
            rect(30.0, 160.0, 200.0, 280.0),
            false,
        );
        cross_sections.scale = 200.0;
        cross_sections.vertical_exaggeration = 5.0;
        let set = SheetSet {
            sheets: vec![
                Sheet {
                    id: String::from("s1"),
                    viewports: vec![plan, legend.clone()],
                    ..Sheet::default()
                },
                Sheet {
                    id: String::from("s2"),
                    viewports: vec![long_section, cross_sections],
                    ..Sheet::default()
                },
            ],
            ..SheetSet::default()
        };

        let mut from_plan = set.clone();
        let targets = ids(&["vp2", "vp3", "vp4"]);
        let changed = ok(match_scale(&mut from_plan, &targets, "vp1", None, None));
        assert_eq!(changed, ids(&["vp2", "vp3"]));
        assert_eq!(from_plan.sheets[1].viewports[0].scale, 500.0);
        assert!(!from_plan.sheets[1].viewports[0].auto_scale);
        assert_eq!(
            from_plan.sheets[1].viewports[0].vertical_exaggeration, 10.0,
            "a plan has none to give"
        );
        assert_eq!(from_plan.sheets[1].viewports[1].scale, 500.0);
        assert_eq!(from_plan.sheets[0].viewports[1], legend);

        // Section to section: the exaggeration too.
        let mut from_section = set.clone();
        ok(match_scale(
            &mut from_section,
            &ids(&["vp3"]),
            "vp2",
            None,
            None,
        ));
        assert_eq!(from_section.sheets[1].viewports[1].scale, 1000.0);
        assert_eq!(
            from_section.sheets[1].viewports[1].vertical_exaggeration,
            10.0
        );

        // An automatic scale is passed as drawn.
        let mut drawn = set.clone();
        ok(match_scale(
            &mut drawn,
            &ids(&["vp1"]),
            "vp2",
            Some(750.0),
            None,
        ));
        assert_eq!(drawn.sheets[0].viewports[0].scale, 750.0);
        // An automatic section's exaggeration is passed as drawn too, not its
        // stale stored one.
        let mut fitted = set.clone();
        ok(match_scale(
            &mut fitted,
            &ids(&["vp3"]),
            "vp2",
            Some(750.0),
            Some(5.0),
        ));
        assert_eq!(fitted.sheets[1].viewports[1].scale, 750.0);
        assert_eq!(fitted.sheets[1].viewports[1].vertical_exaggeration, 5.0);

        let mut refused_set = set.clone();
        assert_eq!(
            refused(match_scale(&mut refused_set, &targets, "vp9", None, None)).code,
            ErrorCode::NotFound
        );
        assert_eq!(
            refused(match_scale(
                &mut refused_set,
                &ids(&["vp9"]),
                "vp1",
                None,
                None
            ))
            .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            refused(match_scale(&mut refused_set, &targets, "vp4", None, None)).code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            refused(match_scale(
                &mut refused_set,
                &targets,
                "vp1",
                Some(0.0),
                None
            ))
            .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(refused_set, set);
    }

    #[test]
    fn a_view_is_sized_to_its_content_about_its_centre() {
        // 100 x 50 m at 1 : 500 is 200 x 100 mm, and 5% more: 210 x 105 about
        // the old centre (150, 150).
        let mut plan = viewport_at(
            "vp1",
            ViewportKind::Plan,
            rect(100.0, 100.0, 200.0, 200.0),
            false,
        );
        plan.scale = 500.0;
        plan.auto_centre = true;
        let content = turned_rectangle(Point2::new(1000.0, 2000.0), 100.0, 50.0, 0.0);
        let area = drawing_area(&Sheet::default());
        ok(fit_viewport_to_content(&mut plan, &content, &area));
        expect_box(&plan.rect, 45.0, 97.5, 255.0, 202.5, 1e-9);
        near(plan.centre.x, 1000.0, 1e-9);
        near(plan.centre.y, 2000.0, 1e-9);
        assert!(
            !plan.auto_centre,
            "the centre is set, so it is no longer automatic"
        );
        assert!(!plan.auto_scale);
        assert_eq!(plan.scale, 500.0);

        // Turned a quarter, the 50 m runs across.
        plan.rotation = PI / 2.0;
        ok(fit_viewport_to_content(&mut plan, &content, &area));
        expect_box(&plan.rect, 97.5, 45.0, 202.5, 255.0, 1e-9);

        // Too big for the paper: the drawing area. Too small: the kind's
        // minimum.
        plan.rotation = 0.0;
        ok(fit_viewport_to_content(
            &mut plan,
            &turned_rectangle(Point2::new(0.0, 0.0), 1000.0, 1000.0, 0.0),
            &area,
        ));
        assert_eq!(plan.rect, area);
        ok(fit_viewport_to_content(
            &mut plan,
            &turned_rectangle(Point2::new(0.0, 0.0), 1.0, 1.0, 0.0),
            &area,
        ));
        near(plan.rect.width(), 35.0, 1e-9);
        near(plan.rect.height(), 30.0, 1e-9);

        // A long section's levels are exaggerated: 5 m at 1 : 500 x 10 =
        // 100 mm.
        let mut section = viewport_at(
            "vp2",
            ViewportKind::LongSection,
            rect(100.0, 100.0, 300.0, 200.0),
            false,
        );
        section.scale = 500.0;
        section.vertical_exaggeration = 10.0;
        let profile = [Point2::new(0.0, 10.0), Point2::new(100.0, 15.0)];
        ok(fit_viewport_to_content(&mut section, &profile, &area));
        expect_box(&section.rect, 95.0, 97.5, 305.0, 202.5, 1e-9);
        near(section.centre.x, 50.0, 1e-9);
        near(section.centre.y, 12.5, 1e-9);

        let mut legend = viewport_at(
            "vp3",
            ViewportKind::Legend,
            rect(0.0, 0.0, 10.0, 10.0),
            false,
        );
        assert_eq!(
            refused(fit_viewport_to_content(&mut legend, &content, &area)).code,
            ErrorCode::InvalidArgument
        );
        let mut unplaced = viewport_at("vp4", ViewportKind::Plan, Box2::default(), false);
        assert_eq!(
            refused(fit_viewport_to_content(&mut unplaced, &content, &area)).code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn rotating_a_plan_to_best_fit_turns_it_only_as_far_as_the_scale_needs() {
        // The 450 x 20 m strip at 45 degrees in the whole tiling area, less
        // the painter's 4%: 370.2 x 240.4 mm. Along it 1 : 1215.6, so
        // 1 : 1250; square to the paper 1 : 1382.
        let mut plan = viewport_at(
            "vp1",
            ViewportKind::Plan,
            tiling_area(&Sheet::default()),
            false,
        );
        plan.scale = 2000.0;
        plan.auto_scale = true;
        plan.auto_centre = true;
        let strip = turned_rectangle(Point2::new(5000.0, 7000.0), 450.0, 20.0, 45.0 * DEGREE);
        let chosen = ok(rotate_to_best_fit(&mut plan, &strip));
        assert_eq!(chosen.rotation, plan.rotation);
        assert_eq!(chosen.standard_scale, plan.scale);
        // The painter's own fit measures the drawing's box, which a turned
        // drawing overfills: the view keeps the scale and centre chosen here.
        assert!(!plan.auto_scale);
        assert!(!plan.auto_centre);
        assert_eq!(plan.scale, 1250.0);
        assert!(plan.rotation > 0.0);
        assert!(plan.rotation < 45.0 * DEGREE);
        near(plan.centre.x, 5000.0, 1e-6);
        near(plan.centre.y, 7000.0, 1e-6);
        assert_eq!(
            plan.rect,
            tiling_area(&Sheet::default()),
            "the rectangle stays"
        );
        // At that turn the strip fits 1 : 1250 with the 4% to spare.
        let check = ok(fit_at_rotation(
            &strip,
            SizeMm {
                width: 385.0 / AUTO_SCALE_SPARE,
                height: 250.0 / AUTO_SCALE_SPARE,
            },
            plan.rotation,
        ));
        assert!(check.scale <= 1250.0 * (1.0 + 1e-9));

        let mut legend = viewport_at(
            "vp2",
            ViewportKind::Legend,
            tiling_area(&Sheet::default()),
            false,
        );
        assert_eq!(
            refused(rotate_to_best_fit(&mut legend, &strip)).code,
            ErrorCode::InvalidArgument
        );
        let mut unplaced = viewport_at("vp3", ViewportKind::Plan, Box2::default(), false);
        assert_eq!(
            refused(rotate_to_best_fit(&mut unplaced, &strip)).code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn the_fitted_layout_is_turned_when_that_fits_a_larger_scale_or_fewer_sheets() {
        // The strip at 45 degrees: its box is 332.3 m square, which one A3
        // sheet holds at 1 : 2000 (332.3 / 250 needs 1 : 1329). Turned 3.62
        // degrees (as above) it is 1 : 1250.
        let strip = turned_rectangle(Point2::new(0.0, 0.0), 450.0, 20.0, 45.0 * DEGREE);
        let mut request = LayoutRequest {
            plan_area: Some(box_of(&strip)),
            ..LayoutRequest::default()
        };
        let model = Model::new();
        let t = 45.0 * DEGREE - 225.0_f64.atan2(10.0) + (156.25 / 225.0_f64.hypot(10.0)).acos();

        let square = ok(smart_layout(&model, &request, None));
        assert_eq!(square[0].viewports[0].scale, 2000.0);
        let turned = ok(smart_layout_rotated(&model, &request, &strip, None));
        assert_eq!(turned.len(), 1);
        let plan = &turned[0].viewports[0];
        assert_eq!(plan.scale, 1250.0);
        near(plan.rotation, t, 1e-9);
        assert_eq!(plan.rect, square[0].viewports[0].rect);

        // At a fixed 1 : 1250 the square layout needs a key plan and tiles;
        // turned, one sheet holds it.
        request.scale = 1250.0;
        let tiles = ok(smart_layout(&model, &request, None));
        assert!(tiles.len() > 1);
        let one = ok(smart_layout_rotated(&model, &request, &strip, None));
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].viewports[0].scale, 1250.0);
        near(one[0].viewports[0].rotation, t, 1e-9);

        // At 1 : 2000 it fits square: nothing is turned.
        request.scale = 2000.0;
        assert_eq!(
            ok(smart_layout_rotated(&model, &request, &strip, None)),
            ok(smart_layout(&model, &request, None))
        );
        // At 1 : 1000 it fits no single sheet even turned: the tiles stay.
        request.scale = 1000.0;
        assert_eq!(
            ok(smart_layout_rotated(&model, &request, &strip, None)),
            ok(smart_layout(&model, &request, None))
        );
    }

    #[test]
    fn the_fitted_layout_turns_only_a_plan_of_an_area() {
        let model = Model::new();
        let strip = turned_rectangle(Point2::new(0.0, 0.0), 450.0, 20.0, 45.0 * DEGREE);
        let mut request = LayoutRequest::default();
        assert_eq!(
            refused(smart_layout_rotated(&model, &request, &strip, None)).code,
            ErrorCode::InvalidArgument
        );
        request.plan_area = Some(rect(0.0, 0.0, 100.0, 100.0));
        request.alignment = String::from("ROAD");
        request.long_section = true;
        assert_eq!(
            refused(smart_layout_rotated(&model, &request, &strip, None)).code,
            ErrorCode::InvalidArgument
        );
        // Content with no size is simply not turned.
        request.alignment.clear();
        request.long_section = false;
        let point = [Point2::new(5.0, 5.0)];
        assert_eq!(
            ok(smart_layout_rotated(&model, &request, &point, None)),
            ok(smart_layout(&model, &request, None))
        );
    }
}
