//! The twins of tests/qt_widgets/plotting/test_plan_grid_painter.cpp: a plan's coordinate grid and
//! the live key plan as the sheet painter draws them (docs/plotting.md, "The coordinate grid and
//! the live key plan"). Painted on a sheet without a frame, so nothing but the viewports is on it,
//! and read back pixel by pixel; what the grid and the key plan decide is cad's, tested there.

use katana_cad::plotting::key_plan::{
    fit_key_plan, fit_plan_placement, key_plan_outlines, model_placer, place_plan,
    place_plan_on_sheet,
};
use katana_cad::plotting::plan_grid::{
    GridAxis, PlanGridOptions, PlanPlacement, plan_grid, plan_world_to_paper, stored_placement,
};
use katana_cad::plotting::sheet_set::{
    GridStyle, Sheet, SheetSet, Viewport, ViewportKind, WorldMark, WorldMarkKind, drawing_area,
};
use katana_entity::{Alignment, Entity, Geometry, Layer, Model};
use katana_geometry::{AlignmentPi, Box2, HorizontalAlignment, Point2, Segment2};
use katana_math::numerics::PI;
use katana_window::canvas::{Color, Image, IntRect};
use katana_window::plan_painter::PlanSource;
use katana_window::sheet_painter::{
    ResolvedViewport, SheetPaintStats, SheetSource, resolve_plan_viewport,
    resolve_plan_viewport_on_sheet,
};

use crate::support::{PPMM, Painting, WHITE, add_line, bx, near, painted_with, pixels_in};

/// A3 landscape.
const PAPER_HEIGHT: f64 = 297.0;

/// An A3 sheet without a frame, so nothing but the viewports is on it.
fn bare(id: &str, viewports: Vec<Viewport>) -> Sheet {
    Sheet {
        id: String::from(id),
        name: String::from(id),
        frame: String::new(),
        viewports,
        ..Sheet::default()
    }
}

fn plan(id: &str, rect: Box2, scale: f64, centre: Point2, rotation: f64) -> Viewport {
    Viewport {
        id: String::from(id),
        kind: ViewportKind::Plan,
        rect,
        scale,
        centre,
        rotation,
        ..Viewport::default()
    }
}

fn key_plan(id: &str, rect: Box2, scale: f64, centre: Point2) -> Viewport {
    Viewport {
        kind: ViewportKind::KeyPlan,
        ..plan(id, rect, scale, centre, 0.0)
    }
}

/// The 200 x 100 mm plan the grid tests draw: 1:1000 over (305 230, 6 250 410), so E 305 150 ...
/// 305 300 fall at x 120 ... 270 and N 6 250 400 and 6 250 450 at y 140 and 190.
fn gridded(style: GridStyle, interval: f64, rotation: f64) -> Viewport {
    Viewport {
        grid_style: style,
        grid_interval: interval,
        ..plan(
            "vp1",
            bx(100.0, 100.0, 300.0, 200.0),
            1000.0,
            Point2::new(305_230.0, 6_250_410.0),
            rotation,
        )
    }
}

fn source_of(model: &Model) -> SheetSource<'_> {
    SheetSource {
        plan: PlanSource {
            model: Some(model),
            ..PlanSource::default()
        },
        ..SheetSource::default()
    }
}

fn painted_at(
    set: &SheetSet,
    index: usize,
    source: &SheetSource<'_>,
    name: &str,
) -> (Image, SheetPaintStats) {
    painted_with(
        set,
        source,
        &Painting {
            index,
            ..Painting::named(name)
        },
    )
}

fn painted_sheet(sheet: Sheet, source: &SheetSource<'_>, name: &str) -> (Image, SheetPaintStats) {
    let set = SheetSet {
        sheets: vec![sheet],
        ..SheetSet::default()
    };
    painted_at(&set, 0, source, name)
}

/// Pixels in the paper box that are not white (alpha aside, as QColor::rgb() compares).
fn ink_in(image: &Image, paper: &Box2) -> usize {
    pixels_in(image, paper, |c| {
        (c.r, c.g, c.b) != (WHITE.r, WHITE.g, WHITE.b)
    })
}

/// The darkest pixel in the box: 255 for white paper, 0 for black ink.
fn darkest_in(image: &Image, paper: &Box2) -> u8 {
    let darkest = std::cell::Cell::new(255_u8);
    pixels_in(image, paper, |c| {
        darkest.set(darkest.get().min(c.r).min(c.g).min(c.b));
        false
    });
    darkest.get()
}

/// Pixels whose colour matches `test`.
fn count_in(image: &Image, paper: &Box2, test: impl Fn(Color) -> bool) -> usize {
    pixels_in(image, paper, test)
}

