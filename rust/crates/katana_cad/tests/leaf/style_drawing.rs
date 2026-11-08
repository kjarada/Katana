// A library definition turned into geometry (tests/cad/test_style_drawing.cpp).

use crate::near;
use katana_cad::style_drawing::{
    flatten_definition, lay_linestyle, linestyle_drawing, style_drawing_of_style,
    symbol_drawing_of_style, two_point_drawing_of_style,
};
use katana_cad::{LinestyleLayoutOutcome, LinestyleOptions, StyleDrawing};
use katana_entity::{LineStyle, Stroke, StrokeOp, StrokeText, StyleUnits};
use katana_geometry::{Box2, Point2, Polyline2};
use katana_math::numerics::PI;

fn definition(strokes: Vec<Stroke>, units: StyleUnits) -> LineStyle {
    LineStyle {
        name: String::from("S"),
        units,
        strokes,
        ..LineStyle::default()
    }
}

fn world(strokes: Vec<Stroke>) -> LineStyle {
    definition(strokes, StyleUnits::World)
}

// The C++ `move`, a keyword here.
fn move_to(x: f64, y: f64) -> Stroke {
    Stroke {
        op: StrokeOp::Move,
        point: Point2::new(x, y),
        ..Stroke::default()
    }
}

fn draw(x: f64, y: f64) -> Stroke {
    Stroke {
        op: StrokeOp::Draw,
        point: Point2::new(x, y),
        ..Stroke::default()
    }
}

fn arc(radius: f64, start_angle: f64, end_angle: f64) -> Stroke {
    Stroke {
        op: StrokeOp::Arc,
        radius,
        start_angle,
        end_angle,
        ..Stroke::default()
    }
}

fn text_stroke(index: usize) -> Stroke {
    Stroke {
        op: StrokeOp::Text,
        text: index,
        ..Stroke::default()
    }
}

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn line(points: &[(f64, f64)]) -> Polyline2 {
    Polyline2::new(points.iter().map(|&(x, y)| p(x, y)).collect(), false)
}

// Total length of everything drawn, which is what a pattern repeating along a
// line multiplies.
fn drawn_length(drawing: &StyleDrawing) -> f64 {
    let mut total = 0.0;
    for stroke in &drawing.strokes {
        for pair in stroke.path.vertices.windows(2) {
            total += pair[0].distance_to(pair[1]);
        }
    }
    total
}

mod style_drawing {
    use super::*;

    #[test]
    fn a_move_starts_a_run_and_a_draw_extends_it() {
        // Two separate strokes: a 3-long run and a 1-long one.
        let style = world(vec![
            move_to(0.0, 0.0),
            draw(3.0, 0.0),
            move_to(5.0, 0.0),
            draw(5.0, 1.0),
        ]);
        let drawing = linestyle_drawing(&style, &line(&[(0.0, 0.0), (100.0, 0.0)]), 1.0);
        assert!(!drawing.strokes.is_empty());
        assert_eq!(drawing.strokes[0].path.vertices.len(), 2);
        assert_eq!(drawing.strokes[0].path.vertices[0], p(0.0, 0.0));
        assert_eq!(drawing.strokes[0].path.vertices[1], p(3.0, 0.0));
        // The period is the pen's span, 5, so 100/5 + 1 = 21 instances of two
        // runs each. The twenty-first starts exactly AT the end of the line:
        // its dash is clipped to nothing and its tick, which would sit five
        // further on, is dropped - a pattern belongs to the line it is laid
        // along.
        assert_eq!(drawing.strokes.len(), 21 * 2 - 1);
        near(drawn_length(&drawing), 20.0 * 4.0, 1e-9);
    }

    #[test]
    fn the_stated_length_is_the_period_rather_than_the_strokes_span() {
        let mut style = world(vec![move_to(0.0, 0.0), draw(1.0, 0.0)]);
        style.length = 10.0;
        let drawing = linestyle_drawing(&style, &line(&[(0.0, 0.0), (50.0, 0.0)]), 1.0);
        assert_eq!(drawing.strokes.len(), 6, "at 0, 10, 20, 30, 40 and 50");
        assert_eq!(drawing.strokes[1].path.vertices[0], p(10.0, 0.0));
    }

