// A plan's coordinate grid, worked out on paper (plan_grid.hpp): the
// automatic interval, the label text, where the lines, crosses, ticks and
// labels fall - square and rotated - what the labels keep off, the JSON form
// and the undoable edit; tests/cad/plotting/test_plan_grid.cpp.

use std::f64::consts::PI;

use crate::{double_eq, near};
use katana_cad::Document;
use katana_cad::plot::SHEET_SCALES;
use katana_cad::plotting::plan_grid::{
    automatic_grid_interval, grid_decimals, grid_style_from, grouped_coordinate, plan_footprint,
    plan_grid as compute_plan_grid, plan_paper_to_world, plan_world_to_paper, set_plan_grid,
    stored_placement,
};
use katana_cad::plotting::sheet_commands::add_sheet;
use katana_cad::plotting::sheet_json::{sheet_set_from_json, sheet_set_to_json};
use katana_cad::plotting::{
    GridAxis, GridSegment, GridStyle, HorizontalJustify, PlanGrid, PlanGridOptions, PlanPlacement,
    Sheet, SheetSet, VerticalJustify, Viewport, ViewportEdge, ViewportKind,
};
use katana_core::ErrorCode;
use katana_entity::EnumName;
use katana_geometry::{Box2, Point2};

fn ok<T>(result: katana_core::Result<T>) -> T {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

// A 200 x 100 mm plan at 1:1000 over (305 230, 6 250 410): the ground from E
// 305 130 to 305 330 and N 6 250 360 to 6 250 460.
fn gridded(style: GridStyle, interval: f64, rotation: f64) -> Viewport {
    Viewport {
        id: String::from("vp1"),
        kind: ViewportKind::Plan,
        rect: Box2::new(Point2::new(100.0, 100.0), Point2::new(300.0, 200.0)),
        scale: 1000.0,
        centre: Point2::new(305230.0, 6250410.0),
        rotation,
        grid_style: style,
        grid_interval: interval,
        ..Viewport::default()
    }
}

fn grid_with(viewport: &Viewport, options: &PlanGridOptions<'_>) -> PlanGrid {
    ok(compute_plan_grid(
        viewport,
        &stored_placement(viewport),
        options,
    ))
}

fn grid_of(viewport: &Viewport) -> PlanGrid {
    grid_with(viewport, &PlanGridOptions::default())
}

fn overlap(a: &Box2, b: &Box2) -> bool {
    a.min.x < b.max.x && b.min.x < a.max.x && a.min.y < b.max.y && b.min.y < a.max.y
}

fn on_border(p: Point2, r: &Box2) -> bool {
    const TOLERANCE: f64 = 1e-9;
    let inside = p.x >= r.min.x - TOLERANCE
        && p.x <= r.max.x + TOLERANCE
        && p.y >= r.min.y - TOLERANCE
        && p.y <= r.max.y + TOLERANCE;
    inside
        && ((p.x - r.min.x).abs() < TOLERANCE
            || (p.x - r.max.x).abs() < TOLERANCE
            || (p.y - r.min.y).abs() < TOLERANCE
            || (p.y - r.max.y).abs() < TOLERANCE)
}

fn label_texts(grid: &PlanGrid, edge: ViewportEdge) -> Vec<String> {
    grid.labels
        .iter()
        .filter(|label| label.edge == edge)
        .map(|label| label.text.clone())
        .collect()
}

mod plan_grid {
    use super::*;

    #[test]
    fn the_automatic_interval_is_the_round_spacing_nearest_fifty_millimetres_on_the_paper() {
        double_eq(automatic_grid_interval(500.0), 20.0); // 40 mm apart
        double_eq(automatic_grid_interval(1000.0), 50.0); // 50 mm
        double_eq(automatic_grid_interval(2000.0), 100.0); // 50 mm
        double_eq(automatic_grid_interval(250.0), 10.0); // 40 mm
        double_eq(automatic_grid_interval(200.0), 10.0); // 50 mm
        // 1:750 lies between 20 m (27 mm) and 50 m (67 mm); 67 mm is nearer
        // 50 by ratio.
        double_eq(automatic_grid_interval(750.0), 50.0);
        double_eq(automatic_grid_interval(1.0), 0.05);
        assert_eq!(automatic_grid_interval(0.0), 0.0);
        assert_eq!(automatic_grid_interval(-500.0), 0.0);
        assert_eq!(automatic_grid_interval(f64::NAN), 0.0);
        assert_eq!(automatic_grid_interval(f64::INFINITY), 0.0);
        // Every scale of the ladder: 40 to 67 mm apart (1:75, 1:150 and 1:750
        // fall between 1-2-5 steps; 67 mm is nearer 50 than 27 mm is).
        for scale in SHEET_SCALES {
            let interval = automatic_grid_interval(scale);
            let spacing = interval * 1000.0 / scale;
            assert!(spacing >= 40.0 - 1e-9, "1:{scale}");
            assert!(spacing <= 200.0 / 3.0 + 1e-9, "1:{scale}");
            let first = interval / 10.0_f64.powf((interval.log10() + 1e-12).floor());
            assert!(
                (first - 1.0).abs() < 1e-9
                    || (first - 2.0).abs() < 1e-9
                    || (first - 5.0).abs() < 1e-9,
                "{interval}"
            );
        }
    }

    #[test]
    fn coordinates_are_grouped_by_thousands_with_decimals_only_when_the_interval_needs_them() {
        assert_eq!(grid_decimals(20.0), 0);
        assert_eq!(grid_decimals(1.0), 0);
        assert_eq!(grid_decimals(0.5), 1);
        assert_eq!(grid_decimals(2.5), 1);
        assert_eq!(grid_decimals(0.1), 1);
        assert_eq!(grid_decimals(0.25), 2);
        assert_eq!(grid_decimals(0.05), 2);
        assert_eq!(grid_decimals(0.001), 3);
        assert_eq!(grid_decimals(0.0001), 3);

        assert_eq!(grouped_coordinate(305200.0, 0), "305 200");
        assert_eq!(grouped_coordinate(6250400.0, 0), "6 250 400");
        assert_eq!(grouped_coordinate(100000.0, 0), "100 000");
        assert_eq!(grouped_coordinate(999.0, 0), "999");
        assert_eq!(grouped_coordinate(1000.0, 0), "1 000");
        assert_eq!(grouped_coordinate(-1200.0, 0), "-1 200");
        assert_eq!(grouped_coordinate(-305200.5, 1), "-305 200.5");
        assert_eq!(grouped_coordinate(12.5, 1), "12.5");
        assert_eq!(grouped_coordinate(1234567.25, 2), "1 234 567.25");
        // Nothing prints as "-0".
        assert_eq!(grouped_coordinate(-0.0, 0), "0");
        assert_eq!(grouped_coordinate(-0.4, 0), "0");
        assert_eq!(grouped_coordinate(-0.04, 1), "0.0");
    }

    #[test]
    fn the_styles_have_stable_names() {
        for style in [
            GridStyle::None,
            GridStyle::Ticks,
            GridStyle::Crosses,
            GridStyle::Lines,
        ] {
            assert_eq!(grid_style_from(style.name()), Some(style));
        }
        assert_eq!(GridStyle::Crosses.name(), "crosses");
        assert!(grid_style_from("dots").is_none());
        assert!(grid_style_from("").is_none());
    }

    #[test]
    fn paper_and_ground_convert_both_ways_and_the_footprint_is_the_ground_under_the_rectangle() {
        let viewport = gridded(GridStyle::Lines, 50.0, 0.3);
        let at = stored_placement(&viewport);
        for paper in [
            Point2::new(100.0, 100.0),
            Point2::new(212.5, 187.25),
            Point2::new(300.0, 150.0),
        ] {
            let back =
                plan_world_to_paper(&viewport, &at, plan_paper_to_world(&viewport, &at, paper));
            near(back.x, paper.x, 1e-6);
            near(back.y, paper.y, 1e-6);
        }
        // The rectangle's centre is the viewport's centre on the ground.
        let middle = plan_paper_to_world(&viewport, &at, Point2::new(200.0, 150.0));
        near(middle.x, 305230.0, 1e-9);
        near(middle.y, 6250410.0, 1e-9);

        let square = gridded(GridStyle::Lines, 50.0, 0.0);
        let corners = plan_footprint(&square, &stored_placement(&square));
        assert_eq!(corners.len(), 4);
        near(corners[0].x, 305130.0, 1e-9);
        near(corners[0].y, 6250360.0, 1e-9);
        near(corners[2].x, 305330.0, 1e-9);
        near(corners[2].y, 6250460.0, 1e-9);

        let mut unplaced = square.clone();
        unplaced.rect = Box2::default();
        assert!(plan_footprint(&unplaced, &stored_placement(&unplaced)).is_empty());
        assert!(
            plan_footprint(
                &square,
                &PlanPlacement {
                    scale: 0.0,
                    centre: Point2::default()
                }
            )
            .is_empty()
        );
    }

    #[test]
    fn a_square_grids_lines_lie_at_round_coordinates_right_across_the_viewport() {
        let grid = grid_of(&gridded(GridStyle::Lines, 50.0, 0.0));
        assert_eq!(grid.style, GridStyle::Lines);
        double_eq(grid.interval, 50.0);
        assert_eq!(grid.decimals, 0);
        // Eastings 305 150 to 305 300, then northings 6 250 400 and 6 250 450.
        assert_eq!(grid.lines.len(), 6);
        let eastings = [305150.0, 305200.0, 305250.0, 305300.0];
        for (line, easting) in grid.lines.iter().zip(eastings) {
            assert_eq!(line.axis, GridAxis::Easting);
            double_eq(line.value, easting);
            let x = 200.0 + (easting - 305230.0);
            near(line.from.x, x, 1e-6);
            near(line.from.y, 100.0, 1e-9);
            near(line.to.x, x, 1e-6);
            near(line.to.y, 200.0, 1e-9);
            assert_eq!(line.from_edge, ViewportEdge::Bottom);
            assert_eq!(line.to_edge, ViewportEdge::Top);
        }
        assert_eq!(grid.lines[4].axis, GridAxis::Northing);
        double_eq(grid.lines[4].value, 6250400.0);
        near(grid.lines[4].from.x, 100.0, 1e-9);
        near(grid.lines[4].from.y, 140.0, 1e-6);
        near(grid.lines[4].to.x, 300.0, 1e-9);
        assert_eq!(grid.lines[4].from_edge, ViewportEdge::Left);
        assert_eq!(grid.lines[4].to_edge, ViewportEdge::Right);
        near(grid.lines[5].from.y, 190.0, 1e-6);

        // Lines are stroked whole, and nothing else.
        assert_eq!(grid.strokes.len(), grid.lines.len());
        for (stroke, line) in grid.strokes.iter().zip(&grid.lines) {
            assert_eq!(stroke.from, line.from);
            assert_eq!(stroke.to, line.to);
        }
        assert!(grid.crossings.is_empty());
    }

    #[test]
    fn labels_read_the_coordinate_along_every_edge_inside_the_viewport() {
        let viewport = gridded(GridStyle::Lines, 50.0, 0.0);
        let grid = grid_of(&viewport);
        let across = vec!["E 305 150", "E 305 200", "E 305 250", "E 305 300"];
        let up = vec!["N 6 250 400", "N 6 250 450"];
        assert_eq!(label_texts(&grid, ViewportEdge::Bottom), across);
        assert_eq!(label_texts(&grid, ViewportEdge::Top), across);
        assert_eq!(label_texts(&grid, ViewportEdge::Left), up);
        assert_eq!(label_texts(&grid, ViewportEdge::Right), up);
        assert_eq!(grid.labels.len(), 12);
        // Bottom first, then left, top and right.
        assert_eq!(grid.labels[0].edge, ViewportEdge::Bottom);
        assert_eq!(grid.labels[grid.labels.len() - 1].edge, ViewportEdge::Right);

        for label in &grid.labels {
            assert!(
                viewport.rect.contains_box(label.box_value),
                "{}",
                label.text
            );
            assert_eq!(label.horizontal, HorizontalJustify::Centre);
            match label.edge {
                ViewportEdge::Bottom => {
                    // Along the edge, 0.8 mm in, standing on the anchor.
                    double_eq(label.angle_degrees, 0.0);
                    near(label.anchor.y, 100.8, 1e-9);
                    assert_eq!(label.vertical, VerticalJustify::Bottom);
                }
                ViewportEdge::Top => {
                    double_eq(label.angle_degrees, 0.0);
                    near(label.anchor.y, 199.2, 1e-9);
                    assert_eq!(label.vertical, VerticalJustify::Top);
                }
                ViewportEdge::Left => {
                    // Up the side, its head at the border.
                    double_eq(label.angle_degrees, 90.0);
                    near(label.anchor.x, 100.8, 1e-9);
                    assert_eq!(label.vertical, VerticalJustify::Top);
                }
                ViewportEdge::Right => {
                    double_eq(label.angle_degrees, 90.0);
                    near(label.anchor.x, 299.2, 1e-9);
                    assert_eq!(label.vertical, VerticalJustify::Bottom);
                }
            }
        }
        // Centred on its line: E 305 200 is at x 170, N 6 250 400 at y 140.
        near(grid.labels[1].anchor.x, 170.0, 1e-6);
        near(grid.labels[4].anchor.y, 140.0, 1e-6);
        // No two labels share any paper.
        for (i, label) in grid.labels.iter().enumerate() {
            for other in &grid.labels[i + 1..] {
                assert!(
                    !overlap(&label.box_value, &other.box_value),
                    "{} / {}",
                    label.text,
                    other.text
                );
            }
        }
    }

    #[test]
    fn labels_keep_off_the_furniture_and_off_each_other() {
        let viewport = gridded(GridStyle::Lines, 50.0, 0.0);
        // A title in the bottom-left corner, 80 x 10 mm: the labels of E 305
        // 150 and E 305 200 would sit on it.
        let options = PlanGridOptions {
            keep_out: vec![Box2::new(
                Point2::new(100.0, 100.0),
                Point2::new(180.0, 110.0),
            )],
            ..PlanGridOptions::default()
        };
        let grid = grid_with(&viewport, &options);
        assert_eq!(
            label_texts(&grid, ViewportEdge::Bottom),
            vec!["E 305 250", "E 305 300"]
        );
        for label in &grid.labels {
            assert!(
                !overlap(&label.box_value, &options.keep_out[0]),
                "{}",
                label.text
            );
        }
        // The lines themselves are not cut: the title knocks them out when
        // drawn.
        assert_eq!(grid.lines.len(), 6);

        // Labels as wide as the spacing, 50 mm (50.8 with their knock-out):
        // E 305 150 at x 120 would stick out past the left edge, E 305 250
        // would overlap E 305 200, so every other one is left out along the
        // bottom.
        let fifty = |_: &str| 50.0;
        let wide = PlanGridOptions {
            label_width_mm: Some(&fifty),
            ..PlanGridOptions::default()
        };
        let crowded = grid_with(&viewport, &wide);
        assert_eq!(
            label_texts(&crowded, ViewportEdge::Bottom),
            vec!["E 305 200", "E 305 300"]
        );
        // Up the sides N 6 250 450, at y 190, would stick out past the top.
        assert_eq!(
            label_texts(&crowded, ViewportEdge::Left),
            vec!["N 6 250 400"]
        );
        for (i, label) in crowded.labels.iter().enumerate() {
            assert!(viewport.rect.contains_box(label.box_value));
            for other in &crowded.labels[i + 1..] {
                assert!(!overlap(&label.box_value, &other.box_value));
            }
        }
    }

    #[test]
    fn two_labels_closer_than_the_spacing_are_not_both_kept() {
        // Along the bottom E 305 150, 305 200 ... sit 50 mm apart. Labels 48
        // mm wide leave 2 mm between their centres' neighbours less 0.8 mm of
        // knock-out: 1.2 mm of paper, which reads as one label run on into the
        // next. The default 1.5 mm keeps every other one; asked for no
        // spacing, each fits.
        let viewport = gridded(GridStyle::Lines, 50.0, 0.0);
        let forty_eight = |_: &str| 48.0;
        let mut close = PlanGridOptions {
            label_width_mm: Some(&forty_eight),
            ..PlanGridOptions::default()
        };
        let spaced = grid_with(&viewport, &close);
        assert_eq!(
            label_texts(&spaced, ViewportEdge::Bottom),
            vec!["E 305 200", "E 305 300"]
        );
        for (i, label) in spaced.labels.iter().enumerate() {
            for other in &spaced.labels[i + 1..] {
                assert!(
                    !overlap(
                        &label.box_value.inflated(close.label_spacing_mm),
                        &other.box_value
                    ),
                    "{} / {}",
                    label.text,
                    other.text
                );
            }
        }
        close.label_spacing_mm = 0.0;
        assert_eq!(
            label_texts(&grid_with(&viewport, &close), ViewportEdge::Bottom),
            vec!["E 305 200", "E 305 250", "E 305 300"]
        );
    }

    #[test]
    fn ticks_come_in_from_the_border_and_their_labels_stand_past_them() {
        let viewport = gridded(GridStyle::Ticks, 50.0, 0.0);
        let grid = grid_of(&viewport);
        assert_eq!(grid.lines.len(), 6);
        assert_eq!(grid.strokes.len(), 12);
        // E 305 150 at x 120: 2.5 mm up from the bottom, 2.5 mm down from the
        // top.
        assert_eq!(
            grid.strokes[0],
            GridSegment {
                from: Point2::new(120.0, 100.0),
                to: Point2::new(120.0, 102.5)
            }
        );
        near(grid.strokes[1].from.y, 200.0, 1e-9);
        near(grid.strokes[1].to.y, 197.5, 1e-9);
        for tick in &grid.strokes {
            assert!(on_border(tick.from, &viewport.rect));
            near((tick.to - tick.from).length(), 2.5, 1e-9);
            assert!(viewport.rect.contains(tick.to));
        }
        // 2.5 mm of tick and 0.8 mm of gap.
        for label in &grid.labels {
            if label.edge == ViewportEdge::Bottom {
                near(label.anchor.y, 103.3, 1e-9);
            }
            if label.edge == ViewportEdge::Right {
                near(label.anchor.x, 296.7, 1e-9);
            }
        }
    }

    #[test]
    fn crosses_mark_every_intersection_wholly_inside_the_viewport() {
        let viewport = gridded(GridStyle::Crosses, 50.0, 0.0);
        let grid = grid_of(&viewport);
        // Four eastings by two northings.
        assert_eq!(grid.crossings.len(), 8);
        assert_eq!(grid.strokes.len(), 16);
        near(grid.crossings[0].x, 120.0, 1e-6);
        near(grid.crossings[0].y, 140.0, 1e-6);
        near(grid.crossings[1].y, 190.0, 1e-6);
        // Each 3 mm across, along the grid.
        near(grid.strokes[0].from.x, 118.5, 1e-6);
        near(grid.strokes[0].to.x, 121.5, 1e-6);
        near(grid.strokes[1].from.y, 138.5, 1e-6);
        near(grid.strokes[1].to.y, 141.5, 1e-6);

        // An intersection 1 mm from the border is left out, not cut in half:
        // move the ground so E 305 150 falls at x 101.
        let mut close = viewport.clone();
        close.centre.x = 305150.0 + 99.0;
        let edge = grid_of(&close);
        for crossing in &edge.crossings {
            assert!(crossing.x >= 101.5);
        }
        assert_eq!(edge.crossings.len(), 6);
        // The line is there; its label, centred 1 mm from the edge, would
        // stick out past it, so the first label along the bottom is the next
        // line's.
        assert_eq!(edge.lines.len(), 6);
        near(edge.lines[0].from.x, 101.0, 1e-6);
        assert_eq!(edge.labels[0].text, "E 305 200");
    }

    #[test]
    fn a_rotated_grid_runs_askew_and_every_line_ends_on_the_border_at_its_coordinate() {
        let rotation = PI / 6.0; // 30 degrees
        let viewport = gridded(GridStyle::Lines, 20.0, rotation);
        let at = stored_placement(&viewport);
        let grid = grid_of(&viewport);
        assert!(!grid.lines.is_empty());

        // World north on the paper is 30 degrees clockwise of up.
        let north = Point2::new(rotation.sin(), rotation.cos());
        let mut eastings = 0;
        for line in &grid.lines {
            assert!(on_border(line.from, &viewport.rect));
            assert!(on_border(line.to, &viewport.rect));
            near(line.value.abs() % 20.0, 0.0, 1e-6);
            let a = plan_paper_to_world(&viewport, &at, line.from);
            let b = plan_paper_to_world(&viewport, &at, line.to);
            let easting = line.axis == GridAxis::Easting;
            near(if easting { a.x } else { a.y }, line.value, 1e-6);
            near(if easting { b.x } else { b.y }, line.value, 1e-6);
            let d = (line.to - line.from) * (1.0 / (line.to - line.from).length());
            if easting {
                eastings += 1;
                near((d.x * north.x + d.y * north.y).abs(), 1.0, 1e-9);
            } else {
                near((d.x * north.x + d.y * north.y).abs(), 0.0, 1e-9);
            }
        }
        // Every easting that crosses the viewport is there: one whose value
        // lies strictly between the ground under two of the rectangle's
        // corners.
        let corners = plan_footprint(&viewport, &at);
        let mut expected = 0;
        let mut e = (305000.0_f64 / 20.0).floor() * 20.0;
        while e < 305500.0 {
            let mut low = f64::INFINITY;
            let mut high = f64::NEG_INFINITY;
            for corner in &corners {
                let offset = corner.x - e;
                if offset < low {
                    low = offset;
                }
                if high < offset {
                    high = offset;
                }
            }
            if low < 0.0 && high > 0.0 {
                expected += 1;
            }
            e += 20.0;
        }
        assert_eq!(eastings, expected);

        // Every label is inside, clear of the others, and centred where its
        // line meets its edge.
        for (i, label) in grid.labels.iter().enumerate() {
            assert!(
                viewport.rect.contains_box(label.box_value),
                "{}",
                label.text
            );
            assert_eq!(
                &label.text[..2],
                if label.axis == GridAxis::Easting {
                    "E "
                } else {
                    "N "
                }
            );
            let line = grid
                .lines
                .iter()
                .find(|line| line.axis == label.axis && line.value == label.value)
                .expect("the label's line");
            let end = if line.from_edge == label.edge {
                line.from
            } else {
                line.to
            };
            let across = label.edge == ViewportEdge::Bottom || label.edge == ViewportEdge::Top;
            near(
                if across {
                    label.anchor.x
                } else {
                    label.anchor.y
                },
                if across { end.x } else { end.y },
                1e-9,
            );
            for other in &grid.labels[i + 1..] {
                assert!(!overlap(&label.box_value, &other.box_value));
            }
        }
        assert!(!grid.labels.is_empty());
    }

    #[test]
    fn an_interval_of_less_than_a_metre_writes_its_decimals() {
        let mut viewport = gridded(GridStyle::Ticks, 0.5, 0.0);
        viewport.scale = 10.0; // 0.5 m is 50 mm
        viewport.centre = Point2::new(305230.25, 6250410.0);
        let grid = grid_of(&viewport);
        assert_eq!(grid.decimals, 1);
        assert_eq!(
            label_texts(&grid, ViewportEdge::Top),
            vec!["E 305 229.5", "E 305 230.0", "E 305 230.5", "E 305 231.0"]
        );
    }

    #[test]
    fn an_automatic_grid_takes_the_round_spacing_for_the_scale() {
        let mut viewport = gridded(GridStyle::Crosses, 0.0, 0.0);
        viewport.scale = 500.0;
        let grid = grid_of(&viewport);
        double_eq(grid.interval, 20.0);
        // The same viewport drawn at an automatic scale of 1:2000.
        let rescaled = ok(compute_plan_grid(
            &viewport,
            &PlanPlacement {
                scale: 2000.0,
                centre: viewport.centre,
            },
            &PlanGridOptions::default(),
        ));
        double_eq(rescaled.interval, 100.0);
    }

    #[test]
    fn no_grid_is_empty_and_a_grid_that_cannot_be_drawn_is_refused_with_why() {
        let options = PlanGridOptions::default();
        let off = gridded(GridStyle::None, 0.0, 0.0);
        let none = ok(compute_plan_grid(&off, &stored_placement(&off), &options));
        assert_eq!(none, PlanGrid::default());

        // 1 m at 1:10000 is 0.1 mm apart.
        let mut dense = gridded(GridStyle::Lines, 1.0, 0.0);
        dense.scale = 10000.0;
        let refused =
            compute_plan_grid(&dense, &stored_placement(&dense), &options).expect_err("too dense");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
        assert!(
            refused
                .message
                .contains("1 m grid at 1:10000 is 0.1 mm apart"),
            "{}",
            refused.message
        );

        let mut unplaced = gridded(GridStyle::Lines, 50.0, 0.0);
        unplaced.rect = Box2::default();
        assert_eq!(
            compute_plan_grid(&unplaced, &stored_placement(&unplaced), &options)
                .expect_err("unplaced")
                .code,
            ErrorCode::InvalidArgument
        );
        let lines = gridded(GridStyle::Lines, 50.0, 0.0);
        let placed_at = |scale: f64| PlanPlacement {
            scale,
            centre: lines.centre,
        };
        assert!(compute_plan_grid(&lines, &placed_at(0.0), &options).is_err());
        assert!(compute_plan_grid(&lines, &placed_at(f64::INFINITY), &options).is_err());
        let negative = gridded(GridStyle::Lines, -5.0, 0.0);
        assert!(compute_plan_grid(&negative, &stored_placement(&negative), &options).is_err());
        let nan = gridded(GridStyle::Lines, f64::NAN, 0.0);
        assert!(compute_plan_grid(&nan, &stored_placement(&nan), &options).is_err());
    }

    #[test]
    fn the_same_viewport_always_gives_the_same_grid() {
        let viewport = gridded(GridStyle::Ticks, 25.0, 0.4);
        assert_eq!(grid_of(&viewport), grid_of(&viewport));
    }

    #[test]
    fn the_grid_is_stored_in_the_sheets_json_only_when_it_is_not_the_default() {
        let mut plain = gridded(GridStyle::None, 0.0, 0.0);
        plain.id = String::from("vp2");
        let mut set = SheetSet::default();
        set.sheets.push(Sheet {
            id: String::from("s1"),
            viewports: vec![gridded(GridStyle::Crosses, 25.0, 0.0), plain],
            ..Sheet::default()
        });

        let json = ok(sheet_set_to_json(&set));
        assert!(json.contains("\"grid_style\":\"crosses\""), "{json}");
        assert!(json.contains("\"grid_interval\":25"), "{json}");
        // Once each: the plain viewport writes neither.
        assert_eq!(json.matches("grid_style").count(), 1);
        assert_eq!(json.matches("grid_interval").count(), 1);
        assert_eq!(ok(sheet_set_from_json(&json)), set);

        for style in [GridStyle::Ticks, GridStyle::Lines] {
            set.sheets[0].viewports[0].grid_style = style;
            set.sheets[0].viewports[0].grid_interval = 0.25;
            let again = ok(sheet_set_from_json(&ok(sheet_set_to_json(&set))));
            assert_eq!(again, set);
        }

        let unknown = sheet_set_from_json(
            r#"{"format": "katana-sheets", "version": 1, "sheets": [{"id": "s1", "viewports":
            [{"id": "vp1", "kind": "plan", "grid_style": "dots"}]}]}"#,
        )
        .expect_err("no such grid style");
        assert_eq!(unknown.code, ErrorCode::ParseFailure);
        assert!(unknown.context.contains("dots"));
    }

    #[test]
    fn setting_a_plans_grid_is_one_undoable_step() {
        let mut document = Document::default();
        let sheet = Sheet {
            id: String::from("s1"),
            viewports: vec![
                gridded(GridStyle::None, 0.0, 0.0),
                Viewport {
                    id: String::from("vp2"),
                    kind: ViewportKind::Legend,
                    ..Viewport::default()
                },
            ],
            ..Sheet::default()
        };
        ok(add_sheet(&mut document, sheet, None));
        let viewport = |document: &Document| document.sheet_set().sheets[0].viewports[0].clone();
        let before = document.history().undo_count();

        ok(set_plan_grid(&mut document, "vp1", GridStyle::Lines, 25.0));
        assert_eq!(document.history().undo_count(), before + 1);
        assert_eq!(viewport(&document).grid_style, GridStyle::Lines);
        double_eq(viewport(&document).grid_interval, 25.0);
        // The same again records nothing.
        ok(set_plan_grid(&mut document, "vp1", GridStyle::Lines, 25.0));
        assert_eq!(document.history().undo_count(), before + 1);

        // Refused, and nothing changes.
        assert_eq!(
            set_plan_grid(&mut document, "vp2", GridStyle::Lines, 0.0)
                .expect_err("a legend has no grid")
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            set_plan_grid(&mut document, "vp9", GridStyle::Lines, 0.0)
                .expect_err("no viewport vp9")
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            set_plan_grid(&mut document, "vp1", GridStyle::Ticks, -1.0)
                .expect_err("a negative interval")
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            set_plan_grid(&mut document, "vp1", GridStyle::Ticks, f64::INFINITY)
                .expect_err("an infinite interval")
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(document.history().undo_count(), before + 1);
        assert_eq!(viewport(&document).grid_style, GridStyle::Lines);

        ok(document.undo());
        assert_eq!(viewport(&document).grid_style, GridStyle::None);
        double_eq(viewport(&document).grid_interval, 0.0);
    }
}