/// A key plan's red, for the other sheets' outlines and numbers.
fn reddish(c: Color) -> bool {
    i32::from(c.r) - i32::from(c.g) > 70 && i32::from(c.r) - i32::from(c.b) > 60
}

/// Its blue, for "you are here".
fn bluish(c: Color) -> bool {
    i32::from(c.b) - i32::from(c.r) > 50
}

/// Whether two images agree on every pixel of the paper box; where they first differ, in paper
/// millimetres, when they do not.
fn same_in(a: &Image, b: &Image, paper: &Box2) -> Result<(), String> {
    let x0 = (paper.min.x * PPMM).floor() as u32;
    let x1 = (paper.max.x * PPMM).floor() as u32;
    let y0 = ((PAPER_HEIGHT - paper.max.y) * PPMM).floor() as u32;
    let y1 = ((PAPER_HEIGHT - paper.min.y) * PPMM).floor() as u32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            if a.pixel(x, y) != b.pixel(x, y) {
                return Err(format!(
                    "they differ at ({}, {}) mm: {} and {}",
                    (f64::from(x) + 0.5) / PPMM,
                    PAPER_HEIGHT - (f64::from(y) + 0.5) / PPMM,
                    a.pixel(x, y).name(),
                    b.pixel(x, y).name()
                ));
            }
        }
    }
    Ok(())
}

/// A key-plan sheet and two plans side by side, 100 x 50 m each at 1:1000: sheet 2 over E
/// 950..1050, sheet 3 over E 1050..1150, both N 1975..2025. The key plan, 200 x 150 mm at 1:2000
/// over (1050, 2000), shows sheet 2's outline at x 150..200, y 162.5..187.5, and sheet 3's at x
/// 200..250.
fn key_and_two_plans() -> SheetSet {
    let on_paper = bx(50.0, 50.0, 150.0, 100.0);
    SheetSet {
        sheets: vec![
            bare(
                "s1",
                vec![key_plan(
                    "vp1",
                    bx(100.0, 100.0, 300.0, 250.0),
                    2000.0,
                    Point2::new(1050.0, 2000.0),
                )],
            ),
            bare(
                "s2",
                vec![plan(
                    "vp2",
                    on_paper,
                    1000.0,
                    Point2::new(1000.0, 2000.0),
                    0.0,
                )],
            ),
            bare(
                "s3",
                vec![plan(
                    "vp3",
                    on_paper,
                    1000.0,
                    Point2::new(1100.0, 2000.0),
                    0.0,
                )],
            ),
        ],
        ..SheetSet::default()
    }
}

fn placement(at: &ResolvedViewport) -> PlanPlacement {
    PlanPlacement {
        scale: at.scale,
        centre: at.centre,
    }
}

fn pi_at(x: f64, y: f64, radius: f64) -> AlignmentPi {
    AlignmentPi {
        point: Point2::new(x, y),
        radius,
        ..AlignmentPi::default()
    }
}

fn add_road(model: &mut Model, pis: Vec<AlignmentPi>) {
    model
        .alignments
        .add(Alignment {
            name: String::from("ROAD"),
            horizontal: HorizontalAlignment {
                pis,
                ..HorizontalAlignment::default()
            },
            ..Alignment::default()
        })
        .expect("the road is added");
}

// ---- the coordinate grid ------------------------------------------------------------------------

mod sheet_grid {
    use super::*;

    #[test]
    fn lines_are_faint_grey_at_their_coordinates_and_labelled_at_the_edges() {
        let model = Model::new();
        let source = source_of(&model);
        let (lines, stats) = painted_sheet(
            bare("s1", vec![gridded(GridStyle::Lines, 50.0, 0.0)]),
            &source,
            "grid_lines",
        );
        assert!(stats.problems.is_empty(), "{:?}", stats.problems);
        assert_eq!(stats.viewports_drawn, 1);

        // E 305 200 runs up x 170 and N 6 250 400 across y 140: light grey, a 0.13 mm hairline,
        // never black.
        assert!(ink_in(&lines, &bx(169.5, 150.0, 170.5, 185.0)) > 30);
        assert!(darkest_in(&lines, &bx(169.5, 150.0, 170.5, 185.0)) >= 180);
        assert!(ink_in(&lines, &bx(230.0, 139.5, 260.0, 140.5)) > 30);
        // Nothing between the lines.
        assert_eq!(ink_in(&lines, &bx(140.0, 150.0, 150.0, 185.0)), 0);

        // Its labels, in ink: "E 305 200" along the bottom, centred on x 170, and "N 6 250 400" up
        // the left side, centred on y 140.
        assert!(darkest_in(&lines, &bx(165.0, 100.9, 175.0, 102.5)) < 100);
        assert!(darkest_in(&lines, &bx(100.9, 135.0, 102.5, 145.0)) < 100);
        // Up the right side and along the top too.
        assert!(darkest_in(&lines, &bx(297.5, 135.0, 299.1, 145.0)) < 100);
        assert!(darkest_in(&lines, &bx(165.0, 197.5, 175.0, 199.1)) < 100);

        // No grid: none of it.
        let (none, _) = painted_sheet(
            bare("s1", vec![gridded(GridStyle::None, 50.0, 0.0)]),
            &source,
            "grid_lines_none",
        );
        assert_eq!(ink_in(&none, &bx(169.5, 150.0, 170.5, 185.0)), 0);
        assert_eq!(ink_in(&none, &bx(165.0, 100.9, 175.0, 102.5)), 0);
        assert_eq!(ink_in(&none, &bx(100.9, 135.0, 102.5, 145.0)), 0);
    }