    #[test]
    fn the_patterns_y_is_across_the_line_and_bends_with_it() {
        // A tick 1 to the left of the line. The line turns a right angle at
        // (10,0) and is 20 long, so with a period of 10 the three instances
        // sit at the start, exactly ON the corner, and at the end.
        let mut style = world(vec![move_to(0.0, 0.0), draw(0.0, 1.0)]);
        style.length = 10.0;
        let drawing =
            linestyle_drawing(&style, &line(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]), 1.0);
        assert_eq!(drawing.strokes.len(), 3);
        // Along +x, left is +y.
        assert_eq!(drawing.strokes[0].path.vertices[0], p(0.0, 0.0));
        assert_eq!(drawing.strokes[0].path.vertices[1], p(0.0, 1.0));
        // On the corner the direction is genuinely two-valued, and the
        // OUTGOING leg is taken - the pattern is being laid forwards, so what
        // is about to be drawn belongs to the leg it is about to be drawn
        // along. Along +y, left is -x.
        assert_eq!(drawing.strokes[1].path.vertices[0], p(10.0, 0.0));
        let at_corner = drawing.strokes[1].path.vertices[1];
        near(at_corner.x, 9.0, 1e-9);
        near(at_corner.y, 0.0, 1e-9);
        // And at the end of the second leg, still across it.
        let at_end = drawing.strokes[2].path.vertices[1];
        near(at_end.x, 9.0, 1e-9);
        near(at_end.y, 10.0, 1e-9);
    }

    #[test]
    fn a_paper_style_is_measured_in_plot_millimetres_and_a_world_style_is_not() {
        let strokes = vec![move_to(0.0, 0.0), draw(2.0, 0.0)];
        // Long enough that 2 mm at 1:500 - a kilometre of ground - still fits
        // on it, since a pattern is clipped to the line it is drawn along.
        let along = line(&[(0.0, 0.0), (5000.0, 0.0)]);
        // 500 model units to the millimetre is a 1:500 plot.
        let paper = linestyle_drawing(
            &definition(strokes.clone(), StyleUnits::Paper),
            &along,
            500.0,
        );
        let world_style = linestyle_drawing(&definition(strokes, StyleUnits::World), &along, 500.0);
        assert_eq!(
            paper.strokes[0].path.vertices[1],
            p(1000.0, 0.0),
            "2 mm at 1:500 is 1000 m"
        );
        assert_eq!(
            world_style.strokes[0].path.vertices[1],
            p(2.0, 0.0),
            "2 model units, whatever the plot"
        );
    }

    #[test]
    fn a_symbol_is_placed_scaled_and_rotated() {
        // A unit square from -1 to 1: two wide, so `size` 4 doubles it.
        let mut style = world(vec![
            move_to(-1.0, -1.0),
            draw(1.0, -1.0),
            draw(1.0, 1.0),
            draw(-1.0, 1.0),
            draw(-1.0, -1.0),
        ]);
        style.at_vertices = true;
        let plain = symbol_drawing_of_style(&style, p(10.0, 20.0), 0.0, 0.0, 1.0);
        assert_eq!(plain.strokes[0].path.vertices[0], p(9.0, 19.0));

        let scaled = symbol_drawing_of_style(&style, p(0.0, 0.0), 4.0, 0.0, 1.0);
        assert_eq!(
            scaled.strokes[0].path.vertices[0],
            p(-2.0, -2.0),
            "size is a width"
        );

        let turned = symbol_drawing_of_style(&style, p(0.0, 0.0), 0.0, PI / 2.0, 1.0);
        let corner = turned.strokes[0].path.vertices[0];
        near(corner.x, 1.0, 1e-12); // (-1,-1) turned a quarter turn is (1,-1)
        near(corner.y, -1.0, 1e-12);
    }

    #[test]
    fn a_symbol_goes_on_every_vertex_and_a_linestyle_along_the_line() {
        let mut symbol = world(vec![move_to(0.0, 0.0), draw(1.0, 0.0)]);
        symbol.at_vertices = true;
        let along = line(&[(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)]);
        assert_eq!(
            style_drawing_of_style(&symbol, &along, 1.0).strokes.len(),
            3,
            "one per vertex"
        );

        let mut linestyle = world(vec![move_to(0.0, 0.0), draw(1.0, 0.0)]);
        linestyle.length = 5.0;
        assert_eq!(
            style_drawing_of_style(&linestyle, &along, 1.0)
                .strokes
                .len(),
            5,
            "0 5 10 15 20"
        );
    }

    #[test]
    fn a_two_point_style_stretches_between_its_anchors_without_shearing() {
        // Anchors 4 apart; drawn across a span of 8 turned a quarter turn, so
        // everything doubles and turns with it.
        let mut style = definition(
            vec![move_to(0.0, 0.0), draw(4.0, 0.0), draw(4.0, 1.0)],
            StyleUnits::TwoPoint,
        );
        style.anchor1 = p(0.0, 0.0);
        style.anchor2 = p(4.0, 0.0);
        let drawing = two_point_drawing_of_style(&style, p(0.0, 0.0), p(0.0, 8.0), 1.0);
        assert_eq!(drawing.strokes.len(), 1);
        let points = &drawing.strokes[0].path.vertices;
        assert_eq!(points.len(), 3);
        near(points[1].x, 0.0, 1e-12);
        near(points[1].y, 8.0, 1e-12); // the second anchor lands on the second point
        // (4,1) scaled by 2 and turned a quarter turn is (-2, 8).
        near(points[2].x, -2.0, 1e-12); // scaled equally in both axes, not sheared
        near(points[2].y, 8.0, 1e-12);
    }

    #[test]
    fn a_colour_command_sets_the_pen_for_what_follows_and_view_colour_means_the_entitys() {
        let pen = |name: &str| Stroke {
            op: StrokeOp::Pen,
            pen: String::from(name),
            ..Stroke::default()
        };
        let mut style = world(vec![
            pen("pen 035"),
            move_to(0.0, 0.0),
            draw(1.0, 0.0),
            pen("view_colour"),
            move_to(1.0, 0.0),
            draw(2.0, 0.0),
        ]);
        style.at_vertices = true;
        let drawing = symbol_drawing_of_style(&style, p(0.0, 0.0), 0.0, 0.0, 1.0);
        assert_eq!(drawing.strokes.len(), 2);
        assert_eq!(drawing.strokes[0].pen, "pen 035");
        assert_eq!(
            drawing.strokes[1].pen, "",
            "view_colour is the entity's own colour"
        );
    }

    // A definition read from a file sweeps no arc more than two turns, but one
    // made in code is not held to that, and the chords an arc is drawn with
    // follow its sweep: two billion degrees was twenty million points for one
    // arc, and a sweep past what an int holds of chords an undefined cast.
    #[test]
    fn an_arc_of_a_huge_sweep_is_drawn_as_two_turns_and_never_asked_for_millions_of_points() {
        // Chords are 24 to a turn: two turns, 48, and one more point than
        // chords.
        for end in [2.0e9, -2.0e9, 721.0, 1.0e300] {
            let mut style = world(vec![arc(1.0, 0.0, end)]);
            style.at_vertices = true;
            let drawing = symbol_drawing_of_style(&style, p(0.0, 0.0), 0.0, 0.0, 1.0);
            assert_eq!(drawing.strokes.len(), 1, "{end}");
            assert_eq!(drawing.strokes[0].path.vertices.len(), 49, "{end}");
        }
        // Exactly two turns is already that many, and a quarter turn is 6
        // chords.
        let mut style = world(vec![arc(1.0, 0.0, 90.0)]);
        style.at_vertices = true;
        assert_eq!(
            symbol_drawing_of_style(&style, p(0.0, 0.0), 0.0, 0.0, 1.0).strokes[0]
                .path
                .vertices
                .len(),
            7
        );
    }

    #[test]
    fn an_arc_whose_angles_are_not_numbers_draws_nothing_and_does_not_crash() {
        for bad in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            let mut style = world(vec![arc(1.0, 0.0, bad)]);
            style.at_vertices = true;
            let drawing = symbol_drawing_of_style(&style, p(0.0, 0.0), 0.0, 0.0, 1.0);
            for stroke in &drawing.strokes {
                assert!(stroke.path.vertices.is_empty(), "{bad}");
            }
        }
    }

    #[test]
    fn a_circle_an_arc_and_a_dot_are_drawn_about_the_current_point() {
        let circle = Stroke {
            op: StrokeOp::Circle,
            radius: 2.0,
            ..Stroke::default()
        };
        let dot = Stroke {
            op: StrokeOp::Dot,
            ..Stroke::default()
        };

        let mut style = world(vec![move_to(5.0, 5.0), circle, arc(1.0, 0.0, 180.0), dot]);
        style.at_vertices = true;
        let drawing = symbol_drawing_of_style(&style, p(0.0, 0.0), 0.0, 0.0, 1.0);
        assert_eq!(drawing.strokes.len(), 3);

        assert!(drawing.strokes[0].path.closed);
        for point in &drawing.strokes[0].path.vertices {
            near(point.distance_to(p(5.0, 5.0)), 2.0, 1e-12); // a circle about the pen
        }
        // A half turn of radius 1 from (6,5) round to (4,5).
        let arc_points = &drawing.strokes[1].path.vertices;
        near(arc_points[0].x, 6.0, 1e-12);
        near(arc_points[arc_points.len() - 1].x, 4.0, 1e-12);
        near(arc_points[arc_points.len() - 1].y, 5.0, 1e-12);

        assert_eq!(
            drawing.strokes[2].path.vertices.len(),
            1,
            "a dot is a point mark"
        );
        assert_eq!(drawing.strokes[2].path.vertices[0], p(5.0, 5.0));
    }

    #[test]
    fn text_takes_the_direction_of_the_line_it_sits_on() {
        let mut style = world(vec![move_to(0.0, 0.0)]);
        style.texts.push(StrokeText {
            text: String::from("WM"),
            height: 1.5,
            justify: String::from("middle-centre"),
            font: String::from("Arial"),
            ..StrokeText::default()
        });
        style.strokes.push(text_stroke(0));
        style.length = 10.0;

        // A line going straight up: the text turns with it.
        let drawing = linestyle_drawing(&style, &line(&[(0.0, 0.0), (0.0, 30.0)]), 1.0);
        assert!(!drawing.texts.is_empty());
        assert_eq!(drawing.texts[0].text, "WM");
        assert_eq!(drawing.texts[0].height, 1.5);
        near(drawing.texts[0].angle, PI / 2.0, 1e-12);
        assert_eq!(drawing.texts[0].justify, "middle-centre");
        assert_eq!(drawing.texts.len(), 4, "at 0, 10, 20 and 30");
    }

    #[test]
    fn a_symbols_text_keeps_its_width_factor_and_font_for_the_painter() {
        // 326 of the 514 reference texts are narrowed (0.8 or 0.85); the
        // painter can only narrow what reaches it.
        let mut style = world(vec![move_to(0.0, 0.0)]);
        style.at_vertices = true;
        style.texts.push(StrokeText {
            text: String::from("BM"),
            height: 2.0,
            font: String::from("Arial"),
            width_factor: 0.8,
            ..StrokeText::default()
        });
        style.strokes.push(text_stroke(0));

        // Size 0 is the definition's own scale, so the height stays 2.0.
        let drawing = symbol_drawing_of_style(&style, p(5.0, 5.0), 0.0, 0.0, 1.0);
        assert_eq!(drawing.texts.len(), 1);
        assert_eq!(drawing.texts[0].width_factor, 0.8);
        assert_eq!(drawing.texts[0].font, "Arial");
        assert_eq!(drawing.texts[0].height, 2.0);
    }

    #[test]
    fn a_definition_with_no_extent_is_drawn_once_rather_than_endlessly() {
        // A single tick across the line has no length along it, so there is
        // no period to repeat at. Drawing it once beats not returning.
        let style = world(vec![move_to(0.0, 0.0), draw(0.0, 1.0)]);
        let drawing = linestyle_drawing(&style, &line(&[(0.0, 0.0), (1000.0, 0.0)]), 1.0);
        assert_eq!(drawing.strokes.len(), 1);
    }

    #[test]
    fn a_very_short_period_on_a_very_long_line_lays_nothing_rather_than_a_truncated_pattern() {
        let mut style = world(vec![move_to(0.0, 0.0), draw(0.001, 0.0)]);
        style.length = 0.001;
        // 30 km of traverse at a millimetre a time would be thirty million
        // instances. It used to be cut off at twenty thousand - 20 m of a 30
        // km line, the rest silently missing (audit CAD-04). Now nothing is
        // laid and the caller is told, so it draws the plain line instead.
        let traverse = line(&[(0.0, 0.0), (30000.0, 0.0)]);
        assert!(linestyle_drawing(&style, &traverse, 1.0).empty());
        let laid = lay_linestyle(
            &flatten_definition(&style),
            &traverse,
            &LinestyleOptions::default(),
        );
        assert_eq!(laid.outcome, LinestyleLayoutOutcome::OverBudget);
        assert!(laid.drawing.empty());
        assert_eq!(laid.instances, 0);
    }

    #[test]
    fn only_the_repeats_that_can_reach_the_view_are_laid_each_where_it_falls_on_the_whole_line() {
        // A 1-long dash every 10 along a 1000-long line: unclipped, repeats
        // 0, 10, ..., 1000 - floor(1000 / 10) + 1 = 101 of them.
        let mut style = world(vec![move_to(0.0, 0.0), draw(1.0, 0.0)]);
        style.length = 10.0;
        let along = line(&[(0.0, 0.0), (1000.0, 0.0)]);
        let flat = flatten_definition(&style);
        assert_eq!(
            lay_linestyle(&flat, &along, &LinestyleOptions::default()).instances,
            101
        );

        // The view spans x 93..207. Grown by one period (10) and by the
        // pattern's reach across the line (0: every point is ON it) it is
        // 83..217, and repeat k covers [10k, 10k + 1], so it can reach that
        // stretch for k from ceil((83 - 1) / 10) = 9 to floor(217 / 10) = 21:
        // thirteen repeats, at 90, 100, ..., 210 - multiples of the period
        // from the line's own start, so the pattern does not crawl as the
        // view pans.
        let options = LinestyleOptions {
            visible: Some(Box2::new(p(93.0, -5.0), p(207.0, 5.0))),
            ..LinestyleOptions::default()
        };
        let laid = lay_linestyle(&flat, &along, &options);
        assert_eq!(laid.outcome, LinestyleLayoutOutcome::Laid);
        assert_eq!(laid.instances, 13);
        assert_eq!(laid.drawing.strokes.len(), 13);
        assert_eq!(laid.drawing.strokes[0].path.vertices[0], p(90.0, 0.0));
        assert_eq!(
            laid.drawing.strokes[laid.drawing.strokes.len() - 1]
                .path
                .vertices[0],
            p(210.0, 0.0)
        );

        // And the budget is counted on what is laid: 101 is over a budget of
        // 50, 13 is not.
        let mut tight = options;
        tight.maximum_instances = 50;
        assert_eq!(
            lay_linestyle(&flat, &along, &tight).outcome,
            LinestyleLayoutOutcome::Laid
        );
        tight.visible = None;
        assert_eq!(
            lay_linestyle(&flat, &along, &tight).outcome,
            LinestyleLayoutOutcome::OverBudget
        );
    }

    #[test]
    fn a_mark_beside_a_line_outside_the_view_is_still_laid_when_it_reaches_into_it() {
        // A tick 3 to the left of the line, every 10. The line runs along
        // y = 0, below the view (y 1..5, x 43..57), and its ticks reach up
        // into it. Grown by the reach across (3) and one period (10), the view
        // is x 30..70, y -12..18, which the line crosses from 30 to 70; a tick
        // covers only its own chainage, so repeats k = ceil(30 / 10) = 3 to
        // floor(70 / 10) = 7: five of them, at 30, 40, 50, 60 and 70.
        let mut style = world(vec![move_to(0.0, 0.0), draw(0.0, 3.0)]);
        style.length = 10.0;
        let options = LinestyleOptions {
            visible: Some(Box2::new(p(43.0, 1.0), p(57.0, 5.0))),
            ..LinestyleOptions::default()
        };
        let laid = lay_linestyle(
            &flatten_definition(&style),
            &line(&[(0.0, 0.0), (100.0, 0.0)]),
            &options,
        );
        assert_eq!(laid.outcome, LinestyleLayoutOutcome::Laid);
        assert_eq!(laid.drawing.strokes.len(), 5);
        assert_eq!(laid.drawing.strokes[0].path.vertices[0], p(30.0, 0.0));
        assert_eq!(laid.drawing.strokes[4].path.vertices[1], p(70.0, 3.0));
    }

    #[test]
    fn a_whole_line_in_view_lays_exactly_what_the_unclipped_laying_does_when_an_arc_overhangs_the_pen()
     {
        // A scallop written the way a library writes arcs - move to the
        // centre, then arc - so the pen only ever stands at x = 0.5 while the
        // half-circle below it runs from x = 0 (180 deg) round to x = 1 (360
        // deg).
        //   length 1 / move 0.5 0 / arc 0.5 180 360
        // Along a 10.4-long line, unclipped: repeats k = 0 .. floor(10.4 / 1)
        // = 10, eleven of them, and repeat 10's arc covers 10 .. 11, so it
        // starts ON the line at (10, 0) and is drawn, cut off at the end.
        // With the whole line in view the visible stretch is all of 0 .. 10.4,
        // and repeat k reaches it while k + 0 <= 10.4 - the arc's own low x,
        // not the pen's 0.5, which gave floor((10.4 - 0.5) / 1) = 9 and left
        // the last 0.4 of the line blank on screen and on the plot while the
        // preview drew it.
        let mut scallop = world(vec![move_to(0.5, 0.0), arc(0.5, 180.0, 360.0)]);
        scallop.length = 1.0;
        // The same hole, from the other side: a Draw from the pen's starting
        // place, which no Move ever stood on. "length 1 / draw 0.5 0" draws
        // 0 .. 0.5 in every period; repeat 10 draws 10 .. 10.4.
        let mut unmoved = world(vec![draw(0.5, 0.0)]);
        unmoved.length = 1.0;

        let along = line(&[(0.0, 0.0), (10.4, 0.0)]);
        let whole_line = LinestyleOptions {
            visible: Some(Box2::new(p(-5.0, -5.0), p(15.0, 5.0))),
            ..LinestyleOptions::default()
        };
        for style in [&scallop, &unmoved] {
            let flat = flatten_definition(style);
            let unclipped = lay_linestyle(&flat, &along, &LinestyleOptions::default());
            let clipped = lay_linestyle(&flat, &along, &whole_line);
            assert_eq!(clipped.outcome, LinestyleLayoutOutcome::Laid);
            assert_eq!(unclipped.instances, 11);
            assert_eq!(clipped.instances, 11);
            assert_eq!(unclipped.drawing.strokes.len(), 11);
            assert_eq!(clipped.drawing.strokes.len(), 11);
            for (i, (clipped_stroke, unclipped_stroke)) in clipped
                .drawing
                .strokes
                .iter()
                .zip(&unclipped.drawing.strokes)
                .enumerate()
            {
                assert_eq!(
                    clipped_stroke.path.vertices, unclipped_stroke.path.vertices,
                    "repeat {i}"
                );
            }
            let last_start = clipped.drawing.strokes[clipped.drawing.strokes.len() - 1]
                .path
                .vertices[0];
            near(last_start.x, 10.0, 1e-9);
            near(last_start.y, 0.0, 1e-9);
        }
    }

    #[test]
    fn a_pattern_under_two_pixels_is_not_laid_so_the_caller_draws_the_plain_line() {
        // A period of 10 model units: at 0.15 px a unit it is 1.5 px on
        // screen, under the two-pixel floor; at 0.25 px it is 2.5 px and is
        // laid.
        let mut style = world(vec![move_to(0.0, 0.0), draw(1.0, 0.0)]);
        style.length = 10.0;
        let flat = flatten_definition(&style);
        let along = line(&[(0.0, 0.0), (100.0, 0.0)]);
        let mut options = LinestyleOptions {
            view_scale: 0.15,
            ..LinestyleOptions::default()
        };
        assert_eq!(
            lay_linestyle(&flat, &along, &options).outcome,
            LinestyleLayoutOutcome::TooFine
        );
        options.view_scale = 0.25;
        assert_eq!(
            lay_linestyle(&flat, &along, &options).outcome,
            LinestyleLayoutOutcome::Laid
        );
    }

    #[test]
    fn degenerate_input_gives_nothing_rather_than_nonsense() {
        let style = world(vec![move_to(0.0, 0.0), draw(1.0, 0.0)]);
        assert!(linestyle_drawing(&style, &Polyline2::default(), 1.0).empty());
        assert!(style_drawing_of_style(&style, &Polyline2::default(), 1.0).empty());
        // Every vertex in one place: no direction to lay a pattern along.
        assert!(
            linestyle_drawing(&style, &line(&[(5.0, 5.0), (5.0, 5.0), (5.0, 5.0)]), 1.0).empty()
        );
        // A definition with no strokes draws nothing, from anywhere.
        assert!(symbol_drawing_of_style(&world(Vec::new()), p(0.0, 0.0), 0.0, 0.0, 1.0).empty());
    }

    #[test]
    fn the_factor_scales_every_coordinate_and_the_origin_is_where_the_definition_is_anchored() {
        let mut style = world(vec![move_to(1.0, 1.0), draw(3.0, 1.0)]);
        style.factor = 2.0;
        style.origin = p(1.0, 1.0);
        style.at_vertices = true;
        let drawing = symbol_drawing_of_style(&style, p(100.0, 200.0), 0.0, 0.0, 1.0);
        // (1,1) is the origin, so it lands on the point; (3,1) is 2 further in
        // x, doubled by the factor.
        assert_eq!(drawing.strokes[0].path.vertices[0], p(100.0, 200.0));
        assert_eq!(drawing.strokes[0].path.vertices[1], p(104.0, 200.0));
    }

    #[test]
    fn a_trailing_move_is_the_gap_and_counts_towards_the_period() {
        // "move 0 0 / draw 3 0 / move 5 0" is a three-unit dash and a TWO-UNIT
        // GAP: the bare move at the end is how a library linestyle ends its
        // period. Measuring only what was drawn made the period 3, so every
        // dash butted against the next one and the whole linestyle came out
        // as a solid line.
        let style = world(vec![move_to(0.0, 0.0), draw(3.0, 0.0), move_to(5.0, 0.0)]);
        let drawing = linestyle_drawing(&style, &line(&[(0.0, 0.0), (20.0, 0.0)]), 1.0);
        assert_eq!(drawing.strokes.len(), 5, "at 0, 5, 10, 15 and 20");
        assert_eq!(drawing.strokes[0].path.vertices[0], p(0.0, 0.0));
        assert_eq!(drawing.strokes[0].path.vertices[1], p(3.0, 0.0));
        assert_eq!(
            drawing.strokes[1].path.vertices[0],
            p(5.0, 0.0),
            "the gap is two long"
        );
        // Four dashes of three, and a fifth that starts exactly at the end of
        // the line and so is clipped to nothing.
        near(drawn_length(&drawing), 12.0, 1e-9);
    }

    #[test]
    fn a_pattern_is_clipped_to_the_line_rather_than_running_off_the_end_of_it() {
        // A dash longer than the line it is laid along stops where the line
        // does. Letting it run on drew a 164 m dash for a 30 m fence and
        // scribbled it across the drawing, which is what a zoomed-out survey
        // looked like.
        let mut style = world(vec![move_to(0.0, 0.0), draw(50.0, 0.0)]);
        style.length = 100.0;
        let drawing = linestyle_drawing(&style, &line(&[(0.0, 0.0), (10.0, 0.0)]), 1.0);
        assert_eq!(drawing.strokes.len(), 1);
        let vertices = &drawing.strokes[0].path.vertices;
        assert_eq!(
            vertices[vertices.len() - 1],
            p(10.0, 0.0),
            "stops at the end"
        );

        // And a run that lies WHOLLY beyond the end is not drawn at all.
        let mut beyond = world(vec![
            move_to(0.0, 0.0),
            draw(1.0, 0.0),
            move_to(80.0, 0.0),
            draw(90.0, 0.0),
        ]);
        beyond.length = 100.0;
        let clipped = linestyle_drawing(&beyond, &line(&[(0.0, 0.0), (10.0, 0.0)]), 1.0);
        assert_eq!(clipped.strokes.len(), 1, "only the run that is on the line");
        let vertices = &clipped.strokes[0].path.vertices;
        assert_eq!(vertices[vertices.len() - 1], p(1.0, 0.0));
    }

    #[test]
    fn a_negative_arc_radius_is_a_length_not_a_side_so_the_arc_is_not_mirrored_through_its_centre()
    {
        // Cut from a road-marking symbol in a reference library (a U-turn
        // arrow), which brackets each arc with moves to where it starts and
        // ends:
        //   move 1.127 -0.5348 / move 1.1379 0.0095 /
        //   arc -0.5444 -91.0850 -62.5336 / move 1.3859 -0.4751
        // Worked by hand with |r| = 0.5444 about (1.1379, 0.0095):
        //   start -91.0850 deg: cos = -sin(1.0850 deg) = -0.018936,
        //                       sin = -cos(1.0850 deg) = -0.999821
        //     x = 1.1379 + 0.5444 * -0.018936 = 1.127591
        //     y = 0.0095 + 0.5444 * -0.999821 = -0.534803
        //   end -62.5336 deg:   cos = 0.461228, sin = -0.887281
        //     x = 1.1379 + 0.5444 * 0.461228 = 1.388993
        //     y = 0.0095 + 0.5444 * -0.887281 = -0.473536
        // which the file's breadcrumbs (1.127, -0.5348) and (1.3859, -0.4751)
        // confirm to within 4 mm of symbol unit. The signed radius put the arc
        // at (1.1482, 0.5538) -> (0.8868, 0.4925): the same arc turned half a
        // turn about its centre, so every U-turn arrow was drawn broken.
        let mut style = world(vec![
            move_to(1.127, -0.5348),
            move_to(1.1379, 0.0095),
            arc(-0.5444, -91.0850, -62.5336),
            move_to(1.3859, -0.4751),
        ]);
        style.at_vertices = true;
        let drawing = symbol_drawing_of_style(&style, p(0.0, 0.0), 0.0, 0.0, 1.0);
        assert_eq!(drawing.strokes.len(), 1);
        let points = &drawing.strokes[0].path.vertices;
        assert!(points.len() >= 2);
        let first = points[0];
        let last = points[points.len() - 1];
        near(first.x, 1.127591, 1e-5);
        near(first.y, -0.534803, 1e-5);
        near(last.x, 1.388993, 1e-5);
        near(last.y, -0.473536, 1e-5);
        // And against the file's own breadcrumbs, independently of the
        // arithmetic.
        near(first.distance_to(p(1.127, -0.5348)), 0.0, 0.004);
        near(last.distance_to(p(1.3859, -0.4751)), 0.0, 0.004);
    }

    #[test]
    fn a_text_that_would_sit_off_the_end_of_the_line_is_not_drawn() {
        let mut style = world(vec![move_to(90.0, 0.0)]);
        style.texts.push(StrokeText {
            text: String::from("W"),
            height: 1.0,
            ..StrokeText::default()
        });
        style.strokes.push(text_stroke(0));
        style.length = 100.0;
        // The word would sit 90 along a line that is only 10 long.
        assert!(
            linestyle_drawing(&style, &line(&[(0.0, 0.0), (10.0, 0.0)]), 1.0)
                .texts
                .is_empty()
        );
        // On a line long enough for it, it is drawn.
        assert!(
            !linestyle_drawing(&style, &line(&[(0.0, 0.0), (200.0, 0.0)]), 1.0)
                .texts
                .is_empty()
        );
    }
}
