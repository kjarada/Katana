// Tiling presets, tiling ranks and snapping (include/katana/cad/plotting/layout.hpp);
// tests/cad/plotting/test_layout.cpp.
//
// The expected rectangles are worked by hand from the A3 frame's drawing area
// (23..410 x 35..287 mm, resources/plot_frames/a3_landscape.json) inset by
// 1 mm - 24..409 x 36..286, 385 x 250 mm - and the owner's app's cell
// fractions with its 3 mm gutter.

use std::collections::BTreeSet;

use crate::{expect_box, near, rect};
use katana_cad::PaperSize;
use katana_cad::plotting::layout::{
    TILING_GUTTER_MM, TILING_PRESET_COUNT, minimum_size, preset_cells, preset_from_id, preset_id,
    preset_name, snap_rect, tile_viewports, tiling_area, tiling_rank,
};
use katana_cad::plotting::{Sheet, TilingPreset, Viewport, ViewportKind};

fn viewport_of(kind: ViewportKind, id: &str) -> Viewport {
    Viewport {
        id: String::from(id),
        kind,
        rect: rect(0.0, 0.0, 10.0, 10.0),
        ..Viewport::default()
    }
}

fn by_id<'a>(sheet: &'a Sheet, id: &str) -> &'a Viewport {
    sheet
        .viewports
        .iter()
        .find(|viewport| viewport.id == id)
        .unwrap_or_else(|| panic!("no viewport {id}"))
}

const EVERY_KIND: [ViewportKind; 10] = [
    ViewportKind::Plan,
    ViewportKind::LongSection,
    ViewportKind::CrossSections,
    ViewportKind::Model3D,
    ViewportKind::Legend,
    ViewportKind::Notes,
    ViewportKind::Image,
    ViewportKind::KeyPlan,
    ViewportKind::SheetIndex,
    ViewportKind::Revisions,
];

mod sheet_layout {
    use super::*;

    #[test]
    fn the_tiling_area_is_the_drawing_area_inset_by_one_millimetre() {
        let sheet = Sheet::default();
        expect_box(&tiling_area(&sheet), 24.0, 36.0, 409.0, 286.0);
    }

    #[test]
    fn the_full_preset_is_the_tiling_area_less_half_the_gutter_all_round() {
        let cells = preset_cells(
            TilingPreset::Full,
            &tiling_area(&Sheet::default()),
            TILING_GUTTER_MM,
        );
        assert_eq!(cells.len(), 1);
        // 24 + 1.5 .. 409 - 1.5, 36 + 1.5 .. 286 - 1.5.
        expect_box(&cells[0], 25.5, 37.5, 407.5, 284.5);
    }

    #[test]
    fn neighbouring_cells_are_one_gutter_apart() {
        let area = tiling_area(&Sheet::default());
        // Two columns: each 385 / 2 - 3 = 189.5 wide; the second starts at
        // 24 + 192.5 + 1.5 = 218, 3 mm after the first ends at 215.
        let columns = preset_cells(TilingPreset::Columns2, &area, TILING_GUTTER_MM);
        assert_eq!(columns.len(), 2);
        expect_box(&columns[0], 25.5, 37.5, 215.0, 284.5);
        expect_box(&columns[1], 218.0, 37.5, 407.5, 284.5);
        // Main and panel below: the main 0.64 x 250 - 3 = 157 high from the
        // top (284.5 down to 127.5), the panel 0.36 x 250 - 3 = 87 high under
        // it (124.5 down to 37.5).
        let below = preset_cells(TilingPreset::MainBelow, &area, TILING_GUTTER_MM);
        assert_eq!(below.len(), 2);
        expect_box(&below[0], 25.5, 127.5, 407.5, 284.5);
        expect_box(&below[1], 25.5, 37.5, 407.5, 124.5);
        // Quarters: the fourth is bottom-right, from x 218 and up to y
        // 286 - 125 - 1.5 = 159.5.
        let quad = preset_cells(TilingPreset::Quad, &area, TILING_GUTTER_MM);
        assert_eq!(quad.len(), 4);
        expect_box(&quad[3], 218.0, 37.5, 407.5, 159.5);
        // Main and two panels right: the main 0.64 x 385 - 3 = 243.4 wide; the
        // panels 0.36 x 385 - 3 = 135.6 wide from 24 + 246.4 + 1.5 = 271.9,
        // the upper from 284.5 down to 286 - 125 + 1.5 = 162.5.
        let two_right = preset_cells(TilingPreset::MainTwoRight, &area, TILING_GUTTER_MM);
        assert_eq!(two_right.len(), 3);
        expect_box(&two_right[0], 25.5, 37.5, 268.9, 284.5);
        expect_box(&two_right[1], 271.9, 162.5, 407.5, 284.5);
        expect_box(&two_right[2], 271.9, 37.5, 407.5, 159.5);
    }