    #[test]
    fn crosses_mark_the_intersections_and_ticks_only_the_border() {
        let model = Model::new();
        let source = source_of(&model);
        let (crosses, _) = painted_sheet(
            bare("s1", vec![gridded(GridStyle::Crosses, 50.0, 0.0)]),
            &source,
            "grid_crosses",
        );
        // A 3 mm cross at (170, 140): both arms, in ink.
        assert!(ink_in(&crosses, &bx(168.7, 139.6, 169.6, 140.4)) > 0);
        assert!(ink_in(&crosses, &bx(169.6, 138.7, 170.4, 139.6)) > 0);
        assert!(darkest_in(&crosses, &bx(168.5, 139.5, 171.5, 140.5)) < 180);
        // Nothing along the line between two crosses, nor past an arm's end.
        assert_eq!(ink_in(&crosses, &bx(169.0, 145.0, 171.0, 185.0)), 0);
        assert_eq!(ink_in(&crosses, &bx(172.0, 139.0, 180.0, 141.0)), 0);

        let (ticks, _) = painted_sheet(
            bare("s1", vec![gridded(GridStyle::Ticks, 50.0, 0.0)]),
            &source,
            "grid_crosses_ticks",
        );
        // A 2.5 mm tick in from the bottom border at x 170, and from the top.
        assert!(ink_in(&ticks, &bx(169.6, 100.3, 170.4, 102.2)) > 0);
        assert!(ink_in(&ticks, &bx(169.6, 197.8, 170.4, 199.7)) > 0);
        // Nothing inside: no line, no cross.
        assert_eq!(ink_in(&ticks, &bx(168.0, 108.0, 172.0, 190.0)), 0);
        // Its label past the tick's end: 2.5 mm of tick and 0.8 of gap.
        assert!(darkest_in(&ticks, &bx(165.0, 103.4, 175.0, 105.0)) < 100);
    }

    #[test]
    fn a_rotated_grid_is_drawn_where_its_geometry_says() {
        let model = Model::new();
        let source = source_of(&model);
        let viewport = gridded(GridStyle::Lines, 20.0, PI / 6.0);
        let (image, _) = painted_sheet(bare("s1", vec![viewport.clone()]), &source, "grid_rotated");

        // Every line of the headless grid is on the paper halfway along it (the labels, knocked
        // out at its ends, are nowhere near its middle).
        let grid = plan_grid(
            &viewport,
            &stored_placement(&viewport),
            &PlanGridOptions::default(),
        )
        .expect("the grid is worked out");
        assert!(grid.lines.len() >= 10);
        for line in &grid.lines {
            let middle = (line.from + line.to) * 0.5;
            if (line.to - line.from).length() < 20.0 || middle.x < 140.0 {
                continue; // a corner's short line, or the title's corner
            }
            let around = bx(
                middle.x - 0.5,
                middle.y - 0.5,
                middle.x + 0.5,
                middle.y + 0.5,
            );
            assert!(
                ink_in(&image, &around) > 0,
                "{} {}",
                if line.axis == GridAxis::Easting {
                    "E"
                } else {
                    "N"
                },
                line.value
            );
        }
        // The viewport's centre, (305 230, 6 250 410), is the middle of a 20 m square of the grid:
        // 10 mm from every line, however it is turned.
        assert_eq!(ink_in(&image, &bx(197.0, 147.0, 203.0, 153.0)), 0);
        // A square, not a line level across the paper: the paper's own axes through the centre
        // cross the grid askew, so the ink on them is in short runs.
        let mut run = 0;
        let mut longest = 0;
        let mut x = 150.0;
        while x <= 250.0 {
            run = if ink_in(&image, &bx(x, 150.0, x, 150.0)) > 0 {
                run + 1
            } else {
                0
            };
            longest = longest.max(run);
            x += 0.25;
        }
        assert!(longest <= 6, "{longest}");
    }

