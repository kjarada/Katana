// The smart sheet generators (include/katana/cad/plotting/generators.hpp);
// tests/cad/plotting/test_generators.cpp.
//
// Paper sizes, scales and world positions are worked by hand in the comments
// from the A3 frame's tiling area - 24..409 x 36..286 mm, 385 x 250 mm - and
// the sheet scale ladder (1:250, 500, 750, 1000, 1250, 2000, 5000 ...). None
// is read off the code's output.

use std::collections::BTreeSet;
use std::f64::consts::PI;

use crate::{expect_box, expect_point, near, rect};
use katana_cad::PaperSize;
use katana_cad::SectionSurfaceInput;
use katana_cad::plotting::frame::BUILT_IN_FRAME_ID;
use katana_cad::plotting::generators::{
    cross_section_sheets, fit_to_sheet, grid_sheets, prepare_for_append, sheets_from_plot_frames,
    smart_layout, strip_sheets,
};
use katana_cad::plotting::sheet_set::{
    automatic_title, mark_label, next_sheet_id, next_viewport_id, scale_text, sheet_scale_text,
};
use katana_cad::plotting::{
    CrossSectionRequest, GridRequest, LayoutRequest, Sheet, SheetSet, SheetTemplate, StripRequest,
    Viewport, ViewportKind, WorldMark, WorldMarkKind,
};
use katana_core::ErrorCode;
use katana_entity::{Alignment, Entity, Geometry, Model, PropertyValue};
use katana_geometry::alignment::solve_alignment;
use katana_geometry::{
    AlignmentPi, Box2, HorizontalAlignment, Point2, Point3, Polyline2, SolvedAlignment, Vec2,
};
use katana_terrain::TinSurface;

const TIGHT: f64 = 1e-9;

fn pi(x: f64, y: f64, radius: f64) -> AlignmentPi {
    AlignmentPi {
        point: Point2::new(x, y),
        radius,
        ..AlignmentPi::default()
    }
}

fn alignment_through(pis: Vec<AlignmentPi>, start_station: f64) -> SolvedAlignment {
    solve_alignment(&HorizontalAlignment { pis, start_station })
        .unwrap_or_else(|error| panic!("{}", error.describe()))
}

// Every viewport id of `sheets`, which must all differ.
fn distinct_viewport_ids(sheets: &[Sheet]) -> usize {
    let all: Vec<&Viewport> = sheets
        .iter()
        .flat_map(|sheet| sheet.viewports.iter())
        .collect();
    let ids: BTreeSet<&str> = all.iter().map(|viewport| viewport.id.as_str()).collect();
    assert_eq!(ids.len(), all.len(), "two viewports share an id");
    ids.len()
}

fn set_of(sheets: &[Sheet]) -> SheetSet {
    SheetSet {
        sheets: sheets.to_vec(),
        ..SheetSet::default()
    }
}

fn ok<T>(result: katana_core::Result<T>) -> T {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn fitted(extent: Box2, paper: &SheetTemplate) -> Vec<Sheet> {
    ok(fit_to_sheet(&extent, paper))
}

// Something drawn at each of `points`, and nowhere else: a tile is drawn in
// when its closed box holds one of them.
fn drawn_at(points: Vec<Point2>) -> impl Fn(&Box2) -> bool {
    move |tile: &Box2| points.iter().any(|&point| tile.contains(point))
}

fn tile_request() -> GridRequest<'static> {
    GridRequest {
        area: rect(0.0, 0.0, 400.0, 200.0),
        scale: 500.0,
        overlap_m: 10.0,
        ..GridRequest::default()
    }
}

// What a plot_frame string in a .12da file says: its paper, scale, rotation
// (degrees), origin and margins (left, right, top, bottom).
#[derive(Clone, Copy)]
struct FrameFields {
    width: f64,
    height: f64,
    scale: f64,
    rotation: f64,
    origin: Point2,
    margins: [f64; 4],
}

// A plot frame entity with `outline` as its geometry and `fields` copied into
// its plot_frame.* properties, as the importer copies them.
fn frame_entity(fields: &FrameFields, outline: Vec<Point2>, layer: &str, name: &str) -> Entity {
    let mut entity = Entity {
        geometry: Geometry::Polyline(Polyline2::new(outline, true)),
        layer: String::from(layer),
        ..Entity::default()
    };
    let p = &mut entity.properties;
    let mut put = |key: &str, value: f64| {
        p.insert(String::from(key), PropertyValue::Real(value));
    };
    put("plot_frame.width", fields.width);
    put("plot_frame.height", fields.height);
    put("plot_frame.scale", fields.scale);
    put("plot_frame.rotation", fields.rotation);
    put("plot_frame.xorigin", fields.origin.x);
    put("plot_frame.yorigin", fields.origin.y);
    put("plot_frame.left_margin", fields.margins[0]);
    put("plot_frame.right_margin", fields.margins[1]);
    put("plot_frame.top_margin", fields.margins[2]);
    put("plot_frame.bottom_margin", fields.margins[3]);
    if !name.is_empty() {
        entity.metadata.insert(
            String::from("12d.name"),
            PropertyValue::Text(String::from(name)),
        );
    }
    entity
}

// The frame as the archive importer draws it (PlotFrame in the archive's
// import): the paper on the ground from its origin less the import's origin
// shift, width along the rotation, height up from it - while the properties
// keep the file's origin as it was.
fn imported_frame(fields: &FrameFields, layer: &str, name: &str, origin_shift: Vec2) -> Entity {
    let rotation = fields.rotation * PI / 180.0;
    let along = Vec2::new(rotation.cos(), rotation.sin());
    let up = Vec2::new(-along.y, along.x);
    let w = fields.width * fields.scale / 1000.0;
    let h = fields.height * fields.scale / 1000.0;
    let corner = fields.origin - origin_shift;
    frame_entity(
        fields,
        vec![
            corner,
            corner + along * w,
            corner + along * w + up * h,
            corner + up * h,
        ],
        layer,
        name,
    )
}

fn model_with_straight(length: f64) -> Model {
    let mut model = Model::default();
    let road = Alignment {
        name: String::from("ROAD"),
        horizontal: HorizontalAlignment {
            pis: vec![pi(0.0, 0.0, 0.0), pi(length, 0.0, 0.0)],
            start_station: 0.0,
        },
        ..Alignment::default()
    };
    ok(model.alignments.add(road));
    model
}

fn model_with_road() -> Model {
    model_with_straight(1000.0)
}

// The ground a plan viewport shows across its width, in metres.
fn metres_wide(plan: &Viewport) -> f64 {
    plan.rect.width() * plan.scale / 1000.0
}

fn mark(kind: WorldMarkKind, sheet: &str) -> WorldMark {
    WorldMark {
        kind,
        points: Vec::new(),
        label: String::new(),
        sheet: String::from(sheet),
    }
}

mod sheet_generators {
    use super::*;

    // ---- fit_to_sheet --------------------------------------------------------------------