    #[test]
    fn the_eight_presets_keep_the_owners_app_ids() {
        let ids = [
            (TilingPreset::Full, "full"),
            (TilingPreset::Columns2, "cols2"),
            (TilingPreset::Rows2, "rows2"),
            (TilingPreset::Quad, "quad"),
            (TilingPreset::MainRight, "sectionR"),
            (TilingPreset::MainBelow, "sectionB"),
            (TilingPreset::MainCorner, "sectionBR"),
            (TilingPreset::MainTwoRight, "sectionMap3d"),
        ];
        assert_eq!(ids.len(), TILING_PRESET_COUNT);
        for (preset, id) in ids {
            assert_eq!(preset_id(preset), id);
            assert_eq!(preset_from_id(id), Some(preset));
            assert!(!preset_name(preset).is_empty());
        }
        assert!(preset_from_id("nonsense").is_none());
    }

    #[test]
    fn every_viewport_kind_has_its_own_rank_and_the_main_drawing_comes_first() {
        let ranks: BTreeSet<i32> = EVERY_KIND.iter().map(|&kind| tiling_rank(kind)).collect();
        // Every kind its own rank: no kind falls through a missing table entry
        // the way the owner's app's map panels did.
        assert_eq!(ranks.len(), EVERY_KIND.len());
        assert!(tiling_rank(ViewportKind::Plan) < tiling_rank(ViewportKind::LongSection));
        assert!(tiling_rank(ViewportKind::LongSection) < tiling_rank(ViewportKind::CrossSections));
        assert!(tiling_rank(ViewportKind::CrossSections) < tiling_rank(ViewportKind::Model3D));
        assert!(tiling_rank(ViewportKind::Model3D) < tiling_rank(ViewportKind::KeyPlan));
    }

    #[test]
    fn tiling_puts_the_section_in_the_big_cell_whatever_order_the_viewports_were_added() {
        // The owner's app, given [map, section, 3D] and "Section + map + 3D",
        // left the map in the big cell (its sort had no rank for maps). Here a
        // key plan, cross sections and a 3D view added in the worst order
        // still land: sections in the main cell, 3D top-right, key plan
        // bottom-right.
        let mut sheet = Sheet {
            viewports: vec![
                viewport_of(ViewportKind::KeyPlan, "key"),
                viewport_of(ViewportKind::CrossSections, "xs"),
                viewport_of(ViewportKind::Model3D, "3d"),
            ],
            ..Sheet::default()
        };
        assert_eq!(
            tile_viewports(&mut sheet, TilingPreset::MainTwoRight, TILING_GUTTER_MM),
            3
        );
        expect_box(&by_id(&sheet, "xs").rect, 25.5, 37.5, 268.9, 284.5);
        expect_box(&by_id(&sheet, "3d").rect, 271.9, 162.5, 407.5, 284.5);
        expect_box(&by_id(&sheet, "key").rect, 271.9, 37.5, 407.5, 159.5);
        // The sheet's own order (the drawing order) is left alone.
        assert_eq!(sheet.viewports[0].id, "key");
    }

    #[test]
    fn locked_viewports_notes_and_viewports_beyond_the_cells_stay_where_they_were() {
        let mut sheet = Sheet {
            viewports: vec![
                viewport_of(ViewportKind::Plan, "a"),
                viewport_of(ViewportKind::Plan, "b"),
                viewport_of(ViewportKind::Plan, "c"),
                viewport_of(ViewportKind::Notes, "n"),
                viewport_of(ViewportKind::Legend, "locked"),
            ],
            ..Sheet::default()
        };
        sheet.viewports[4].locked = true;
        // Two columns for three plans: the first two (in sheet order, as ties
        // keep it) are placed, the third is not.
        assert_eq!(
            tile_viewports(&mut sheet, TilingPreset::Columns2, TILING_GUTTER_MM),
            2
        );
        expect_box(&by_id(&sheet, "a").rect, 25.5, 37.5, 215.0, 284.5);
        expect_box(&by_id(&sheet, "b").rect, 218.0, 37.5, 407.5, 284.5);
        expect_box(&by_id(&sheet, "c").rect, 0.0, 0.0, 10.0, 10.0);
        expect_box(&by_id(&sheet, "n").rect, 0.0, 0.0, 10.0, 10.0);
        expect_box(&by_id(&sheet, "locked").rect, 0.0, 0.0, 10.0, 10.0);
    }