    #[test]
    fn a_grid_too_fine_to_read_is_reported_and_the_plan_still_drawn() {
        let mut model = Model::new();
        add_line(
            &mut model,
            Point2::new(305_200.0, 6_250_410.0),
            Point2::new(305_260.0, 6_250_410.0),
        );
        let source = source_of(&model);
        // 0.5 mm apart at 1:1000.
        let dense = gridded(GridStyle::Lines, 0.5, 0.0);
        let (image, stats) = painted_sheet(bare("s1", vec![dense]), &source, "grid_too_fine");
        assert_eq!(stats.problems.len(), 1);
        assert!(
            stats.problems[0].starts_with("vp1: a 0.5 m grid at 1:1000 is 0.5 mm apart"),
            "{}",
            stats.problems[0]
        );
        assert_eq!(stats.viewports_drawn, 1);
        // The drawing is there, and no grey smear over it.
        assert!(ink_in(&image, &bx(175.0, 149.5, 225.0, 150.5)) > 50);
        assert_eq!(ink_in(&image, &bx(150.0, 160.0, 250.0, 190.0)), 0);
    }

    #[test]
    fn labels_keep_off_the_scale_bar_title_and_north_arrow() {
        let model = Model::new();
        let source = source_of(&model);
        // Crosses, so nothing but a label inks the strip along the bottom. E 305 250 falls at x
        // 243, the left edge of the scale bar's knock-out (243..299 x 100.8..109.4 for its 50 mm
        // bar); its label, about 10 mm wide, would straddle that edge.
        let mut viewport = gridded(GridStyle::Crosses, 50.0, 0.0);
        viewport.centre.x = 305_207.0;
        viewport.scale_bar = true;
        viewport.north_arrow = true;
        let (furnished, _) = painted_sheet(
            bare("s1", vec![viewport.clone()]),
            &source,
            "grid_labels_keep_off",
        );
        assert_eq!(ink_in(&furnished, &bx(238.5, 100.9, 242.5, 102.5)), 0);
        // The label of E 305 200, clear of it all, is there.
        assert!(darkest_in(&furnished, &bx(188.0, 100.9, 198.0, 102.5)) < 100);

        // Without the scale bar the label is set.
        viewport.scale_bar = false;
        let (bare_bar, _) = painted_sheet(
            bare("s1", vec![viewport.clone()]),
            &source,
            "grid_labels_keep_off_no_scale_bar",
        );
        assert!(darkest_in(&bare_bar, &bx(238.5, 100.9, 242.5, 102.5)) < 100);

        // The title, the scale bar and the north arrow read exactly as they do with no grid at
        // all.
        viewport.scale_bar = true;
        viewport.grid_style = GridStyle::Lines;
        viewport.grid_interval = 10.0; // a line every 10 mm, through all three
        let (lined, _) = painted_sheet(
            bare("s1", vec![viewport.clone()]),
            &source,
            "grid_labels_keep_off_lines",
        );
        viewport.grid_style = GridStyle::None;
        let (plain, _) = painted_sheet(
            bare("s1", vec![viewport]),
            &source,
            "grid_labels_keep_off_plain",
        );
        // "PLAN 1:1000", the scale bar, the north arrow.
        same_in(&lined, &plain, &bx(102.3, 102.2, 115.0, 106.6)).expect("the title reads the same");
        same_in(&lined, &plain, &bx(244.0, 101.3, 298.0, 109.0))
            .expect("the scale bar reads the same");
        same_in(&lined, &plain, &bx(287.0, 185.0, 297.0, 199.0))
            .expect("the north arrow reads the same");
        assert!(same_in(&lined, &plain, &bx(120.0, 120.0, 280.0, 180.0)).is_err());
    }

    // ---- a whole set, framed, to look at --------------------------------------------------------