    #[test]
    fn an_extent_is_fitted_at_the_largest_standard_scale_that_holds_it() {
        // 300 x 100 m in 385 x 250 mm needs 1 : max(300000 / 385 = 779.2,
        // 100000 / 250 = 400) - the next ladder step up is 1 : 1000.
        let sheets = fitted(rect(0.0, 0.0, 300.0, 100.0), &SheetTemplate::default());
        assert_eq!(sheets.len(), 1);
        let sheet = &sheets[0];
        assert_eq!(sheet.paper, PaperSize::A3);
        assert!(sheet.landscape);
        assert_eq!(sheet.frame, BUILT_IN_FRAME_ID);
        assert_eq!(sheet.id, "s1");
        assert_eq!(sheet.viewports.len(), 1);
        let plan = &sheet.viewports[0];
        assert_eq!(plan.id, "vp1");
        assert_eq!(plan.kind, ViewportKind::Plan);
        assert_eq!(plan.scale, 1000.0);
        expect_point(plan.centre, 150.0, 50.0, TIGHT);
        expect_box(&plan.rect, 24.0, 36.0, 409.0, 286.0);
        assert!(plan.north_arrow);
        assert!(plan.scale_bar);

        // 190 x 120 m: max(493.5, 480) - 1 : 500.
        assert_eq!(
            fitted(rect(0.0, 0.0, 190.0, 120.0), &SheetTemplate::default())[0].viewports[0].scale,
            500.0
        );
        // Exactly 385 x 250 m needs exactly 1 : 1000, which fits.
        assert_eq!(
            fitted(rect(0.0, 0.0, 385.0, 250.0), &SheetTemplate::default())[0].viewports[0].scale,
            1000.0
        );
    }

