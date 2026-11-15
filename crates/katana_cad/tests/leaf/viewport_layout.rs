// The layout presets and how they become docked views
// (tests/cad/test_viewport_layout.cpp).
//
// Since the dock workspace of 2026-09-23 a preset is no longer a tiling that
// Katana computes pixels for - the window owns the pixels - but an
// ARRANGEMENT: layout_rects says what "Three: Left" looks like, and
// dock_splits says how to build it out of docks by halving one view at a
// time. The test that matters is that the two agree exactly, so the picture
// in the menu and the docks it produces cannot drift apart. It simulates each
// split on rectangles the way the window splits a dock: the existing view
// keeps the left or top half, the added view gets the right or bottom.
//
// Where the retired ViewportLayout class's tests went is listed at the top of
// the C++ file.

use std::collections::BTreeSet;

use crate::near;
use katana_cad::viewport_layout::{cell_count, default_view_kind, dock_splits, layout_rects};
use katana_cad::{CellRect, DockSplit, LayoutKind, ViewKind};
use katana_entity::EnumName;

const ALL_LAYOUTS: [LayoutKind; 6] = [
    LayoutKind::Single,
    LayoutKind::SplitVertical,
    LayoutKind::SplitHorizontal,
    LayoutKind::ThreeLeft,
    LayoutKind::ThreeTop,
    LayoutKind::Quad,
];

// What each preset's name promises, counted independently of layout_rects.
fn views_promised(kind: LayoutKind) -> usize {
    match kind {
        LayoutKind::Single => 1,
        LayoutKind::SplitVertical | LayoutKind::SplitHorizontal => 2,
        LayoutKind::ThreeLeft | LayoutKind::ThreeTop => 3,
        LayoutKind::Quad => 4,
    }
}

// Applies the steps to rectangles. Every value involved is a sum of powers of
// two (1, 1/2, 1/4), so the halving is exact and the comparison below can be
// ==, not "near".
fn simulate_docking(steps: &[DockSplit], views: usize) -> Vec<CellRect> {
    let mut placed = vec![CellRect::default(); views];
    let mut is_placed = vec![false; views];
    placed[0] = CellRect::new(0.0, 0.0, 1.0, 1.0);
    is_placed[0] = true;
    for step in steps {
        assert!(step.existing < views);
        assert!(step.added < views);
        assert!(
            is_placed[step.existing],
            "a split can only halve a view already placed"
        );
        assert!(!is_placed[step.added], "each view is placed exactly once");
        let existing = &mut placed[step.existing];
        let mut added = *existing;
        if step.side_by_side {
            existing.width /= 2.0;
            added.x = existing.x + existing.width;
            added.width = existing.width;
        } else {
            existing.height /= 2.0;
            added.y = existing.y + existing.height;
            added.height = existing.height;
        }
        placed[step.added] = added;
        is_placed[step.added] = true;
    }
    for (i, &placed_here) in is_placed.iter().enumerate() {
        assert!(placed_here, "view {i} was never placed");
    }
    placed
}

mod viewport_layout {
    use super::*;

    // ---- the presets --------------------------------------------------------

    #[test]
    fn each_preset_has_as_many_views_as_its_name_says() {
        for kind in ALL_LAYOUTS {
            assert_eq!(cell_count(kind), views_promised(kind), "{}", kind.name());
            assert_eq!(layout_rects(kind).len(), views_promised(kind));
        }
    }

    #[test]
    fn every_layout_tiles_the_area_exactly_once() {
        for kind in ALL_LAYOUTS {
            let rects = layout_rects(kind);
            assert!(!rects.is_empty());

            let mut area = 0.0;
            for rect in &rects {
                assert!(rect.width > 0.0);
                assert!(rect.height > 0.0);
                assert!(rect.x >= 0.0);
                assert!(rect.y >= 0.0);
                assert!(rect.x + rect.width <= 1.0 + 1e-12);
                assert!(rect.y + rect.height <= 1.0 + 1e-12);
                area += rect.width * rect.height;
            }
            // Tiled, not floating: the cells cover the area and do not
            // overlap, so the areas must add to exactly one.
            near(area, 1.0, 1e-12);
        }
    }