    #[test]
    fn a_framed_set_reads_cleanly_in_every_style_and_on_its_key_plan() {
        // Three sheets as they are plotted - in the built-in frame, over a road and a few lines -
        // each plan in another grid style, one turned 25 degrees, and a key plan beside the first.
        // With KATANA_SHEET_PNG set these are the pictures to look at; here every label is held
        // inside its viewport, clear of the others and of the furniture, and every sheet paints
        // without a problem.
        let mut model = Model::new();
        add_road(
            &mut model,
            vec![
                pi_at(305_000.0, 6_250_200.0, 0.0),
                pi_at(305_400.0, 6_250_300.0, 250.0),
                pi_at(305_700.0, 6_250_650.0, 0.0),
            ],
        );
        add_line(
            &mut model,
            Point2::new(305_050.0, 6_250_260.0),
            Point2::new(305_420.0, 6_250_350.0),
        );
        add_line(
            &mut model,
            Point2::new(305_420.0, 6_250_350.0),
            Point2::new(305_660.0, 6_250_640.0),
        );
        let source = source_of(&model);

        let mut set = SheetSet::default();
        let styles = [
            (GridStyle::Lines, 0.0),
            (GridStyle::Crosses, 0.0),
            (GridStyle::Ticks, 0.4363),
        ];
        for (i, &(style, rotation)) in styles.iter().enumerate() {
            let id = format!("s{}", i + 1);
            let mut sheet = Sheet {
                id: id.clone(),
                name: id,
                ..Sheet::default()
            };
            let area = drawing_area(&sheet);
            assert!(!area.empty());
            let split = if i == 0 {
                area.min.x + 0.68 * area.width()
            } else {
                area.max.x
            };
            let step = i as f64;
            let mut view = plan(
                &format!("vp{}", i + 1),
                bx(area.min.x, area.min.y, split, area.max.y),
                1000.0,
                Point2::new(305_180.0 + 220.0 * step, 6_250_280.0 + 150.0 * step),
                rotation,
            );
            view.grid_style = style;
            view.north_arrow = true;
            view.scale_bar = true;
            sheet.viewports.push(view);
            if i == 0 {
                let mut key = key_plan(
                    "vpk",
                    bx(split + 4.0, area.max.y - 90.0, area.max.x, area.max.y),
                    5000.0,
                    Point2::default(),
                );
                key.auto_scale = true;
                key.auto_centre = true;
                key.north_arrow = true;
                key.grid_style = GridStyle::Ticks;
                sheet.viewports.push(key);
            }
            set.sheets.push(sheet);
        }

        for index in 0..set.sheets.len() {
            let (_, stats) = painted_at(
                &set,
                index,
                &source,
                &format!("grid_framed_set_sheet{}", index + 1),
            );
            assert!(stats.problems.is_empty(), "{:?}", stats.problems);
            for viewport in &set.sheets[index].viewports {
                let at = resolve_plan_viewport_on_sheet(viewport, &source, &set, index);
                let grid = plan_grid(viewport, &placement(&at), &PlanGridOptions::default())
                    .unwrap_or_else(|error| panic!("{}", error.message));
                assert!(!grid.labels.is_empty(), "{}", viewport.id);
                for a in 0..grid.labels.len() {
                    assert!(
                        viewport.rect.contains_box(grid.labels[a].box_value),
                        "{}",
                        grid.labels[a].text
                    );
                    for b in a + 1..grid.labels.len() {
                        let p = &grid.labels[a].box_value;
                        let q = &grid.labels[b].box_value;
                        assert!(
                            p.max.x <= q.min.x
                                || q.max.x <= p.min.x
                                || p.max.y <= q.min.y
                                || q.max.y <= p.min.y,
                            "{} / {}",
                            grid.labels[a].text,
                            grid.labels[b].text
                        );
                    }
                }
            }
        }
    }
}

// ---- the live key plan --------------------------------------------------------------------------

mod sheet_key_plan {
    use super::*;

    #[test]
    fn the_other_sheets_are_outlined_as_they_are_now() {
        let model = Model::new();
        let source = source_of(&model);
        let mut set = key_and_two_plans();
        let (before, _) = painted_at(&set, 0, &source, "key_plan_outlined");
        // Sheet 2's outline: its left side at x 150, its number in its middle.
        assert!(count_in(&before, &bx(149.5, 170.0, 150.5, 180.0), reddish) > 10);
        assert!(count_in(&before, &bx(170.0, 170.0, 180.0, 180.0), reddish) > 5);
        // Sheet 3's, beside it.
        assert!(count_in(&before, &bx(249.5, 170.0, 250.5, 180.0), reddish) > 10);
        assert!(count_in(&before, &bx(220.0, 170.0, 230.0, 180.0), reddish) > 5);

        // Sheet 3's plan panned 100 m north: its outline follows at once, 50 mm up the key plan.
        // Nothing stored had to be brought up to date.
        set.sheets[2].viewports[0].centre = Point2::new(1100.0, 2100.0);
        let (after, _) = painted_at(&set, 0, &source, "key_plan_outlined_panned");
        assert_eq!(
            count_in(&after, &bx(249.5, 170.0, 250.5, 180.0), reddish),
            0
        );
        assert_eq!(
            count_in(&after, &bx(220.0, 170.0, 230.0, 180.0), reddish),
            0
        );
        assert!(count_in(&after, &bx(249.5, 220.0, 250.5, 230.0), reddish) > 10);
        assert!(count_in(&after, &bx(220.0, 220.0, 230.0, 230.0), reddish) > 5);
        // Sheet 2's stays.
        assert!(count_in(&after, &bx(149.5, 170.0, 150.5, 180.0), reddish) > 10);

        // A sheet removed is gone from it.
        set.sheets.remove(2);
        let (removed, _) = painted_at(&set, 0, &source, "key_plan_outlined_removed");
        assert_eq!(
            count_in(&removed, &bx(249.5, 220.0, 250.5, 230.0), reddish),
            0
        );
        assert!(count_in(&removed, &bx(149.5, 170.0, 150.5, 180.0), reddish) > 10);
    }