    #[test]
    fn a_fitted_line_along_an_axis_has_a_scale_and_a_point_is_refused() {
        // A line 100 m long and no height: 100000 / 385 = 259.7 - 1 : 500.
        let line = fitted(rect(0.0, 5.0, 100.0, 5.0), &SheetTemplate::default());
        assert_eq!(line[0].viewports[0].scale, 500.0);
        let point = fit_to_sheet(&rect(3.0, 4.0, 3.0, 4.0), &SheetTemplate::default());
        assert_eq!(
            point.expect_err("a point has no extent").code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            fit_to_sheet(&Box2::default(), &SheetTemplate::default())
                .expect_err("nothing to fit")
                .code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn a_fit_on_a1_uses_the_doubled_frame() {
        // A1's drawing area is 46..820 x 70..574 (twice A3's), tiled 772 x 502
        // inside the 1 mm inset: 300 x 100 m needs max(388.6, 199.2) - 1 : 500.
        let a1 = SheetTemplate {
            paper: PaperSize::A1,
            landscape: true,
            ..SheetTemplate::default()
        };
        let sheets = fitted(rect(0.0, 0.0, 300.0, 100.0), &a1);
        assert_eq!(sheets[0].paper, PaperSize::A1);
        assert_eq!(sheets[0].viewports[0].scale, 500.0);
        expect_box(&sheets[0].viewports[0].rect, 47.0, 71.0, 819.0, 573.0);
    }

    // ---- grid_sheets ---------------------------------------------------------------------

    #[test]
    fn grid_tiles_are_numbered_in_reading_order_after_the_key_plan() {
        // 400 x 200 m at 1 : 500 with 10 m of overlap. A tile is 385 x 0.5 =
        // 192.5 by 250 x 0.5 = 125 m, stepping 182.5 across and 115 down:
        // ceil((400 - 10) / 182.5) = 3 columns, ceil((200 - 10) / 115) = 2
        // rows. They cover 3 x 182.5 + 10 = 557.5 by 2 x 115 + 10 = 240 m,
        // centred on (200, 100): from x -78.75 and down from y 220.
        let sheets = ok(grid_sheets(&tile_request(), None));
        assert_eq!(sheets.len(), 7); // the key plan and six tiles
        assert_eq!(distinct_viewport_ids(&sheets), 7);
        for (i, sheet) in sheets.iter().enumerate() {
            assert_eq!(sheet.id, format!("s{}", i + 1));
        }

        // Top-left tile, sheet 2: x -78.75..113.75, y 95..220, centre (17.5,
        // 157.5).
        let first = &sheets[1].viewports[0];
        assert_eq!(sheets[1].name, "PLAN TILE 1");
        assert_eq!(first.scale, 500.0);
        expect_point(first.centre, 17.5, 157.5, TIGHT);
        // Top-right, sheet 4: from x -78.75 + 2 x 182.5 = 286.25, centre 382.5.
        expect_point(sheets[3].viewports[0].centre, 382.5, 157.5, TIGHT);
        // Bottom-left, sheet 5: down from y 220 - 115 = 105, centre 42.5.
        expect_point(sheets[4].viewports[0].centre, 17.5, 42.5, TIGHT);

        // The top-left tile has two neighbours: a match line down the middle
        // of the strip it shares with sheet 3 (x 113.75 - 5 = 108.75) and
        // across the middle of the strip it shares with sheet 5 (y 95 + 5 =
        // 100).
        assert_eq!(first.marks.len(), 2);
        let set = set_of(&sheets);
        let right = &first.marks[0];
        assert_eq!(right.kind, WorldMarkKind::MatchLine);
        assert_eq!(right.points.len(), 2);
        near(right.points[0].x, 108.75, 1e-9);
        near(right.points[1].x, 108.75, 1e-9);
        assert_eq!(mark_label(&set, right), "MATCH LINE - SEE SHEET 3");
        let below = &first.marks[1];
        near(below.points[0].y, 100.0, 1e-9);
        assert_eq!(mark_label(&set, below), "MATCH LINE - SEE SHEET 5");
        // The middle tile of the bottom row (sheet 6) has three: left, right,
        // up.
        assert_eq!(sheets[5].viewports[0].marks.len(), 3);
    }

    #[test]
    fn the_grids_key_plan_outlines_every_tile_at_a_scale_that_holds_them_all() {
        // The tiles span 557.5 x 240 m around (200, 100): 1 : max(557500 / 385
        // = 1448, 240000 / 250 = 960) - 1 : 2000.
        let mut request = tile_request();
        let sheets = ok(grid_sheets(&request, None));
        let key = &sheets[0];
        assert_eq!(key.name, "KEY PLAN");
        assert_eq!(sheet_scale_text(key), "1:2000");
        assert_eq!(key.viewports.len(), 1);
        let view = &key.viewports[0];
        assert_eq!(view.kind, ViewportKind::KeyPlan);
        assert_eq!(view.scale, 2000.0);
        expect_point(view.centre, 200.0, 100.0, TIGHT);
        assert_eq!(view.marks.len(), 6);
        let set = set_of(&sheets);
        for (i, outline) in view.marks.iter().enumerate() {
            assert_eq!(outline.kind, WorldMarkKind::SheetOutline);
            assert_eq!(outline.points.len(), 4);
            assert_eq!(mark_label(&set, outline), format!("{}", i + 2));
        }
        // Without a key plan the tiles are sheets 1 to 6, and a match line
        // still names its neighbour correctly.
        request.key_plan = false;
        let bare = ok(grid_sheets(&request, None));
        assert_eq!(bare.len(), 6);
        assert_eq!(
            mark_label(&set_of(&bare), &bare[0].viewports[0].marks[0]),
            "MATCH LINE - SEE SHEET 2"
        );
    }

    #[test]
    fn grid_tiles_with_nothing_drawn_in_them_are_left_out_and_the_rest_keep_their_places() {
        // The six tiles of the grid above: columns x -78.75..113.75,
        // 103.75..296.25 and 286.25..478.75; rows y 95..220 and -20..105.
        // Drawn at one point in the top-left, top-middle, bottom-middle and
        // bottom-right tiles, each in no overlap: the top-right and
        // bottom-left tiles have nothing drawn in them.
        let drawn = drawn_at(vec![
            Point2::new(0.0, 200.0),
            Point2::new(200.0, 200.0),
            Point2::new(200.0, 0.0),
            Point2::new(400.0, 0.0),
        ]);
        let request = GridRequest {
            drawn: Some(&drawn),
            ..tile_request()
        };
        let mut skipped = Vec::new();
        let sheets = ok(grid_sheets(&request, Some(&mut skipped)));
        assert_eq!(sheets.len(), 5); // the key plan and four tiles
        assert_eq!(skipped, vec!["2 of 6 tiles, with nothing drawn in them"]);
        for (i, sheet) in sheets.iter().enumerate() {
            assert_eq!(sheet.id, format!("s{}", i + 1));
        }
        // Numbered in reading order among those kept, each where it was on
        // the grid.
        assert_eq!(sheets[1].name, "PLAN TILE 1");
        assert_eq!(sheets[4].name, "PLAN TILE 4");
        expect_point(sheets[1].viewports[0].centre, 17.5, 157.5, TIGHT);
        expect_point(sheets[2].viewports[0].centre, 200.0, 157.5, TIGHT);
        expect_point(sheets[3].viewports[0].centre, 200.0, 42.5, TIGHT);
        expect_point(sheets[4].viewports[0].centre, 382.5, 42.5, TIGHT);

        // Match lines lead only to tiles kept. Top-left: to the top-middle
        // (sheet 3) at x 108.75, and none down to the bottom-left, left out.
        let set = set_of(&sheets);
        let top_left = &sheets[1].viewports[0].marks;
        assert_eq!(top_left.len(), 1);
        near(top_left[0].points[0].x, 108.75, 1e-9);
        assert_eq!(mark_label(&set, &top_left[0]), "MATCH LINE - SEE SHEET 3");
        // Top-middle: none right to the top-right, left out; left to sheet 2
        // at x 103.75 + 5 = 108.75 and down to the bottom-middle (sheet 4) at
        // y 100.
        let top_middle = &sheets[2].viewports[0].marks;
        assert_eq!(top_middle.len(), 2);
        near(top_middle[0].points[0].x, 108.75, 1e-9);
        assert_eq!(mark_label(&set, &top_middle[0]), "MATCH LINE - SEE SHEET 2");
        near(top_middle[1].points[0].y, 100.0, 1e-9);
        assert_eq!(mark_label(&set, &top_middle[1]), "MATCH LINE - SEE SHEET 4");
        // Bottom-middle: right to sheet 5 at x 296.25 - 5 = 291.25 and up to
        // sheet 3; none left. Bottom-right: left to sheet 4; none up.
        let bottom_middle = &sheets[3].viewports[0].marks;
        assert_eq!(bottom_middle.len(), 2);
        near(bottom_middle[0].points[0].x, 291.25, 1e-9);
        assert_eq!(
            mark_label(&set, &bottom_middle[0]),
            "MATCH LINE - SEE SHEET 5"
        );
        assert_eq!(
            mark_label(&set, &bottom_middle[1]),
            "MATCH LINE - SEE SHEET 3"
        );
        let bottom_right = &sheets[4].viewports[0].marks;
        assert_eq!(bottom_right.len(), 1);
        assert_eq!(
            mark_label(&set, &bottom_right[0]),
            "MATCH LINE - SEE SHEET 4"
        );

        // The key plan outlines the four kept, numbered 2 to 5.
        let key = &sheets[0].viewports[0];
        assert_eq!(key.marks.len(), 4);
        for (i, outline) in key.marks.iter().enumerate() {
            assert_eq!(mark_label(&set, outline), format!("{}", i + 2));
        }
    }

    #[test]
    fn the_grids_key_plan_frames_only_the_tiles_kept() {
        // Drawn in the top-left and top-middle tiles alone: they span x -78.75
        // to 296.25 and y 95 to 220, 375 x 125 m around (108.75, 157.5),
        // which needs 1 : max(375000 / 385 = 974, 125000 / 250 = 500) - 1 :
        // 1000, where all six tiles needed 1 : 2000.
        let drawn = drawn_at(vec![Point2::new(0.0, 200.0), Point2::new(200.0, 200.0)]);
        let request = GridRequest {
            drawn: Some(&drawn),
            ..tile_request()
        };
        let sheets = ok(grid_sheets(&request, None));
        assert_eq!(sheets.len(), 3);
        let key = &sheets[0].viewports[0];
        assert_eq!(key.scale, 1000.0);
        expect_point(key.centre, 108.75, 157.5, TIGHT);
        assert_eq!(key.marks.len(), 2);
    }

    #[test]
    fn a_grid_with_nothing_drawn_in_any_tile_is_refused_and_one_with_everything_drawn_is_whole() {
        let nothing = |_: &Box2| false;
        let request = GridRequest {
            drawn: Some(&nothing),
            ..tile_request()
        };
        let mut skipped = Vec::new();
        let none = grid_sheets(&request, Some(&mut skipped)).expect_err("nothing is drawn");
        assert_eq!(none.code, ErrorCode::NotFound);
        assert_eq!(none.message, "nothing is drawn in any tile over the area");
        assert_eq!(none.context, "6 tiles of 192.5 x 125 m");
        assert!(skipped.is_empty());

        // Drawn everywhere: the same sheets as with no test at all, and
        // nothing said to be left out.
        let everything = |_: &Box2| true;
        let request = GridRequest {
            drawn: Some(&everything),
            ..tile_request()
        };
        let every = ok(grid_sheets(&request, Some(&mut skipped)));
        assert_eq!(every, ok(grid_sheets(&tile_request(), None)));
        assert!(skipped.is_empty());
    }

    #[test]
    fn a_grid_refuses_an_overlap_as_large_as_a_tile_or_no_scale() {
        let mut request = GridRequest {
            overlap_m: 125.0, // a whole tile's height
            ..tile_request()
        };
        assert_eq!(
            grid_sheets(&request, None)
                .expect_err("an overlap of a tile")
                .code,
            ErrorCode::InvalidArgument
        );
        request.overlap_m = 0.0;
        request.scale = 0.0;
        assert_eq!(
            grid_sheets(&request, None).expect_err("no scale").code,
            ErrorCode::InvalidArgument
        );
        request.scale = 500.0;
        request.area = Box2::default();
        assert_eq!(
            grid_sheets(&request, None).expect_err("no area").code,
            ErrorCode::InvalidArgument
        );
    }

    // ---- strip_sheets --------------------------------------------------------------------

    #[test]
    fn strips_follow_a_straight_alignment_rotated_to_its_bearing() {
        // (0, 0) to (600, 800): 1000 m on a bearing whose direction is
        // atan2(800, 600). A strip at 1 : 500 is 192.5 x 125 m; with 20 m of
        // overlap each covers 172.5 m of chainage: 0, 172.5, 345, 517.5, 690,
        // 862.5 and the last to 1000 - six sheets.
        let road = alignment_through(vec![pi(0.0, 0.0, 0.0), pi(600.0, 800.0, 0.0)], 0.0);
        let request = StripRequest {
            scale: 500.0,
            overlap_m: 20.0,
            ..StripRequest::default()
        };
        let sheets = ok(strip_sheets(&road, "ROAD", &request));
        assert_eq!(sheets.len(), 6);
        assert_eq!(sheets[0].name, "ROAD CH 0.000 TO 172.500");
        assert_eq!(sheets[5].name, "ROAD CH 862.500 TO 1000.000");
        let bearing = 800.0_f64.atan2(600.0);
        for sheet in &sheets {
            assert_eq!(sheet.viewports.len(), 1);
            let plan = &sheet.viewports[0];
            near(plan.rotation, bearing, 1e-12);
            assert_eq!(plan.scale, 500.0);
            assert_eq!(plan.source.alignment, "ROAD");
        }
        // The first strip is centred on chainage 86.25: (0.6, 0.8) x 86.25.
        let first = &sheets[0].viewports[0];
        expect_point(first.centre, 51.75, 69.0, TIGHT);
        assert_eq!(first.source.chainage_from, 0.0);
        assert_eq!(first.source.chainage_to, 172.5);
        // The last on (862.5 + 1000) / 2 = 931.25: (558.75, 745).
        expect_point(sheets[5].viewports[0].centre, 558.75, 745.0, TIGHT);

        // One match line on the first sheet, square across the alignment at
        // CH 172.5 (the point (103.5, 138)), 62.5 m either side along the left
        // normal (-0.8, 0.6): from (153.5, 100.5) on the right to (53.5,
        // 175.5).
        assert_eq!(first.marks.len(), 1);
        let matching = &first.marks[0];
        expect_point(matching.points[0], 153.5, 100.5, TIGHT);
        expect_point(matching.points[1], 53.5, 175.5, TIGHT);
        assert_eq!(
            mark_label(&set_of(&sheets), matching),
            "MATCH LINE CH 172.500 - SEE SHEET 2"
        );
        // A middle sheet has one at each end.
        assert_eq!(sheets[2].viewports[0].marks.len(), 2);
    }

    #[test]
    fn a_strip_never_lets_a_curved_alignment_leave_its_viewport() {
        // Two 200 m tangents and a 100 m radius quarter circle between them:
        // the curve bends out of a 192.5 x 125 m strip unless the strip is
        // shortened. Checked as a property, at every metre of every strip, in
        // the strip's own frame: along within +-96.25 m, across within
        // +-62.5 m.
        let road = alignment_through(
            vec![
                pi(0.0, 0.0, 0.0),
                pi(300.0, 0.0, 100.0),
                pi(300.0, 300.0, 0.0),
            ],
            0.0,
        );
        let request = StripRequest {
            scale: 500.0,
            ..StripRequest::default()
        };
        let sheets = ok(strip_sheets(&road, "BEND", &request));
        // 200 + 50 pi + 200 = 557.1 m at no more than 192.5 m a strip: at
        // least three, and more where the curve shortened one.
        assert!(sheets.len() >= 3);
        let mut covered = road.start_station();
        for sheet in &sheets {
            let plan = &sheet.viewports[0];
            // Contiguous: each strip starts where the last ended.
            near(plan.source.chainage_from, covered, 1e-9);
            covered = plan.source.chainage_to;
            let c = plan.rotation.cos();
            let s = plan.rotation.sin();
            let mut station = plan.source.chainage_from;
            while station <= plan.source.chainage_to {
                let at = road.point_at_station(station).expect("on the road");
                let dx = at.x - plan.centre.x;
                let dy = at.y - plan.centre.y;
                assert!(
                    (dx * c + dy * s).abs() <= 96.25 + 1e-9,
                    "chainage {station}"
                );
                assert!(
                    (-dx * s + dy * c).abs() <= 62.5 + 1e-9,
                    "chainage {station}"
                );
                station += 1.0;
            }
        }
        near(covered, road.end_station(), 1e-9);
    }

    #[test]
    fn strips_can_start_with_a_key_plan_and_refuse_an_empty_range() {
        let road = alignment_through(vec![pi(0.0, 0.0, 0.0), pi(600.0, 800.0, 0.0)], 0.0);
        let mut request = StripRequest {
            scale: 500.0,
            overlap_m: 20.0,
            key_plan: true,
            ..StripRequest::default()
        };
        let sheets = ok(strip_sheets(&road, "ROAD", &request));
        assert_eq!(sheets.len(), 7);
        assert_eq!(sheets[0].viewports[0].kind, ViewportKind::KeyPlan);
        assert_eq!(sheets[0].viewports[0].marks.len(), 6);
        // The first strip's match line now leads to sheet 3.
        assert_eq!(
            mark_label(&set_of(&sheets), &sheets[1].viewports[0].marks[0]),
            "MATCH LINE CH 172.500 - SEE SHEET 3"
        );
        request.from_chainage = Some(500.0);
        request.to_chainage = Some(500.0);
        assert_eq!(
            strip_sheets(&road, "ROAD", &request)
                .expect_err("an empty range")
                .code,
            ErrorCode::InvalidArgument
        );
    }

    // ---- cross_section_sheets ----------------------------------------------------------------

    #[test]
    fn cross_sections_are_packed_down_each_column_in_chainage_order() {
        // A 100 m alignment whose chainage starts at 1000, cut every 20 m: CH
        // 1000, 1020 ... 1100, six sections. Two rows by two columns: four on
        // the first sheet, two on the second. A cell is (385 - 3) / 2 = 191 by
        // (250 - 3) / 2 = 123.5 mm. A 40 m wide section in 80% of 191 mm needs
        // 1 : 40000 / 152.8 = 261.8 - 1 : 500. With nothing sampled the
        // exaggeration is 1 and each section is centred when it is drawn.
        let road = alignment_through(vec![pi(0.0, 0.0, 0.0), pi(100.0, 0.0, 0.0)], 1000.0);
        let request = CrossSectionRequest {
            interval: 20.0,
            half_width: 20.0,
            rows: 2,
            columns: 2,
            ..CrossSectionRequest::default()
        };
        let sheets = ok(cross_section_sheets(&road, "ROAD", &request));
        assert_eq!(sheets.len(), 2);
        assert_eq!(sheets[0].viewports.len(), 4);
        assert_eq!(sheets[1].viewports.len(), 2);
        assert_eq!(
            sheets[0].name,
            "ROAD CROSS SECTIONS CH 1000.000 TO 1060.000"
        );
        assert_eq!(
            sheets[1].name,
            "ROAD CROSS SECTIONS CH 1080.000 TO 1100.000"
        );
        let chainages = [1000.0, 1020.0, 1040.0, 1060.0];
        for (section, chainage) in sheets[0].viewports.iter().zip(chainages) {
            assert_eq!(section.kind, ViewportKind::CrossSections);
            assert_eq!(section.source.stations.len(), 1);
            near(section.source.stations[0], chainage, 1e-9);
            assert_eq!(section.scale, 500.0);
            assert_eq!(section.vertical_exaggeration, 1.0);
            assert!(section.auto_centre);
            assert_eq!(section.source.section_half_width, 20.0);
        }
        // Down the first column, then the second: top-left, bottom-left,
        // top-right. The bottom row starts 123.5 + 3 below the top (286 -
        // 126.5 = 159.5); the second column 191 + 3 right of the first (218).
        expect_box(&sheets[0].viewports[0].rect, 24.0, 162.5, 215.0, 286.0);
        expect_box(&sheets[0].viewports[1].rect, 24.0, 36.0, 215.0, 159.5);
        expect_box(&sheets[0].viewports[2].rect, 218.0, 162.5, 409.0, 286.0);
        expect_box(&sheets[1].viewports[0].rect, 24.0, 162.5, 215.0, 286.0);
    }

    #[test]
    fn given_stations_are_sorted_and_their_duplicates_dropped() {
        let road = alignment_through(vec![pi(0.0, 0.0, 0.0), pi(100.0, 0.0, 0.0)], 0.0);
        let request = CrossSectionRequest {
            stations: vec![60.0, 20.0, 40.0, 20.0],
            ..CrossSectionRequest::default()
        };
        let sheets = ok(cross_section_sheets(&road, "ROAD", &request));
        assert_eq!(sheets.len(), 1);
        assert_eq!(sheets[0].viewports.len(), 3);
        assert_eq!(sheets[0].viewports[0].source.stations[0], 20.0);
        assert_eq!(sheets[0].viewports[1].source.stations[0], 40.0);
        assert_eq!(sheets[0].viewports[2].source.stations[0], 60.0);
    }

    #[test]
    fn the_exaggeration_is_the_largest_that_fits_the_deepest_section() {
        // Ground z = x / 2 (exact in binary). The alignment runs north up x =
        // 50, so every section runs east-west from x 30 to x 70: ground 15 to
        // 35, 20 m deep, centred on 25. At 1 : 500 (as above) 20 m is 40 mm;
        // 80% of a 123.5 mm cell is 98.8 mm, so the exaggeration may be up to
        // 2.47: of 1, 2, 2.5, 4 ... the largest that fits is 2.
        let ground = ok(TinSurface::create(
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(100.0, 0.0, 50.0),
                Point3::new(100.0, 100.0, 50.0),
                Point3::new(0.0, 100.0, 0.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
            Vec::new(),
        ));
        let road = alignment_through(vec![pi(50.0, 10.0, 0.0), pi(50.0, 90.0, 0.0)], 0.0);
        let request = CrossSectionRequest {
            interval: 40.0,
            half_width: 20.0,
            rows: 2,
            columns: 2,
            surfaces: vec![SectionSurfaceInput {
                name: "GROUND",
                surface: Some(&ground),
            }],
            ..CrossSectionRequest::default()
        };
        let sheets = ok(cross_section_sheets(&road, "ROAD", &request));
        assert_eq!(sheets[0].viewports.len(), 3); // CH 0, 40, 80
        for section in &sheets[0].viewports {
            assert_eq!(section.scale, 500.0);
            assert_eq!(section.vertical_exaggeration, 2.0);
            assert!(!section.auto_centre);
            expect_point(section.centre, 0.0, 25.0, TIGHT);
        }
    }

    #[test]
    fn cross_sections_on_a_curve_are_cut_at_their_true_chainage_to_the_very_end() {
        // East 300 m, a 100 m radius curve, north 300 m. The turn is 90
        // degrees, so each tangent is 100 tan 45 = 100 m and the curve 100 pi
        // / 2 = 157.0796326794897 m: the road ends at CH 200 +
        // 157.0796326794897 + 200 = 557.0796326794897. Drawn in chords the
        // curve is a little shorter; a chainage is still the road's, not the
        // chords'.
        let road = alignment_through(
            vec![
                pi(0.0, 0.0, 0.0),
                pi(300.0, 0.0, 100.0),
                pi(300.0, 300.0, 0.0),
            ],
            0.0,
        );
        near(road.end_station(), 557.0796326794897, 1e-9);

        // The end chainage given as a station is cut there.
        let mut request = CrossSectionRequest {
            stations: vec![0.0, 100.0, road.end_station()],
            ..CrossSectionRequest::default()
        };
        let given = ok(cross_section_sheets(&road, "ROAD", &request));
        assert_eq!(given[0].viewports.len(), 3);
        assert_eq!(given[0].viewports[2].source.stations[0], road.end_station());

        // Every 100 m: CH 0, 100 ... 500 and the end, seven sections; the last
        // is at the end chainage and titled with it, rounded to 557.080.
        request.stations.clear();
        request.interval = 100.0;
        let every = ok(cross_section_sheets(&road, "ROAD", &request));
        let sections: Vec<&Viewport> = every
            .iter()
            .flat_map(|sheet| sheet.viewports.iter())
            .collect();
        let chainages: Vec<f64> = sections
            .iter()
            .map(|section| section.source.stations[0])
            .collect();
        assert_eq!(chainages.len(), 7);
        for (i, &chainage) in chainages.iter().take(6).enumerate() {
            near(chainage, 100.0 * i as f64, 1e-9);
        }
        near(chainages[6], 557.0796326794897, 1e-9);
        assert_eq!(
            automatic_title(sections[sections.len() - 1]),
            "CROSS SECTION CH 557.080"
        );

        // A chainage past the end is still refused.
        request.stations = vec![600.0];
        assert_eq!(
            cross_section_sheets(&road, "ROAD", &request)
                .expect_err("past the end")
                .code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn cross_sections_refuse_an_empty_grid_or_no_width() {
        let road = alignment_through(vec![pi(0.0, 0.0, 0.0), pi(100.0, 0.0, 0.0)], 0.0);
        let mut request = CrossSectionRequest {
            rows: 0,
            ..CrossSectionRequest::default()
        };
        assert_eq!(
            cross_section_sheets(&road, "ROAD", &request)
                .expect_err("no rows")
                .code,
            ErrorCode::InvalidArgument
        );
        request.rows = 4;
        request.half_width = 0.0;
        assert_eq!(
            cross_section_sheets(&road, "ROAD", &request)
                .expect_err("no width")
                .code,
            ErrorCode::InvalidArgument
        );
    }

    // ---- sheets_from_plot_frames ---------------------------------------------------------

    #[test]
    fn an_imported_plot_frame_becomes_a_sheet_showing_the_ground_it_covered() {
        let mut model = Model::default();
        // An A3 frame at 1 : 1000, rotated 30 degrees, its paper's corner at
        // (1000, 2000), with the built-in frame's own margins (L23 R10 T10
        // B35).
        let north = FrameFields {
            width: 420.0,
            height: 297.0,
            scale: 1000.0,
            rotation: 30.0,
            origin: Point2::new(1000.0, 2000.0),
            margins: [23.0, 10.0, 10.0, 35.0],
        };
        ok(model
            .entities
            .add(imported_frame(&north, "FRAMES", "NORTH", Vec2::default())));
        // An A1 frame at 1 : 500, not rotated, margins 22 all round but 82 at
        // the bottom.
        let a1 = FrameFields {
            width: 841.0,
            height: 594.0,
            scale: 500.0,
            rotation: 0.0,
            origin: Point2::new(0.0, 0.0),
            margins: [22.0, 22.0, 22.0, 82.0],
        };
        ok(model
            .entities
            .add(imported_frame(&a1, "FRAMES", "", Vec2::default())));
        // 500 x 300 mm is no ISO A size.
        let odd = FrameFields {
            width: 500.0,
            height: 300.0,
            scale: 1000.0,
            rotation: 0.0,
            origin: Point2::new(0.0, 0.0),
            margins: [10.0, 10.0, 10.0, 10.0],
        };
        ok(model
            .entities
            .add(imported_frame(&odd, "FRAMES", "ODD", Vec2::default())));
        let mut skipped = Vec::new();
        let sheets = ok(sheets_from_plot_frames(
            &model,
            &SheetTemplate::default(),
            Some(&mut skipped),
        ));
        assert_eq!(sheets.len(), 2);
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].contains("not an ISO A size"), "{}", skipped[0]);

        let north = &sheets[0];
        assert_eq!(north.name, "NORTH");
        assert_eq!(north.paper, PaperSize::A3);
        assert!(north.landscape);
        let plan = &north.viewports[0];
        // The frame's window on the paper, 23..410 x 35..287, is exactly the
        // built-in frame's drawing area; its centre (216.5, 161) mm is 216.5
        // m along and 161 m up from the corner at 1 : 1000. Along (cos 30,
        // sin 30) = (0.8660254037844386, 0.5), up (-0.5, 0.8660254037844386):
        // x = 1000 + 187.4944999193310 - 80.5 = 1106.994499919331,
        // y = 2000 + 108.25 + 139.4300900092946 = 2247.680090009295.
        // (The outline's corners are rounded to the ground's doubles, so the
        // direction read back from them is 30 degrees to within a few units
        // in the last place.)
        expect_box(&plan.rect, 23.0, 35.0, 410.0, 287.0);
        assert_eq!(plan.scale, 1000.0);
        near(plan.rotation, PI / 6.0, 1e-12);
        expect_point(plan.centre, 1106.994499919331, 2247.680090009295, 1e-9);
        assert!(plan.hidden_layers.hides("FRAMES"));

        let a1 = &sheets[1];
        assert_eq!(a1.paper, PaperSize::A1);
        assert_eq!(a1.name, "plot frame 2"); // no name of its own
        // Its window 22..819 x 82..572 clipped to A1's drawing area 46..820 x
        // 70..574: 46..819 x 82..572, centre (432.5, 327) mm - at 1 : 500 the
        // point (216.25, 163.5) m from the corner.
        expect_box(&a1.viewports[0].rect, 46.0, 82.0, 819.0, 572.0);
        expect_point(a1.viewports[0].centre, 216.25, 163.5, TIGHT);
    }

    #[test]
    fn a_plot_frame_sheet_is_placed_by_the_frames_outline_not_by_its_file_origin() {
        let mut model = Model::default();
        // An A3 frame at 1 : 500 whose file puts its corner at (300000,
        // 6200000), imported with that shifted to (0, 0) - the outline is
        // drawn at (0, 0), the properties still say 300000, 6200000. Margins
        // L23 R10 T10 B35 give the window 23..410 x 35..287, centre (216.5,
        // 161) mm: at 1 : 500, (108.25, 80.5) m from the outline's corner.
        let shifted = FrameFields {
            width: 420.0,
            height: 297.0,
            scale: 500.0,
            rotation: 0.0,
            origin: Point2::new(300000.0, 6200000.0),
            margins: [23.0, 10.0, 10.0, 35.0],
        };
        ok(model.entities.add(imported_frame(
            &shifted,
            "FRAMES",
            "SHIFTED",
            Vec2::new(300000.0, 6200000.0),
        )));
        // A frame imported at (0, 0) unrotated and then turned a quarter turn
        // and moved: its outline now starts at (100, 50) and runs north - 420
        // m up to (100, 470) at 1 : 1000, then 297 m west to x = -197 - while
        // its properties still say rotation 0, origin (0, 0). Along is (0, 1),
        // up (-1, 0): the window's centre, 216.5 m along and 161 m up, is
        // (100 - 161, 50 + 216.5) = (-61, 266.5), and the plan is turned a
        // quarter turn with it.
        let moved = FrameFields {
            width: 420.0,
            height: 297.0,
            scale: 1000.0,
            rotation: 0.0,
            origin: Point2::new(0.0, 0.0),
            margins: [23.0, 10.0, 10.0, 35.0],
        };
        ok(model.entities.add(frame_entity(
            &moved,
            vec![
                Point2::new(100.0, 50.0),
                Point2::new(100.0, 470.0),
                Point2::new(-197.0, 470.0),
                Point2::new(-197.0, 50.0),
            ],
            "FRAMES",
            "MOVED",
        )));
        // The first frame mirrored across x = 0: along is (-1, 0) and its last
        // corner is up at (0, 297), to the RIGHT of along - so up is (0, 1),
        // not along's left (0, -1). The window's centre is (-216.5, 161),
        // inside the mirrored outline, and the plan is turned half a turn (the
        // direction of (-420, 0)).
        ok(model.entities.add(frame_entity(
            &moved,
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(-420.0, 0.0),
                Point2::new(-420.0, 297.0),
                Point2::new(0.0, 297.0),
            ],
            "FRAMES",
            "MIRRORED",
        )));
        // An outline that is no longer the frame's four corners - here a
        // triangle - cannot say where its paper is.
        ok(model.entities.add(frame_entity(
            &moved,
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(1.0, 1.0),
            ],
            "FRAMES",
            "BROKEN",
        )));
        let mut skipped = Vec::new();
        let sheets = ok(sheets_from_plot_frames(
            &model,
            &SheetTemplate::default(),
            Some(&mut skipped),
        ));
        assert_eq!(sheets.len(), 3);
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].contains("outline"), "{}", skipped[0]);