    #[test]
    fn a_tiled_viewport_is_never_smaller_than_its_kinds_minimum() {
        // Portrait A4 has no frame: paper less 10 mm, 10..200 x 10..287, tiled
        // inside 11..199 x 11..286 (188 x 275). In "Main and two panels right"
        // the upper panel is 0.36 x 188 - 3 = 64.68 mm wide - under a long
        // section's 70 mm minimum - so it is 70 wide, from 11 + 120.32 + 1.5 =
        // 132.82, and 0.5 x 275 - 3 = 134.5 high down from 284.5.
        let mut sheet = Sheet {
            paper: PaperSize::A4,
            landscape: false,
            frame: String::new(),
            viewports: vec![
                viewport_of(ViewportKind::Plan, "plan"),
                viewport_of(ViewportKind::LongSection, "long"),
            ],
            ..Sheet::default()
        };
        tile_viewports(&mut sheet, TilingPreset::MainTwoRight, TILING_GUTTER_MM);
        expect_box(&by_id(&sheet, "long").rect, 132.82, 150.0, 202.82, 284.5);
        assert_eq!(minimum_size(ViewportKind::LongSection).width, 70.0);
    }

    #[test]
    fn a_moved_viewport_snaps_by_the_smallest_shift_to_an_edge_or_centre() {
        let area = rect(23.0, 35.0, 410.0, 287.0);
        // Another viewport: edges x 160 and 260 (centre 210), y 50 and 120
        // (centre 85).
        let others = [rect(160.0, 50.0, 260.0, 120.0)];
        // The moving one: x 100..158.5, y 100..141 (centres 129.25, 120.5).
        // Its right edge is 1.5 short of the other's left edge; its centre
        // line 0.5 above the other's top edge. Nothing else is within 2.5 mm.
        let snapped = snap_rect(&rect(100.0, 100.0, 158.5, 141.0), &area, &others, 2.5);
        expect_box(&snapped.rect, 101.5, 99.5, 160.0, 140.5);
        assert_eq!(snapped.guide_x, Some(160.0));
        assert_eq!(snapped.guide_y, Some(120.0));
    }

    #[test]
    fn a_snap_prefers_the_nearer_target_and_the_drawing_area_counts() {
        let area = rect(23.0, 35.0, 410.0, 287.0);
        // Left edge 24.2: 1.2 from the area's left edge 23. The right edge 70
        // is 2.0 from another viewport's left edge at 72. The nearer wins.
        let others = [rect(72.0, 200.0, 90.0, 210.0)];
        // Vertically, y 180..196 (centre 188) is 4 mm from the other's bottom
        // edge at 200 and far from the area's centre line at 161: no snap.
        let snapped = snap_rect(&rect(24.2, 180.0, 70.0, 196.0), &area, &others, 2.5);
        near(snapped.rect.min.x, 23.0, 1e-12);
        near(snapped.rect.max.x, 68.8, 1e-12);
        assert_eq!(snapped.guide_x, Some(23.0));
        assert!(snapped.guide_y.is_none());
        assert_eq!(snapped.rect.min.y, 180.0);
    }

    #[test]
    fn nothing_within_tolerance_leaves_the_rectangle_alone() {
        let area = rect(23.0, 35.0, 410.0, 287.0);
        let moving = rect(100.0, 100.0, 150.0, 140.0);
        let snapped = snap_rect(&moving, &area, &[], 1.0);
        expect_box(&snapped.rect, 100.0, 100.0, 150.0, 140.0);
        assert!(snapped.guide_x.is_none());
        assert!(snapped.guide_y.is_none());
        // A zero tolerance is snapping off.
        let off = snap_rect(&rect(23.5, 35.5, 50.0, 60.0), &area, &[], 0.0);
        expect_box(&off.rect, 23.5, 35.5, 50.0, 60.0);
    }
}