    #[test]
    fn the_number_is_the_sheets_number_now() {
        let model = Model::new();
        let source = source_of(&model);
        let mut set = key_and_two_plans();
        // The number over sheet 3's outline, cut out of two paints: "3" as the set is, and "2"
        // once sheet 3 is moved ahead of sheet 2. The outline itself does not move.
        let number = bx(218.0, 170.0, 232.0, 180.0);
        let (three, _) = painted_at(&set, 0, &source, "key_plan_number");
        set.sheets.swap(1, 2);
        let (two, _) = painted_at(&set, 0, &source, "key_plan_number_reordered");
        assert!(same_in(&three, &two, &number).is_err());
        same_in(&three, &two, &bx(249.3, 165.0, 250.7, 185.0)).expect("the outline does not move");
        // And it is the "2" sheet 2's outline had before: the same pixels, 50 mm along (a stray
        // edge pixel of antialiasing allowed).
        let cut = |image: &Image, at: &Box2| {
            image.copy(IntRect::new(
                (at.min.x * PPMM).round() as i32,
                ((PAPER_HEIGHT - at.max.y) * PPMM).round() as i32,
                (at.width() * PPMM).round() as i32,
                (at.height() * PPMM).round() as i32,
            ))
        };
        let now = cut(&two, &number);
        let then = cut(&three, &bx(168.0, 170.0, 182.0, 180.0));
        assert_eq!((now.width(), now.height()), (then.width(), then.height()));
        let mut differing = 0;
        let mut red = 0;
        for y in 0..now.height() {
            for x in 0..now.width() {
                let a = now.pixel(x, y);
                let b = then.pixel(x, y);
                if a.r.abs_diff(b.r) > 16 || a.g.abs_diff(b.g) > 16 || a.b.abs_diff(b.b) > 16 {
                    differing += 1;
                }
                if reddish(a) {
                    red += 1;
                }
            }
        }
        assert!(red > 20, "{red}");
        assert!(differing <= 4, "{differing}");
    }

    #[test]
    fn its_own_sheets_plan_is_shaded_as_you_are_here() {
        let model = Model::new();
        let source = source_of(&model);
        let mut set = key_and_two_plans();
        // Sheet 2 gets a key plan inset of its own, beside its plan: 1:2000 over (1050, 2000) in
        // 200..400 x 100..250, so its own plan is outlined at x 250..300, y 162.5..187.5, and
        // sheet 3's at x 300..350.
        set.sheets[1].viewports.push(key_plan(
            "vp4",
            bx(200.0, 100.0, 400.0, 250.0),
            2000.0,
            Point2::new(1050.0, 2000.0),
        ));
        let (here, _) = painted_at(&set, 1, &source, "key_plan_here");
        assert!(count_in(&here, &bx(255.0, 165.0, 265.0, 170.0), bluish) > 30 * 16);
        assert_eq!(count_in(&here, &bx(330.0, 165.0, 340.0, 170.0), bluish), 0);
        assert!(count_in(&here, &bx(349.5, 170.0, 350.5, 180.0), reddish) > 10);

        // From the key-plan sheet, sheet 2's plan is just another outline.
        let (elsewhere, _) = painted_at(&set, 0, &source, "key_plan_here_key_sheet");
        assert_eq!(
            count_in(&elsewhere, &bx(155.0, 165.0, 165.0, 170.0), bluish),
            0
        );
        assert!(count_in(&elsewhere, &bx(149.5, 170.0, 150.5, 180.0), reddish) > 10);
    }