        let near_view = &sheets[0].viewports[0];
        assert_eq!(sheets[0].name, "SHIFTED");
        assert_eq!(near_view.scale, 500.0);
        assert_eq!(near_view.rotation, 0.0);
        expect_point(near_view.centre, 108.25, 80.5, TIGHT);

        let turned = &sheets[1].viewports[0];
        assert_eq!(sheets[1].name, "MOVED");
        near(turned.rotation, PI / 2.0, 1e-15);
        expect_point(turned.centre, -61.0, 266.5, TIGHT);

        let mirrored = &sheets[2].viewports[0];
        assert_eq!(sheets[2].name, "MIRRORED");
        near(mirrored.rotation, PI, 1e-15);
        expect_point(mirrored.centre, -216.5, 161.0, TIGHT);
    }

    // ---- smart_layout ----------------------------------------------------------------------

    #[test]
    fn a_smart_plan_with_a3_d_view_and_legend_shares_one_sheet() {
        // "Main and two panels right": the plan in the 243.4 x 247 mm main
        // cell, which a 300 x 200 m area needs 1 : max(300000 / 243.4 =
        // 1232.5, 200000 / 247 = 809.7) for - 1 : 1250.
        let request = LayoutRequest {
            plan_area: Some(rect(0.0, 0.0, 300.0, 200.0)),
            model3d: true,
            legend: true,
            ..LayoutRequest::default()
        };
        let sheets = ok(smart_layout(&Model::default(), &request, None));
        assert_eq!(sheets.len(), 1);
        let sheet = &sheets[0];
        assert_eq!(sheet.viewports.len(), 3);
        assert_eq!(sheet.viewports[0].kind, ViewportKind::Plan);
        assert_eq!(sheet.viewports[0].scale, 1250.0);
        expect_box(&sheet.viewports[0].rect, 25.5, 37.5, 268.9, 284.5);
        assert_eq!(sheet.viewports[1].kind, ViewportKind::Model3D);
        expect_box(&sheet.viewports[1].rect, 271.9, 162.5, 407.5, 284.5);
        assert_eq!(sheet.viewports[2].kind, ViewportKind::Legend);
        expect_box(&sheet.viewports[2].rect, 271.9, 37.5, 407.5, 159.5);
        assert_eq!(sheet_scale_text(sheet), "1:1250");
    }

    #[test]
    fn a_smart_plan_at_a_fixed_scale_too_large_for_one_sheet_is_tiled() {
        // 400 x 200 m at 1 : 500 needs 1 : 1039 on one sheet: tiles, 3 x 2 (as
        // the grid test above, without overlap: ceil(400 / 192.5) = 3,
        // ceil(200 / 125) = 2) after a key plan.
        let mut request = LayoutRequest {
            plan_area: Some(rect(0.0, 0.0, 400.0, 200.0)),
            scale: 500.0,
            ..LayoutRequest::default()
        };
        let sheets = ok(smart_layout(&Model::default(), &request, None));
        assert_eq!(sheets.len(), 7);
        assert_eq!(sheets[0].viewports[0].kind, ViewportKind::KeyPlan);
        // At 1 : 500 an area that fits (190 x 120 m needs 1 : 493.5) is one
        // sheet.
        request.plan_area = Some(rect(0.0, 0.0, 190.0, 120.0));
        let one = ok(smart_layout(&Model::default(), &request, None));
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].viewports[0].scale, 500.0);
    }

    #[test]
    fn a_smart_plans_tiles_with_nothing_drawn_in_them_are_left_out_as_the_grids_are() {
        // The 3 x 2 tiles above, from x -88.75 (200 - 3 x 192.5 / 2) and down
        // from y 225 (100 + 125): the top-left is x -88.75..103.75, y
        // 100..225, centre (7.5, 162.5). Drawn there alone, it is the one tile
        // kept.
        let drawn = drawn_at(vec![Point2::new(0.0, 200.0)]);
        let mut request = LayoutRequest {
            plan_area: Some(rect(0.0, 0.0, 400.0, 200.0)),
            scale: 500.0,
            drawn: Some(&drawn),
            ..LayoutRequest::default()
        };
        let mut skipped = Vec::new();
        let sheets = ok(smart_layout(
            &Model::default(),
            &request,
            Some(&mut skipped),
        ));
        assert_eq!(sheets.len(), 2);
        assert_eq!(sheets[0].viewports[0].kind, ViewportKind::KeyPlan);
        assert_eq!(sheets[1].name, "PLAN TILE 1");
        expect_point(sheets[1].viewports[0].centre, 7.5, 162.5, TIGHT);
        assert_eq!(skipped, vec!["5 of 6 tiles, with nothing drawn in them"]);
        // An area that fits one sheet is never tiled, so nothing is left out.
        request.plan_area = Some(rect(1000.0, 1000.0, 1190.0, 1120.0));
        skipped.clear();
        let one = ok(smart_layout(
            &Model::default(),
            &request,
            Some(&mut skipped),
        ));
        assert_eq!(one.len(), 1);
        assert!(skipped.is_empty());
    }

    #[test]
    fn an_automatic_strip_beside_a3_d_view_is_fitted_to_its_narrower_cell() {
        // A 180 m straight road, a plan along it and a 3D view, scale "auto".
        // "Main and panel right" gives the plan the cell 25.5 .. 276.6 x 37.5
        // .. 284.5 (0.66 x 385 - 3 = 251.1 mm wide, 250 - 3 = 247 high) and
        // the 3D view 279.6 .. 407.5 (24 + 254.1 + 1.5; 0.34 x 385 - 3 = 127.9
        // wide). 180 m in 251.1 mm needs 1 : 716.8 - 1 : 750, 188.325 m
        // across: the whole road, CH 0 to 180, in one strip centred on x = 90.
        let model = model_with_straight(180.0);
        let request = LayoutRequest {
            alignment: String::from("ROAD"),
            plan_along_alignment: true,
            model3d: true,
            ..LayoutRequest::default()
        };
        let sheets = ok(smart_layout(&model, &request, None));
        assert_eq!(sheets.len(), 1);
        let sheet = &sheets[0];
        assert_eq!(sheet.viewports.len(), 2);
        let plan = &sheet.viewports[0];
        assert_eq!(plan.kind, ViewportKind::Plan);
        expect_box(&plan.rect, 25.5, 37.5, 276.6, 284.5);
        assert_eq!(plan.scale, 750.0);
        near(plan.source.chainage_from, 0.0, 1e-9);
        near(plan.source.chainage_to, 180.0, 1e-9);
        expect_point(plan.centre, 90.0, 0.0, 1e-9);
        assert!(metres_wide(plan) >= 180.0);
        assert_eq!(sheet.viewports[1].kind, ViewportKind::Model3D);
        expect_box(&sheet.viewports[1].rect, 279.6, 37.5, 407.5, 284.5);
    }

    #[test]
    fn a_fixed_scale_strip_too_long_for_the_narrow_cell_leaves_the3_d_view_a_sheet_of_its_own() {
        // The same road at 1 : 500: the narrow cell holds 251.1 x 0.5 = 125.55
        // m - not the 180 m - so the plan keeps the whole width, 382 mm (385 -
        // 3) = 191 m, one strip, and the 3D view goes on a second sheet.
        let model = model_with_straight(180.0);
        let request = LayoutRequest {
            alignment: String::from("ROAD"),
            plan_along_alignment: true,
            model3d: true,
            scale: 500.0,
            ..LayoutRequest::default()
        };
        let sheets = ok(smart_layout(&model, &request, None));
        assert_eq!(sheets.len(), 2);
        let first = &sheets[0];
        assert_eq!(first.viewports.len(), 1);
        let plan = &first.viewports[0];
        assert_eq!(plan.scale, 500.0);
        expect_box(&plan.rect, 25.5, 37.5, 407.5, 284.5);
        near(plan.source.chainage_from, 0.0, 1e-9);
        near(plan.source.chainage_to, 180.0, 1e-9);
        assert!(metres_wide(plan) >= 180.0);
        let second = &sheets[1];
        assert_eq!(second.id, "s2");
        assert_eq!(second.name, "3D VIEW");
        assert_eq!(second.viewports.len(), 1);
        assert_eq!(second.viewports[0].kind, ViewportKind::Model3D);
    }

    #[test]
    fn an_area_at_a_fixed_scale_goes_beside_the_legend_only_when_it_still_fits() {
        // 180 x 100 m at 1 : 500 with a legend. Beside it, in the 251.1 x 247
        // mm cell of "Main and panel right", the area needs 1 : max(180000 /
        // 251.1 = 716.8, 100000 / 247 = 404.9) - more than 500, it does not
        // fit. On a sheet of its own, 385 x 250 mm, it needs 1 : max(467.5,
        // 400): it fits, so the plan keeps its sheet (the tiling area 24 ..
        // 409 x 36 .. 286, 192.5 m across) and the legend gets the next.
        let mut request = LayoutRequest {
            plan_area: Some(rect(0.0, 0.0, 180.0, 100.0)),
            scale: 500.0,
            legend: true,
            ..LayoutRequest::default()
        };
        let sheets = ok(smart_layout(&Model::default(), &request, None));
        assert_eq!(sheets.len(), 2);
        assert_eq!(sheets[0].viewports.len(), 1);
        let plan = &sheets[0].viewports[0];
        assert_eq!(plan.scale, 500.0);
        expect_box(&plan.rect, 24.0, 36.0, 409.0, 286.0);
        assert!(metres_wide(plan) >= 180.0);
        assert_eq!(sheets[1].name, "LEGEND");
        assert_eq!(sheets[1].viewports.len(), 1);
        assert_eq!(sheets[1].viewports[0].kind, ViewportKind::Legend);

        // 100 x 80 m needs 1 : max(100000 / 251.1 = 398.2, 80000 / 247 =
        // 323.9) beside the legend: at 1 : 500 it fits, and shares the sheet.
        request.plan_area = Some(rect(0.0, 0.0, 100.0, 80.0));
        let shared = ok(smart_layout(&Model::default(), &request, None));
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].viewports.len(), 2);
        let beside = &shared[0].viewports[0];
        assert_eq!(beside.scale, 500.0);
        expect_box(&beside.rect, 25.5, 37.5, 276.6, 284.5);
        expect_point(beside.centre, 50.0, 40.0, TIGHT);
        assert_eq!(shared[0].viewports[1].kind, ViewportKind::Legend);
        expect_box(&shared[0].viewports[1].rect, 279.6, 37.5, 407.5, 284.5);
    }

    #[test]
    fn a_smart_plan_and_profile_then_cross_sections_spills_onto_more_sheets() {
        // A 1000 m straight road at 1 : 500. The plan takes the top cell of
        // "Main and panel below" (382 mm wide, 191 m at 1 : 500): ceil(1000 /
        // 191) = 6 plan-and-profile sheets. Cross sections every 100 m are 11
        // (CH 0 to 1000), 4 rows by 2 columns a sheet: 2 more sheets. 8 in
        // all.
        let model = model_with_road();
        let request = LayoutRequest {
            alignment: String::from("ROAD"),
            plan_along_alignment: true,
            long_section: true,
            cross_section_interval: 100.0,
            scale: 500.0,
            ..LayoutRequest::default()
        };
        let sheets = ok(smart_layout(&model, &request, None));
        assert_eq!(sheets.len(), 8);
        for (i, sheet) in sheets.iter().enumerate() {
            assert_eq!(sheet.id, format!("s{}", i + 1));
        }
        distinct_viewport_ids(&sheets);
        for sheet in sheets.iter().take(6) {
            assert_eq!(sheet.viewports.len(), 2);
            let plan = &sheet.viewports[0];
            let profile = &sheet.viewports[1];
            assert_eq!(plan.kind, ViewportKind::Plan);
            assert_eq!(profile.kind, ViewportKind::LongSection);
            expect_box(&plan.rect, 25.5, 127.5, 407.5, 284.5);
            expect_box(&profile.rect, 25.5, 37.5, 407.5, 124.5);
            // The profile shows the plan's chainages at the plan's horizontal
            // scale, exaggerated 10 times (V 1 : 50).
            assert_eq!(profile.source.chainage_from, plan.source.chainage_from);
            assert_eq!(profile.source.chainage_to, plan.source.chainage_to);
            assert_eq!(profile.scale, 500.0);
            assert_eq!(sheet_scale_text(sheet), "AS SHOWN");
            assert_eq!(scale_text(profile), "H 1:500 V 1:50");
        }
        near(sheets[0].viewports[0].source.chainage_to, 191.0, 1e-9);
        near(sheets[5].viewports[0].source.chainage_from, 955.0, 1e-9);
        assert_eq!(sheets[6].viewports.len(), 8);
        assert_eq!(sheets[7].viewports.len(), 3);
        let last = &sheets[7].viewports[sheets[7].viewports.len() - 1];
        assert_eq!(last.source.stations[0], 1000.0);
    }

    #[test]
    fn an_automatic_plan_and_profile_fits_the_whole_road_on_one_sheet() {
        // 1000 m in 382 mm needs 1 : 2617.8 - 1 : 5000, one sheet.
        let model = model_with_road();
        let request = LayoutRequest {
            alignment: String::from("ROAD"),
            plan_along_alignment: true,
            long_section: true,
            ..LayoutRequest::default()
        };
        let sheets = ok(smart_layout(&model, &request, None));
        assert_eq!(sheets.len(), 1);
        assert_eq!(sheets[0].viewports[0].scale, 5000.0);
        assert_eq!(sheets[0].viewports[1].scale, 5000.0);
    }

    #[test]
    fn a_smart_layout_refuses_an_unknown_alignment_or_a_request_for_nothing() {
        let mut request = LayoutRequest::default();
        assert_eq!(
            smart_layout(&Model::default(), &request, None)
                .expect_err("nothing to show")
                .code,
            ErrorCode::InvalidArgument
        );
        request.alignment = String::from("NOWHERE");
        request.long_section = true;
        assert_eq!(
            smart_layout(&Model::default(), &request, None)
                .expect_err("no such alignment")
                .code,
            ErrorCode::NotFound
        );
        // An empty plan area, at a fixed scale as well as "auto".
        let mut empty = LayoutRequest {
            plan_area: Some(Box2::default()),
            legend: true,
            ..LayoutRequest::default()
        };
        assert_eq!(
            smart_layout(&Model::default(), &empty, None)
                .expect_err("an empty area")
                .code,
            ErrorCode::InvalidArgument
        );
        empty.scale = 500.0;
        assert_eq!(
            smart_layout(&Model::default(), &empty, None)
                .expect_err("an empty area at a scale")
                .code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn the_same_request_gives_the_same_sheets() {
        let model = model_with_road();
        let request = LayoutRequest {
            alignment: String::from("ROAD"),
            plan_along_alignment: true,
            long_section: true,
            cross_section_interval: 50.0,
            model3d: true,
            legend: true,
            scale: 1000.0,
            ..LayoutRequest::default()
        };
        let first = ok(smart_layout(&model, &request, None));
        let second = ok(smart_layout(&model, &request, None));
        assert!(first == second);
    }

    // ---- prepare_for_append -----------------------------------------------------------------

    #[test]
    fn appended_sheets_take_fresh_ids_and_their_marks_follow() {
        // A set with sheets s1 and s2 and viewports vp1..vp3; a generator's two
        // sheets (its own s1 and s2, vp1 and vp2) whose first leads to its
        // second. Appended they become s3 and s4 with vp4 and vp5, and the
        // mark leads to s4.
        let viewport = |id: &str| Viewport {
            id: String::from(id),
            ..Viewport::default()
        };
        let set = SheetSet {
            sheets: vec![
                Sheet {
                    id: String::from("s1"),
                    viewports: vec![viewport("vp1"), viewport("vp2")],
                    ..Sheet::default()
                },
                Sheet {
                    id: String::from("s2"),
                    viewports: vec![viewport("vp3")],
                    ..Sheet::default()
                },
            ],
            ..SheetSet::default()
        };
        let mut leading = viewport("vp1");
        leading.marks.push(mark(WorldMarkKind::MatchLine, "s2"));
        let mut more = vec![
            Sheet {
                id: String::from("s1"),
                viewports: vec![leading],
                ..Sheet::default()
            },
            Sheet {
                id: String::from("s2"),
                viewports: vec![viewport("vp2")],
                ..Sheet::default()
            },
        ];
        prepare_for_append(&set, &mut more);
        assert_eq!(more[0].id, "s3");
        assert_eq!(more[1].id, "s4");
        assert_eq!(more[0].viewports[0].id, "vp4");
        assert_eq!(more[1].viewports[0].id, "vp5");
        assert_eq!(more[0].viewports[0].marks[0].sheet, "s4");
    }

    #[test]
    fn an_appended_sheet_never_takes_the_id_of_a_removed_sheet_a_mark_still_names() {
        // Sheets s1 and s2 are left; s1's match line still leads to s3, which
        // was removed. A new sheet must not become s3 and be led to by it: it
        // is s4, one past the highest id any sheet has or any mark names.
        let mut leading = Viewport {
            id: String::from("vp1"),
            ..Viewport::default()
        };
        leading.marks.push(mark(WorldMarkKind::MatchLine, "s3"));
        let set = SheetSet {
            sheets: vec![
                Sheet {
                    id: String::from("s1"),
                    viewports: vec![leading],
                    ..Sheet::default()
                },
                Sheet {
                    id: String::from("s2"),
                    ..Sheet::default()
                },
            ],
            ..SheetSet::default()
        };
        assert_eq!(next_sheet_id(&set), "s4");
        let mut more = vec![Sheet::default()];
        prepare_for_append(&set, &mut more);
        assert_eq!(more[0].id, "s4");
    }

    #[test]
    fn appending_after_a_huge_stored_viewport_id_neither_throws_nor_repeats_an_id() {
        // Ids come from stored JSON unchecked. vp4294967295 is the largest
        // number 32 bits hold: the next ids are 4294967296 and 4294967297, one
        // and two past it.
        let mut more = vec![Sheet {
            viewports: vec![Viewport::default(), Viewport::default()],
            ..Sheet::default()
        }];
        let mut large = set_of(&[Sheet::default()]);
        large.sheets[0].id = String::from("s1");
        large.sheets[0].viewports = vec![Viewport {
            id: String::from("vp4294967295"),
            ..Viewport::default()
        }];
        prepare_for_append(&large, &mut more);
        assert_eq!(more[0].viewports[0].id, "vp4294967296");
        assert_eq!(more[0].viewports[1].id, "vp4294967297");

        // 18446744073709551615 is the largest a 64-bit count holds: nothing
        // comes after it, so the new ids are the lowest free ones - vp1 is
        // taken, so vp2 and vp3. An id too large for any count is no number of
        // ours and is left alone.
        let mut largest = large.clone();
        largest.sheets[0].viewports =
            ["vp18446744073709551615", "vp1", "vp99999999999999999999999"]
                .into_iter()
                .map(|id| Viewport {
                    id: String::from(id),
                    ..Viewport::default()
                })
                .collect();
        more[0].viewports[0].id.clear();
        more[0].viewports[1].id.clear();
        prepare_for_append(&largest, &mut more);
        assert_eq!(more[0].viewports[0].id, "vp2");
        assert_eq!(more[0].viewports[1].id, "vp3");
        assert_eq!(next_viewport_id(&largest), "vp2");
    }
}