    #[test]
    fn every_point_of_the_area_belongs_to_exactly_one_rect() {
        // A fine sweep including the shared borders at 0.5, where a closed
        // rectangle test would match two. Asked of layout_rects directly: the
        // menu draws its pictures from them.
        for kind in ALL_LAYOUTS {
            let rects = layout_rects(kind);
            for i in 0..40 {
                for j in 0..40 {
                    let u = f64::from(i) / 40.0;
                    let v = f64::from(j) / 40.0;
                    let matches = rects.iter().filter(|rect| rect.contains(u, v)).count();
                    assert_eq!(matches, 1, "{} at {u},{v}", kind.name());
                }
            }
        }
    }

    #[test]
    fn points_on_the_far_edge_or_outside_the_area_belong_to_no_rect() {
        let rects = layout_rects(LayoutKind::Quad);
        let in_any = |u: f64, v: f64| rects.iter().any(|rect| rect.contains(u, v));
        assert!(!in_any(-0.01, 0.5));
        assert!(!in_any(0.5, 1.0), "the far edge is exclusive");
        assert!(!in_any(1.0, 0.5));
        assert!(!in_any(f64::NAN, 0.5));
        assert!(in_any(0.0, 0.0), "the near edge is inclusive");
    }

    #[test]
    fn every_layout_and_view_kind_has_a_distinct_name() {
        let mut names = BTreeSet::new();
        for kind in ALL_LAYOUTS {
            let name = kind.name();
            assert_ne!(name, "Unknown");
            assert!(names.insert(name), "duplicate layout name {name}");
        }
        names.clear();
        for kind in [
            ViewKind::Plan,
            ViewKind::Model3D,
            ViewKind::Section,
            ViewKind::Elevation,
        ] {
            let name = kind.name();
            assert_ne!(name, "Unknown");
            assert!(names.insert(name), "duplicate view name {name}");
        }
    }

    // ---- the kinds a preset opens -------------------------------------------

    #[test]
    fn an_arrangement_opens_plan_three_d_section_and_elevation_in_that_order() {
        // The four-up arrangement a civil engineer expects, and the first
        // slot is always the plan the user was already drawing in.
        assert_eq!(default_view_kind(0), ViewKind::Plan);
        assert_eq!(default_view_kind(1), ViewKind::Model3D);
        assert_eq!(default_view_kind(2), ViewKind::Section);
        assert_eq!(default_view_kind(3), ViewKind::Elevation);
    }

    #[test]
    fn slots_past_the_fourth_repeat_the_last_kind() {
        for slot in [4, 5, 100, usize::MAX] {
            assert_eq!(default_view_kind(slot), ViewKind::Elevation, "{slot}");
        }
    }

    // ---- dock_splits ---------------------------------------------------------

    #[test]
    fn docking_the_splits_reproduces_every_presets_rectangles_exactly() {
        for kind in ALL_LAYOUTS {
            let expected = layout_rects(kind);
            let steps = dock_splits(kind);
            assert_eq!(
                steps.len(),
                expected.len() - 1,
                "one split per view after the first"
            );

            let docked = simulate_docking(&steps, expected.len());
            assert_eq!(docked.len(), expected.len(), "{}", kind.name());
            for (i, (docked_rect, expected_rect)) in docked.iter().zip(&expected).enumerate() {
                assert_eq!(docked_rect, expected_rect, "{} view {i}", kind.name());
            }
        }
    }

    #[test]
    fn a_single_view_needs_no_split() {
        assert!(dock_splits(LayoutKind::Single).is_empty());
    }

    #[test]
    fn quad_halves_the_top_row_before_splitting_each_half_downwards() {
        // Worked by hand: splitting 0 side by side gives two half-width
        // columns; splitting each column downwards gives four quarters.
        // Splitting 0 downwards first and then 0 again side by side would
        // leave view 1 a full-width half - the order is the contract, so it
        // is pinned.
        let expected = vec![
            DockSplit::new(0, 1, true),
            DockSplit::new(0, 2, false),
            DockSplit::new(1, 3, false),
        ];
        assert_eq!(dock_splits(LayoutKind::Quad), expected);
    }
}