    #[test]
    fn stored_outlines_are_not_drawn_but_its_other_marks_are() {
        let model = Model::new();
        let source = source_of(&model);
        let mut set = key_and_two_plans();
        let key = &mut set.sheets[0].viewports[0];
        // An outline stored when the key plan was made, of where sheet 3 was then (20 x 10 mm
        // around (200, 205) on the paper), and a match line.
        key.marks.push(WorldMark {
            kind: WorldMarkKind::SheetOutline,
            sheet: String::from("s3"),
            points: vec![
                Point2::new(1030.0, 2050.0),
                Point2::new(1070.0, 2050.0),
                Point2::new(1070.0, 2070.0),
                Point2::new(1030.0, 2070.0),
            ],
            ..WorldMark::default()
        });
        key.marks.push(WorldMark {
            points: vec![Point2::new(990.0, 1930.0), Point2::new(1110.0, 1930.0)],
            label: String::from("MATCH LINE"),
            ..WorldMark::default()
        });

        let (image, _) = painted_at(&set, 0, &source, "key_plan_stored_outlines");
        assert_eq!(
            count_in(&image, &bx(185.0, 195.0, 215.0, 215.0), reddish),
            0
        );
        // The match line, dash-dot in ink at y 140 from x 170 to 230.
        assert!(darkest_in(&image, &bx(175.0, 139.5, 225.0, 140.5)) < 100);
    }

    #[test]
    fn an_automatic_key_plan_frames_every_sheet_not_the_drawing() {
        let mut model = Model::new();
        add_line(&mut model, Point2::new(0.0, 0.0), Point2::new(10.0, 10.0));
        let source = source_of(&model);
        let mut set = key_and_two_plans();
        set.sheets[0].viewports[0].auto_scale = true;
        set.sheets[0].viewports[0].auto_centre = true;
        let key = set.sheets[0].viewports[0].clone();

        // On the drawing alone it would show the line at the origin and neither sheet; with the
        // set it frames both sheets, and the line - far off, a kilometre and more away - is left
        // out rather than shrinking them.
        let drawing_only = resolve_plan_viewport(&key, &source);
        let with_sheets = resolve_plan_viewport_on_sheet(&key, &source, &set, 0);
        let outlines = key_plan_outlines(&set, 0, None);
        assert_eq!(outlines.len(), 2);
        let inside = |at: &ResolvedViewport, world: Point2| {
            key.rect
                .contains(plan_world_to_paper(&key, &placement(at), world))
        };
        for outline in &outlines {
            for &corner in &outline.corners {
                assert!(!inside(&drawing_only, corner));
                assert!(inside(&with_sheets, corner));
            }
        }
        assert!(!inside(&with_sheets, Point2::new(0.0, 0.0)));
        assert_eq!(
            with_sheets.scale,
            fit_key_plan(&key, &outlines, &Box2::default()).scale
        );
        // A set with no plan to frame falls back to the drawing.
        let lonely = SheetSet {
            sheets: vec![set.sheets[0].clone()],
            ..SheetSet::default()
        };
        let no_sheets = resolve_plan_viewport_on_sheet(&key, &source, &lonely, 0);
        assert_eq!(no_sheets.scale, drawing_only.scale);
        assert_eq!(no_sheets.centre, drawing_only.centre);
        // A plan is fitted as before: the set changes nothing for it.
        let mut automatic = set.sheets[1].viewports[0].clone();
        automatic.auto_scale = true;
        automatic.auto_centre = true;
        let alone = resolve_plan_viewport(&automatic, &source);
        let in_set = resolve_plan_viewport_on_sheet(&automatic, &source, &set, 1);
        assert_eq!(alone.scale, in_set.scale);
        assert_eq!(alone.centre, in_set.centre);

        let (image, stats) = painted_at(&set, 0, &source, "key_plan_automatic");
        assert!(stats.problems.is_empty(), "{:?}", stats.problems);
        // Both outlines are on the paper, wherever the fit put them.
        let placed = placement(&with_sheets);
        for outline in &outlines {
            let a = plan_world_to_paper(&key, &placed, outline.corners[0]);
            let b = plan_world_to_paper(&key, &placed, outline.corners[1]);
            let middle = (a + b) * 0.5; // its bottom side's middle
            assert!(
                count_in(
                    &image,
                    &bx(
                        middle.x - 1.0,
                        middle.y - 0.6,
                        middle.x + 1.0,
                        middle.y + 0.6
                    ),
                    reddish
                ) > 0
            );
        }
    }

