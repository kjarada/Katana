// The live key plan (include/katana/cad/plotting/key_plan.hpp): every sheet's
// plan outlined from the sheets as they are, numbered as the set numbers them
// now, the key plan's own sheet flagged, and an automatic key plan fitted to
// all of them; tests/cad/plotting/test_key_plan.cpp.

use std::f64::consts::PI;

use crate::{double_eq, near, ok};
use katana_cad::Document;
use katana_cad::plotting::key_plan::{
    KeyPlanOutline, fit_key_plan, fit_plan_placement, key_plan_outlines, model_placer, place_plan,
    place_plan_on_sheet, printed_sheet_number,
};
use katana_cad::plotting::plan_grid::{PlanPlacement, plan_world_to_paper, stored_placement};
use katana_cad::plotting::sheet_commands::{ADD_SHEETS, add_sheets, move_sheet, remove_sheet};
use katana_cad::plotting::{Sheet, SheetSet, Viewport, ViewportKind};
use katana_entity::{Alignment, Entity, Geometry, Model};
use katana_geometry::{AlignmentPi, Box2, Point2, Segment2};

fn plan_of(id: &str, rect: Box2, scale: f64, centre: Point2, rotation: f64) -> Viewport {
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

fn key_plan_of(id: &str, rect: Box2) -> Viewport {
    Viewport {
        kind: ViewportKind::KeyPlan,
        ..plan_of(id, rect, 5000.0, Point2::new(1050.0, 2000.0), 0.0)
    }
}

fn sheet_of(id: &str, viewports: Vec<Viewport>) -> Sheet {
    Sheet {
        id: String::from(id),
        name: String::from(id),
        viewports,
        ..Sheet::default()
    }
}

// 100 x 50 m at 1:1000.
fn strip() -> Box2 {
    Box2::new(Point2::new(0.0, 0.0), Point2::new(100.0, 50.0))
}

// A key-plan sheet and two plans side by side: sheet 2 over 950..1050 x
// 1975..2025, sheet 3 over 1050..1150.
fn key_and_two_plans() -> SheetSet {
    SheetSet {
        sheets: vec![
            sheet_of(
                "s1",
                vec![key_plan_of(
                    "vp1",
                    Box2::new(Point2::new(30.0, 40.0), Point2::new(130.0, 110.0)),
                )],
            ),
            sheet_of(
                "s2",
                vec![plan_of(
                    "vp2",
                    strip(),
                    1000.0,
                    Point2::new(1000.0, 2000.0),
                    0.0,
                )],
            ),
            sheet_of(
                "s3",
                vec![plan_of(
                    "vp3",
                    strip(),
                    1000.0,
                    Point2::new(1100.0, 2000.0),
                    0.0,
                )],
            ),
        ],
        ..SheetSet::default()
    }
}

fn labels(outlines: &[KeyPlanOutline]) -> Vec<String> {
    outlines
        .iter()
        .map(|outline| outline.label.clone())
        .collect()
}

mod key_plan {
    use super::*;

    #[test]
    fn every_other_sheets_plan_is_outlined_on_the_ground_and_numbered() {
        let set = key_and_two_plans();
        let outlines = key_plan_outlines(&set, 0, None);
        assert_eq!(outlines.len(), 2);
        assert_eq!(outlines[0].sheet_id, "s2");
        assert_eq!(outlines[0].sheet_index, 1);
        assert_eq!(outlines[0].viewport_id, "vp2");
        assert_eq!(outlines[0].label, "2");
        assert!(!outlines[0].current);
        assert!(outlines[0].labelled);
        // Counter-clockwise from the ground under the bottom-left corner.
        let expected = [
            Point2::new(950.0, 1975.0),
            Point2::new(1050.0, 1975.0),
            Point2::new(1050.0, 2025.0),
            Point2::new(950.0, 2025.0),
        ];
        assert_eq!(outlines[0].corners.len(), 4);
        for (corner, want) in outlines[0].corners.iter().zip(expected) {
            near(corner.x, want.x, 1e-9);
            near(corner.y, want.y, 1e-9);
        }
        assert_eq!(outlines[1].label, "3");
        near(outlines[1].corners[0].x, 1050.0, 1e-9);
    }

    #[test]
    fn the_numbers_follow_the_sheets_when_they_are_reordered() {
        let mut document = Document::new(None);
        ok(add_sheets(
            &mut document,
            key_and_two_plans().sheets,
            ADD_SHEETS,
        ));
        assert_eq!(
            labels(&key_plan_outlines(&document.sheet_set(), 0, None)),
            ["2", "3"]
        );

        // The tile over 1050..1150 becomes sheet 2: its outline says so, and
        // the outline stays where the sheet's plan is.
        ok(move_sheet(&mut document, 2, 1));
        let moved = key_plan_outlines(&document.sheet_set(), 0, None);
        assert_eq!(moved.len(), 2);
        assert_eq!(moved[0].sheet_id, "s3");
        assert_eq!(moved[0].label, "2");
        near(moved[0].corners[0].x, 1050.0, 1e-9);
        assert_eq!(moved[1].sheet_id, "s2");
        assert_eq!(moved[1].label, "3");

        // The key plan moved to the end is still not outlined, and the
        // numbers shift with it.
        ok(move_sheet(&mut document, 0, 2));
        assert_eq!(
            labels(&key_plan_outlines(&document.sheet_set(), 2, None)),
            ["1", "2"]
        );

        // A removed sheet is gone from the key plan at once.
        ok(remove_sheet(&mut document, 0));
        let after = key_plan_outlines(&document.sheet_set(), 1, None);
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].sheet_id, "s2");
        assert_eq!(after[0].label, "1");
    }

    #[test]
    fn the_number_is_the_one_printed_on_the_sheet() {
        let mut set = key_and_two_plans();
        set.numbering = String::from("{set}-{n:02}");
        set.defaults.set_number = String::from("DS");
        // Typed on the sheet: it wins.
        set.sheets[2]
            .fields
            .insert(String::from("sheet_number"), String::from("X1"));
        assert_eq!(labels(&key_plan_outlines(&set, 0, None)), ["DS-02", "X1"]);
        assert_eq!(printed_sheet_number(&set, 0), "DS-01");
        assert_eq!(printed_sheet_number(&set, 2), "X1");
        assert_eq!(printed_sheet_number(&set, 3), "");
    }

    #[test]
    fn the_key_plans_own_sheet_is_flagged_as_here() {
        // A plan with a key plan inset beside it, on sheet 2.
        let mut set = key_and_two_plans();
        set.sheets[1].viewports.push(key_plan_of(
            "vp4",
            Box2::new(Point2::new(300.0, 40.0), Point2::new(400.0, 110.0)),
        ));
        let outlines = key_plan_outlines(&set, 1, None);
        // Key plans are not outlined: the two plans only.
        assert_eq!(outlines.len(), 2);
        assert!(outlines[0].current);
        assert_eq!(outlines[0].viewport_id, "vp2");
        assert!(!outlines[1].current);
        // The same set seen from the key-plan sheet flags neither.
        for outline in key_plan_outlines(&set, 0, None) {
            assert!(!outline.current);
        }
    }

    #[test]
    fn only_placed_plans_are_outlined() {
        let mut set = key_and_two_plans();
        let unplaced = plan_of("vp5", Box2::default(), 1000.0, Point2::default(), 0.0);
        let unscaled = plan_of("vp6", strip(), 0.0, Point2::default(), 0.0);
        let section = Viewport {
            kind: ViewportKind::LongSection,
            ..plan_of("vp7", strip(), 1000.0, Point2::default(), 0.0)
        };
        let legend = Viewport {
            kind: ViewportKind::Legend,
            ..plan_of("vp8", strip(), 1000.0, Point2::default(), 0.0)
        };
        set.sheets
            .push(sheet_of("s4", vec![unplaced, unscaled, section, legend]));
        assert_eq!(key_plan_outlines(&set, 0, None).len(), 2);
    }

    #[test]
    fn a_rotated_plans_outline_is_the_ground_under_it_turned() {
        // Turned a quarter: the paper's +X runs north.
        let set = SheetSet {
            sheets: vec![sheet_of(
                "s1",
                vec![plan_of(
                    "vp1",
                    strip(),
                    1000.0,
                    Point2::new(0.0, 0.0),
                    PI / 2.0,
                )],
            )],
            ..SheetSet::default()
        };
        let outlines = key_plan_outlines(&set, 5, None);
        assert_eq!(outlines.len(), 1);
        // The paper's bottom-left, 50 mm left and 25 mm down of the centre, is
        // 50 m south and 25 m east.
        near(outlines[0].corners[0].x, 25.0, 1e-9);
        near(outlines[0].corners[0].y, -50.0, 1e-9);
        near(outlines[0].corners[2].x, -25.0, 1e-9);
        near(outlines[0].corners[2].y, 50.0, 1e-9);
    }

    #[test]
    fn an_automatic_plan_is_outlined_where_the_painter_puts_it() {
        let mut set = key_and_two_plans();
        set.sheets[1].viewports[0].auto_scale = true;
        let place = |viewport: &Viewport| {
            assert_eq!(viewport.id, "vp2");
            PlanPlacement {
                scale: 2000.0,
                centre: Point2::new(0.0, 0.0),
            }
        };
        let placed = key_plan_outlines(&set, 0, Some(&place));
        // 100 x 50 mm at 1:2000 over the origin.
        near(placed[0].corners[0].x, -100.0, 1e-9);
        near(placed[0].corners[2].y, 50.0, 1e-9);
        // Without a placer, the stored scale and centre, as last drawn.
        near(
            key_plan_outlines(&set, 0, None)[0].corners[0].x,
            950.0,
            1e-9,
        );
    }

    #[test]
    fn an_inset_of_a_sheets_own_plan_does_not_repeat_its_number() {
        let mut set = key_and_two_plans();
        // A detail of sheet 2 at 1:250, inside its main plan; and a plan of
        // somewhere else on the same sheet.
        set.sheets[1].viewports.push(plan_of(
            "vp5",
            Box2::new(Point2::new(0.0, 60.0), Point2::new(40.0, 80.0)),
            250.0,
            Point2::new(1010.0, 2005.0),
            0.0,
        ));
        set.sheets[1].viewports.push(plan_of(
            "vp6",
            strip(),
            1000.0,
            Point2::new(5000.0, 5000.0),
            0.0,
        ));
        let outlines = key_plan_outlines(&set, 0, None);
        assert_eq!(outlines.len(), 4);
        assert_eq!(outlines[0].viewport_id, "vp2");
        assert!(outlines[0].labelled);
        assert_eq!(outlines[1].viewport_id, "vp5");
        assert!(!outlines[1].labelled);
        assert_eq!(outlines[2].viewport_id, "vp6");
        assert!(outlines[2].labelled);
        // Sheet 3's plan overlaps sheet 2's but is another sheet: numbered.
        assert!(outlines[3].labelled);
    }

    #[test]
    fn an_automatic_plan_is_fitted_by_the_painters_rule() {
        let mut viewport = plan_of("vp1", strip(), 500.0, Point2::default(), 0.0);
        viewport.auto_scale = true;
        viewport.auto_centre = true;
        let points = [Point2::new(0.0, 0.0), Point2::new(300.0, 100.0)];
        // 300 m across 100 mm needs 1:3000, 100 m up 50 mm 1:2000; with 4% to
        // spare 1:3120, and the ladder's next is 1:5000.
        let fitted = fit_plan_placement(&viewport, &points);
        double_eq(fitted.scale, 5000.0);
        near(fitted.centre.x, 150.0, 1e-9);
        near(fitted.centre.y, 50.0, 1e-9);

        // Only what is automatic changes.
        viewport.auto_centre = false;
        assert_eq!(
            fit_plan_placement(&viewport, &points).centre,
            Point2::default()
        );
        viewport.auto_scale = false;
        assert_eq!(
            fit_plan_placement(&viewport, &points),
            stored_placement(&viewport)
        );
        viewport.auto_scale = true;
        assert_eq!(
            fit_plan_placement(&viewport, &[]),
            stored_placement(&viewport)
        );
    }

    #[test]
    fn an_automatic_key_plan_frames_the_outlines_and_the_drawing_only_without_them() {
        let set = key_and_two_plans();
        // 100 x 70 mm.
        let mut key = set.sheets[0].viewports[0].clone();
        key.auto_scale = true;
        key.auto_centre = true;
        let outlines = key_plan_outlines(&set, 0, None);

        // The outlines span 950..1150 x 1975..2025: 200 m across 100 mm with
        // 4% to spare is 1:2080, so 1:2500, centred on them.
        let tight = fit_key_plan(&key, &outlines, &Box2::default());
        double_eq(tight.scale, 2500.0);
        near(tight.centre.x, 1050.0, 1e-9);
        near(tight.centre.y, 2000.0, 1e-9);
        for outline in &outlines {
            for &corner in &outline.corners {
                assert!(key.rect.contains(plan_world_to_paper(&key, &tight, corner)));
            }
        }

        // Drawing far off (a stray down at the origin) does not shrink the
        // sheets: the key plan frames them alone.
        let drawing = Box2::new(Point2::new(0.0, 0.0), Point2::new(10.0, 10.0));
        assert_eq!(fit_key_plan(&key, &outlines, &drawing), tight);

        // With no sheet to frame it shows the drawing.
        let alone = fit_key_plan(&key, &[], &drawing);
        near(alone.centre.x, 5.0, 1e-9);
        near(alone.centre.y, 5.0, 1e-9);
        assert!(alone.scale < tight.scale);
        // And with nothing at all it keeps its own.
        assert_eq!(
            fit_key_plan(&key, &[], &Box2::default()),
            stored_placement(&key)
        );
    }

    #[test]
    fn headless_a_plan_is_placed_over_the_model_and_a_key_plan_over_every_sheet() {
        // A 300 x 100 m drawing: a line, and a road along its bottom edge.
        let mut model = Model::new();
        let line = Entity {
            geometry: Geometry::Line(Segment2::new(
                Point2::new(0.0, 0.0),
                Point2::new(300.0, 100.0),
            )),
            ..Entity::default()
        };
        ok(model.entities.add(line));
        let mut road = Alignment {
            name: String::from("ROAD"),
            ..Alignment::default()
        };
        road.horizontal.pis = vec![
            AlignmentPi {
                point: Point2::new(0.0, 0.0),
                ..AlignmentPi::default()
            },
            AlignmentPi {
                point: Point2::new(300.0, 0.0),
                ..AlignmentPi::default()
            },
        ];
        ok(model.alignments.add(road));

        // An automatic plan over the drawing: 300 m across 100 mm needs 1:3120
        // with 4% to spare, so 1:5000, centred on the drawing.
        let mut plan = plan_of("vp2", strip(), 500.0, Point2::default(), 0.0);
        plan.auto_scale = true;
        plan.auto_centre = true;
        let placed = place_plan(&model, &plan);
        double_eq(placed.scale, 5000.0);
        near(placed.centre.x, 150.0, 1e-9);
        near(placed.centre.y, 50.0, 1e-9);
        // Along the road's first 100 m: 100 m in 100 mm is 1:1040, so 1:1250.
        let mut along = plan.clone();
        along.source.alignment = String::from("ROAD");
        along.source.chainage_to = 100.0;
        let along_road = place_plan(&model, &along);
        double_eq(along_road.scale, 1250.0);
        near(along_road.centre.x, 50.0, 1e-9);
        near(along_road.centre.y, 0.0, 1e-9);
        // No such alignment: the drawing, as the painter falls back.
        along.source.alignment = String::from("NOWHERE");
        assert_eq!(place_plan(&model, &along), placed);
        // A fixed plan keeps its own; so does any plan over an empty model.
        let fixed = plan_of("vp3", strip(), 1000.0, Point2::new(1100.0, 2000.0), 0.0);
        assert_eq!(place_plan(&model, &fixed), stored_placement(&fixed));
        assert_eq!(place_plan(&Model::new(), &plan), stored_placement(&plan));

        // A key plan on sheet 1 over the automatic plan (sheet 2) and the
        // fixed one far off (sheet 3): it frames both outlines.
        let mut key = key_plan_of(
            "vp1",
            Box2::new(Point2::new(30.0, 40.0), Point2::new(130.0, 110.0)),
        );
        key.auto_scale = true;
        key.auto_centre = true;
        let set = SheetSet {
            sheets: vec![
                sheet_of("s1", vec![key.clone()]),
                sheet_of("s2", vec![plan.clone()]),
                sheet_of("s3", vec![fixed]),
            ],
            ..SheetSet::default()
        };
        let placer = model_placer(&model);
        let outlines = key_plan_outlines(&set, 0, Some(&placer));
        assert_eq!(outlines.len(), 2);
        // Sheet 2's is where place_plan put it: 500 x 250 m about (150, 50).
        near(outlines[0].corners[0].x, -100.0, 1e-9);
        near(outlines[0].corners[2].y, 175.0, 1e-9);
        let key_at = place_plan_on_sheet(&model, &set, 0, &key);
        assert_eq!(key_at, fit_key_plan(&key, &outlines, &Box2::default()));
        for outline in &outlines {
            for &corner in &outline.corners {
                assert!(
                    key.rect
                        .contains(plan_world_to_paper(&key, &key_at, corner))
                );
            }
        }
        // The key plan on a sheet of its own is placed like any plan
        // elsewhere.
        let plain = Viewport {
            kind: ViewportKind::Plan,
            ..key.clone()
        };
        assert_eq!(
            place_plan_on_sheet(&model, &set, 0, &plain),
            place_plan(&model, &plain)
        );
    }
}
