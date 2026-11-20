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
use katana_cad::viewport_layout::{
    TILE_KINDS, TileKind, TileSplit, cell_count, default_view_kind, dock_splits, layout_rects,
    tile_plan, tile_rects, tile_rows,
};
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

// The counts the tilings are checked at: none, one, the small ones a person has, a prime, and
// a dozen.
const TILE_COUNTS: [usize; 8] = [0, 1, 2, 3, 4, 5, 7, 12];

// Applies a tiling's steps to rectangles the way the window cuts a dock: the existing view keeps
// the first of `parts` equal parts of its place, to the left or on top, and the added view is
// given the rest.
fn simulate_tiling(steps: &[TileSplit], views: usize) -> Vec<CellRect> {
    let mut placed = vec![CellRect::default(); views];
    let mut is_placed = vec![false; views];
    if views > 0 {
        placed[0] = CellRect::new(0.0, 0.0, 1.0, 1.0);
        is_placed[0] = true;
    }
    for step in steps {
        assert!(step.existing < views);
        assert!(step.added < views);
        assert!(step.parts >= 2, "a cut leaves the added view a part");
        assert!(
            is_placed[step.existing],
            "a step can only cut a view already placed"
        );
        assert!(!is_placed[step.added], "each view is placed exactly once");
        let existing = &mut placed[step.existing];
        let mut added = *existing;
        let parts = step.parts as f64;
        if step.side_by_side {
            let kept = existing.width / parts;
            added.x = existing.x + kept;
            added.width = existing.width - kept;
            existing.width = kept;
        } else {
            let kept = existing.height / parts;
            added.y = existing.y + kept;
            added.height = existing.height - kept;
            existing.height = kept;
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

    // ---- the tilings ---------------------------------------------------------

    #[test]
    fn a_grid_has_the_fewest_rows_of_at_most_ceil_sqrt_columns_dealt_evenly() {
        // Worked by hand: columns = ceil(sqrt(n)), rows = ceil(n / columns), the
        // views dealt over the rows as evenly as they go, the fuller rows on top.
        // 7: columns 3, rows 3, 7 = 3 + 2 + 2. 12: columns 4, rows 3, 4 each.
        let expected: [(usize, &[usize]); 8] = [
            (0, &[]),
            (1, &[1]),
            (2, &[2]),
            (3, &[2, 1]),
            (4, &[2, 2]),
            (5, &[3, 2]),
            (7, &[3, 2, 2]),
            (12, &[4, 4, 4]),
        ];
        for (count, rows) in expected {
            assert_eq!(tile_rows(count, TileKind::Grid), rows, "{count} views");
        }
    }

    #[test]
    fn side_by_side_is_one_row_and_stacked_is_one_view_a_row() {
        for count in TILE_COUNTS {
            let columns = tile_rows(count, TileKind::Columns);
            let rows = tile_rows(count, TileKind::Rows);
            if count == 0 {
                assert!(columns.is_empty());
                assert!(rows.is_empty());
            } else {
                assert_eq!(columns, [count]);
                assert_eq!(rows, vec![1; count]);
            }
        }
    }

    #[test]
    fn every_tiling_places_every_view_whatever_the_count() {
        // Every count to 200, and a million in a grid (1000 rows of 1000): each
        // view has a row, no row is empty, and a grid's rows differ by one view at
        // most and are never more than its widest row holds.
        for kind in TILE_KINDS {
            for count in 0..=200usize {
                let rows = tile_rows(count, kind);
                assert_eq!(rows.iter().sum::<usize>(), count, "{} {count}", kind.word());
                assert!(rows.iter().all(|&views| views > 0));
                assert_eq!(tile_plan(count, kind).len(), count.saturating_sub(1));
                assert_eq!(tile_rects(count, kind).len(), count);
            }
        }
        for count in 1..=200usize {
            let rows = tile_rows(count, TileKind::Grid);
            let widest = rows.iter().copied().max().unwrap_or(0);
            let narrowest = rows.iter().copied().min().unwrap_or(0);
            assert!(widest - narrowest <= 1, "{count}: {rows:?}");
            assert!(rows.len() <= widest, "{count}: {rows:?}");
            // ceil(sqrt(count)) columns at most: the square of one fewer holds
            // fewer views than there are.
            assert!((widest - 1) * (widest - 1) < count, "{count}: {rows:?}");
        }
        let million = tile_rows(1_000_000, TileKind::Grid);
        assert_eq!(million.len(), 1000);
        assert!(million.iter().all(|&views| views == 1000));
    }

    #[test]
    fn no_views_tile_to_nothing_and_one_view_has_the_whole_area() {
        for kind in TILE_KINDS {
            assert!(tile_rects(0, kind).is_empty(), "{}", kind.word());
            assert!(tile_plan(0, kind).is_empty(), "{}", kind.word());
            assert_eq!(
                tile_rects(1, kind),
                [CellRect::new(0.0, 0.0, 1.0, 1.0)],
                "{}",
                kind.word()
            );
            assert!(tile_plan(1, kind).is_empty(), "{}", kind.word());
        }
    }

    #[test]
    fn every_tiling_covers_the_area_once_with_equal_places_in_each_row() {
        for kind in TILE_KINDS {
            for count in TILE_COUNTS {
                let rects = tile_rects(count, kind);
                let rows = tile_rows(count, kind);
                assert_eq!(rects.len(), count);
                let what = format!("{} of {count}", kind.word());

                // Inside the area, and the areas add to the whole (or to nothing
                // for no view).
                let mut area = 0.0;
                for rect in &rects {
                    assert!(rect.width > 0.0 && rect.height > 0.0, "{what}");
                    assert!(rect.x >= 0.0 && rect.y >= 0.0, "{what}");
                    assert!(rect.x + rect.width <= 1.0 + 1e-12, "{what}");
                    assert!(rect.y + rect.height <= 1.0 + 1e-12, "{what}");
                    area += rect.width * rect.height;
                }
                near(area, if count == 0 { 0.0 } else { 1.0 }, 1e-12);

                // No two overlap: one ends, in x or in y, where the other begins.
                for (at, one) in rects.iter().enumerate() {
                    for other in &rects[at + 1..] {
                        let apart_x = one.x + one.width <= other.x + 1e-12
                            || other.x + other.width <= one.x + 1e-12;
                        let apart_y = one.y + one.height <= other.y + 1e-12
                            || other.y + other.height <= one.y + 1e-12;
                        assert!(apart_x || apart_y, "{what}: {one:?} over {other:?}");
                    }
                }

                // The rows are of one height, 1 / rows, and the views of a row of
                // one width, 1 / the row's views, in the order given.
                let mut next = 0;
                for (row, &views) in rows.iter().enumerate() {
                    for column in 0..views {
                        let rect = rects[next];
                        next += 1;
                        near(rect.height, 1.0 / rows.len() as f64, 1e-12);
                        near(rect.y, row as f64 / rows.len() as f64, 1e-12);
                        near(rect.width, 1.0 / views as f64, 1e-12);
                        near(rect.x, column as f64 / views as f64, 1e-12);
                    }
                }
            }
        }
    }

    #[test]
    fn docking_the_tile_steps_reproduces_every_tilings_rectangles() {
        for kind in TILE_KINDS {
            for count in TILE_COUNTS {
                let expected = tile_rects(count, kind);
                let steps = tile_plan(count, kind);
                assert_eq!(
                    steps.len(),
                    count.saturating_sub(1),
                    "one step per view after the first"
                );
                let docked = simulate_tiling(&steps, count);
                for (i, (docked_rect, expected_rect)) in docked.iter().zip(&expected).enumerate() {
                    let what = format!("{} of {count}, view {i}", kind.word());
                    // Thirds and fifths are not exact in binary: a few ulps.
                    assert!((docked_rect.x - expected_rect.x).abs() < 1e-12, "{what}");
                    assert!((docked_rect.y - expected_rect.y).abs() < 1e-12, "{what}");
                    assert!(
                        (docked_rect.width - expected_rect.width).abs() < 1e-12,
                        "{what}"
                    );
                    assert!(
                        (docked_rect.height - expected_rect.height).abs() < 1e-12,
                        "{what}"
                    );
                }
            }
        }
    }

    #[test]
    fn three_in_a_row_are_cut_a_third_then_a_half_never_halved_twice() {
        // Worked by hand. Halving twice, as a preset's steps do, would leave a half
        // and two quarters; a third off the whole and then half of what is left
        // gives three thirds. The parts are the contract, so they are pinned.
        let step = |existing, added, side_by_side, parts| TileSplit {
            existing,
            added,
            side_by_side,
            parts,
        };
        assert_eq!(
            tile_plan(3, TileKind::Columns),
            [step(0, 1, true, 3), step(1, 2, true, 2)]
        );
        assert_eq!(
            tile_plan(3, TileKind::Rows),
            [step(0, 1, false, 3), step(1, 2, false, 2)]
        );
        // Five in a grid, 3 over 2: the rows first - view 3 begins the second - and
        // then each row from the left.
        assert_eq!(
            tile_plan(5, TileKind::Grid),
            [
                step(0, 3, false, 2),
                step(0, 1, true, 3),
                step(1, 2, true, 2),
                step(3, 4, true, 2),
            ]
        );
    }

    #[test]
    fn every_tiling_has_a_distinct_word_and_menu_name() {
        let mut words = BTreeSet::new();
        let mut names = BTreeSet::new();
        for kind in TILE_KINDS {
            assert!(words.insert(kind.word()), "duplicate word {}", kind.word());
            assert!(names.insert(kind.name()), "duplicate name {}", kind.name());
        }
        assert_eq!(words.len(), 3);
    }
}