    #[test]
    fn placed_headless_over_the_model_every_viewport_is_where_the_painter_puts_it() {
        // place_plan (cad's key_plan) is what a command line without a window places plans with;
        // over a drawing of entities it must agree with the painter: a plan over the drawing less
        // a hidden layer, one along a stretch of an alignment, a fixed one, and an automatic key
        // plan in a set.
        let mut model = Model::new();
        model
            .layers
            .add(Layer {
                name: String::from("FAR"),
                ..Layer::default()
            })
            .expect("the layer is added");
        add_line(
            &mut model,
            Point2::new(100.0, 200.0),
            Point2::new(460.0, 330.0),
        );
        model
            .entities
            .add(Entity {
                geometry: Geometry::Line(Segment2::new(
                    Point2::new(9000.0, 9000.0),
                    Point2::new(9010.0, 9000.0),
                )),
                layer: String::from("FAR"),
                ..Entity::default()
            })
            .expect("the stray is added");
        add_road(
            &mut model,
            vec![
                pi_at(0.0, 0.0, 0.0),
                pi_at(300.0, 0.0, 80.0),
                pi_at(300.0, 400.0, 0.0),
            ],
        );
        let source = source_of(&model);

        let mut drawing = plan(
            "vp1",
            bx(40.0, 60.0, 290.0, 230.0),
            500.0,
            Point2::default(),
            0.3,
        );
        drawing.auto_scale = true;
        drawing.auto_centre = true;
        drawing.hidden_layers.hide("FAR");
        let mut along = plan(
            "vp2",
            bx(30.0, 40.0, 400.0, 150.0),
            500.0,
            Point2::default(),
            -0.2,
        );
        along.auto_centre = true;
        along.auto_scale = true;
        along.source.alignment = String::from("ROAD");
        along.source.chainage_from = 150.0;
        along.source.chainage_to = 420.0;
        let fixed = plan(
            "vp3",
            bx(30.0, 40.0, 130.0, 90.0),
            250.0,
            Point2::new(7.0, 8.0),
            0.0,
        );
        let mut key = key_plan("vp4", bx(300.0, 40.0, 400.0, 110.0), 1.0, Point2::default());
        key.auto_scale = true;
        key.auto_centre = true;
        let set = SheetSet {
            sheets: vec![
                bare("s1", vec![drawing.clone(), key.clone()]),
                bare("s2", vec![along]),
                bare("s3", vec![fixed.clone()]),
            ],
            ..SheetSet::default()
        };

        for (sheet, held) in set.sheets.iter().enumerate() {
            for viewport in &held.viewports {
                let painter = resolve_plan_viewport_on_sheet(viewport, &source, &set, sheet);
                let headless = place_plan_on_sheet(&model, &set, sheet, viewport);
                assert_eq!(painter.scale, headless.scale, "{}", viewport.id);
                assert!(
                    near(painter.centre.x, headless.centre.x, 1e-6),
                    "{}",
                    viewport.id
                );
                assert!(
                    near(painter.centre.y, headless.centre.y, 1e-6),
                    "{}",
                    viewport.id
                );
            }
        }
        // Hidden, the stray is not fitted; the key plan still takes in all three sheets, and the
        // fixed plan kept its own.
        assert!(place_plan(&model, &drawing).scale <= 5000.0);
        let mut shown = drawing;
        shown.hidden_layers.show("FAR");
        assert!(place_plan(&model, &shown).scale >= 50000.0);
        assert_eq!(place_plan(&model, &fixed), stored_placement(&fixed));
        let key_at = place_plan_on_sheet(&model, &set, 0, &key);
        let placer = model_placer(&model);
        for outline in key_plan_outlines(&set, 0, Some(&placer)) {
            for &corner in &outline.corners {
                assert!(
                    key.rect
                        .contains(plan_world_to_paper(&key, &key_at, corner)),
                    "{}",
                    outline.viewport_id
                );
            }
        }
    }

    #[test]
    fn an_automatic_plan_is_fitted_by_the_same_rule_headless_as_painted() {
        // fit_plan_placement (cad's key_plan) places the other sheets' automatic plans for a key
        // plan; it must agree with the painter's own fit, or an outline would not be where its
        // sheet draws.
        let mut model = Model::new();
        add_line(
            &mut model,
            Point2::new(100.0, 200.0),
            Point2::new(460.0, 330.0),
        );
        add_line(
            &mut model,
            Point2::new(120.0, 190.0),
            Point2::new(300.0, 420.0),
        );
        let source = source_of(&model);
        for rotation in [0.0, 0.4, -1.1] {
            let mut viewport = plan(
                "vp1",
                bx(40.0, 60.0, 290.0, 230.0),
                500.0,
                Point2::default(),
                rotation,
            );
            viewport.auto_scale = true;
            viewport.auto_centre = true;
            let painter = resolve_plan_viewport(&viewport, &source);
            let extent = [
                Point2::new(100.0, 190.0),
                Point2::new(460.0, 190.0),
                Point2::new(460.0, 420.0),
                Point2::new(100.0, 420.0),
            ];
            let headless = fit_plan_placement(&viewport, &extent);
            assert_eq!(painter.scale, headless.scale, "{rotation}");
            assert!(
                near(painter.centre.x, headless.centre.x, 1e-6),
                "{rotation}"
            );
            assert!(
                near(painter.centre.y, headless.centre.y, 1e-6),
                "{rotation}"
            );
        }
    }
}
