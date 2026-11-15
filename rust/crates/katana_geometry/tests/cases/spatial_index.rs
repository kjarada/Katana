use crate::support::{Random32, double_eq};
use katana_geometry::{Box2, Point2, SpatialEntry, SpatialId, SpatialIndex};
fn box_at(east: f64, north: f64, width: f64, height: f64) -> Box2 {
    Box2::new(
        Point2::new(east, north),
        Point2::new(east + width, north + height),
    )
}
fn brute_force(entries: &[SpatialEntry], query: Box2) -> Vec<SpatialId> {
    let mut output: Vec<_> = entries
        .iter()
        .filter(|entry| !entry.box_value.empty() && entry.box_value.intersects(query))
        .map(|entry| entry.id)
        .collect();
    output.sort();
    output
}
fn awkward_data(count: usize) -> Vec<SpatialEntry> {
    let mut random = Random32::new(20260920);
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let kind = index % 10;
        let bounds = if kind < 4 {
            box_at(
                random.real(0.0, 5.0),
                random.real(0.0, 5.0),
                random.real(0.01, 2.0),
                random.real(0.01, 2.0),
            )
        } else if kind < 7 {
            box_at(
                300.0 + random.real(0.0, 5.0),
                -200.0 + random.real(0.0, 5.0),
                random.real(0.01, 2.0),
                random.real(0.01, 2.0),
            )
        } else if kind < 9 {
            box_at(
                random.real(-500.0, 500.0),
                random.real(-500.0, 500.0),
                random.real(0.01, 2.0),
                random.real(0.01, 2.0),
            )
        } else if index % 97 == 0 {
            box_at(
                random.real(-500.0, 500.0),
                random.real(-500.0, 500.0),
                800.0,
                800.0,
            )
        } else {
            let east = random.real(-500.0, 500.0);
            let north = random.real(-500.0, 500.0);
            Box2::new(Point2::new(east, north), Point2::new(east, north))
        };
        entries.push(SpatialEntry {
            id: (index + 1) as SpatialId,
            box_value: bounds,
        });
    }
    entries
}
mod spatial_index {
    use super::*;
    #[test]
    fn an_empty_index_answers_nothing() {
        let index = SpatialIndex::default();
        assert!(index.empty());
        assert_eq!(index.size(), 0);
        assert!(index.query(box_at(0.0, 0.0, 10.0, 10.0)).is_empty());
        assert!(index.bounds().empty());
    }
    #[test]
    fn finds_what_overlaps_and_only_that() {
        let mut index = SpatialIndex::default();
        index.insert(1, box_at(0.0, 0.0, 10.0, 10.0));
        index.insert(2, box_at(20.0, 20.0, 10.0, 10.0));
        index.insert(3, box_at(5.0, 5.0, 10.0, 10.0));
        assert_eq!(index.query(box_at(1.0, 1.0, 1.0, 1.0)), vec![1]);
        assert_eq!(index.query(box_at(6.0, 6.0, 1.0, 1.0)), vec![1, 3]);
        assert_eq!(index.query(box_at(21.0, 21.0, 1.0, 1.0)), vec![2]);
        assert!(index.query(box_at(100.0, 100.0, 1.0, 1.0)).is_empty());
        assert_eq!(index.query(box_at(10.0, 10.0, 0.0, 0.0)), vec![1, 3]);
    }
    #[test]
    fn results_are_sorted_so_tie_breaks_are_meaningful() {
        let mut index = SpatialIndex::default();
        for id in (1..=50).rev() {
            index.insert(id, box_at(0.0, 0.0, 100.0, 100.0));
        }
        let found = index.query(box_at(50.0, 50.0, 1.0, 1.0));
        assert_eq!(found.len(), 50);
        assert!(found.is_sorted());
        assert_eq!(found[0], 1);
        assert_eq!(*found.last().unwrap(), 50);
    }
    #[test]
    fn an_entity_spanning_many_cells_is_returned_once_not_once_per_cell() {
        let mut index = SpatialIndex::default();
        index.rebuild(&[SpatialEntry {
            id: 1,
            box_value: box_at(0.0, 0.0, 1.0, 1.0),
        }]);
        index.insert(2, box_at(-50.0, -50.0, 100.0, 100.0));
        let found = index.query(box_at(0.0, 0.0, 30.0, 30.0));
        assert_eq!(found.iter().filter(|&&id| id == 2).count(), 1);
    }
    #[test]
    fn agrees_with_brute_force_on_awkward_data() {
        let entries = awkward_data(4000);
        let mut index = SpatialIndex::default();
        index.rebuild(&entries);
        let mut random = Random32::new(7);
        let mut non_empty = 0;
        for _ in 0..400 {
            let east = random.real(-600.0, 600.0);
            let north = random.real(-600.0, 600.0);
            let query = box_at(
                east,
                north,
                random.real(0.0, 120.0),
                random.real(0.0, 120.0),
            );
            let expected = brute_force(&entries, query);
            assert_eq!(index.query(query), expected);
            if !expected.is_empty() {
                non_empty += 1;
            }
        }
        assert!(non_empty > 200);
    }
    #[test]
    fn still_agrees_with_brute_force_after_edits() {
        let mut entries = awkward_data(1500);
        let mut index = SpatialIndex::default();
        index.rebuild(&entries);
        let mut random = Random32::new(11);
        for round in 0..40 {
            for current in (0..entries.len()).step_by(37) {
                entries[current].box_value = box_at(
                    random.real(-500.0, 500.0),
                    random.real(-500.0, 500.0),
                    3.0,
                    3.0,
                );
                index.insert(entries[current].id, entries[current].box_value);
            }
            for current in (5..entries.len()).step_by(211) {
                if entries[current].id != 0 {
                    assert!(
                        index.remove(entries[current].id) || entries[current].box_value.empty()
                    );
                    entries[current].box_value = Box2::default();
                }
            }
            let fresh = (100000 + round) as SpatialId;
            let bounds = box_at(
                random.real(-500.0, 500.0),
                random.real(-500.0, 500.0),
                8.0,
                8.0,
            );
            index.insert(fresh, bounds);
            entries.push(SpatialEntry {
                id: fresh,
                box_value: bounds,
            });
            let query = box_at(
                random.real(-500.0, 500.0),
                random.real(-500.0, 500.0),
                60.0,
                60.0,
            );
            assert_eq!(index.query(query), brute_force(&entries, query));
        }
    }
    #[test]
    fn reinserting_an_id_replaces_it_rather_than_duplicating_it() {
        let mut index = SpatialIndex::default();
        index.insert(1, box_at(0.0, 0.0, 5.0, 5.0));
        index.insert(1, box_at(100.0, 100.0, 5.0, 5.0));
        assert_eq!(index.size(), 1);
        assert!(index.query(box_at(1.0, 1.0, 1.0, 1.0)).is_empty());
        assert_eq!(index.query(box_at(101.0, 101.0, 1.0, 1.0)), vec![1]);
    }
    #[test]
    fn removing_reports_whether_anything_was_there() {
        let mut index = SpatialIndex::default();
        index.insert(1, box_at(0.0, 0.0, 5.0, 5.0));
        assert!(index.contains(1));
        assert!(index.remove(1));
        assert!(!index.contains(1));
        assert!(!index.remove(1));
        assert!(!index.remove(999));
        assert!(index.empty());
    }
    #[test]
    fn degenerate_and_non_finite_boxes_are_dropped_not_filed_somewhere_arbitrary() {
        let mut index = SpatialIndex::default();
        index.insert(1, box_at(0.0, 0.0, 5.0, 5.0));
        index.insert(2, Box2::default());
        index.insert(
            3,
            Box2::new(Point2::new(f64::NAN, 0.0), Point2::new(1.0, 1.0)),
        );
        index.insert(
            4,
            Box2::new(Point2::new(0.0, 0.0), Point2::new(f64::INFINITY, 1.0)),
        );
        assert_eq!(index.size(), 1);
        assert_eq!(index.query(box_at(-1e6, -1e6, 2e6, 2e6)), vec![1]);
    }
    #[test]
    fn replacing_a_good_box_with_a_bad_one_removes_the_entry() {
        let mut index = SpatialIndex::default();
        index.insert(1, box_at(0.0, 0.0, 5.0, 5.0));
        index.insert(1, Box2::default());
        assert!(!index.contains(1));
        assert!(index.query(box_at(1.0, 1.0, 1.0, 1.0)).is_empty());
    }
    #[test]
    fn a_zero_extent_point_is_found_at_its_position() {
        let mut index = SpatialIndex::default();
        index.insert(
            1,
            Box2::new(Point2::new(10.0, 20.0), Point2::new(10.0, 20.0)),
        );
        assert_eq!(index.size(), 1);
        assert_eq!(index.query(box_at(9.5, 19.5, 1.0, 1.0)), vec![1]);
        assert!(index.query(box_at(50.0, 50.0, 1.0, 1.0)).is_empty());
    }
    #[test]
    fn the_radius_query_is_the_box_query_of_that_radius() {
        let mut index = SpatialIndex::default();
        index.insert(1, box_at(0.0, 0.0, 1.0, 1.0));
        index.insert(2, box_at(10.0, 0.0, 1.0, 1.0));
        let mut output = Vec::new();
        index.query_radius(Point2::new(0.5, 0.5), 0.5, &mut output);
        assert_eq!(output, vec![1]);
        index.query_radius(Point2::new(0.5, 0.5), 20.0, &mut output);
        assert_eq!(output, vec![1, 2]);
        index.query_radius(Point2::new(0.5, 0.5), -1.0, &mut output);
        assert!(output.is_empty());
        index.query_radius(Point2::new(0.5, 0.5), f64::NAN, &mut output);
        assert!(output.is_empty());
    }
    #[test]
    fn a_query_covering_the_whole_drawing_still_returns_everything_exactly() {
        let entries = awkward_data(2000);
        let mut index = SpatialIndex::default();
        index.rebuild(&entries);
        let everything = box_at(-1.0e6, -1.0e6, 2.0e6, 2.0e6);
        assert_eq!(index.query(everything), brute_force(&entries, everything));
    }
    #[test]
    fn rebuilding_is_equivalent_to_inserting_one_at_a_time() {
        let entries = awkward_data(800);
        let mut rebuilt = SpatialIndex::default();
        rebuilt.rebuild(&entries);
        let mut incremental = SpatialIndex::default();
        for entry in &entries {
            incremental.insert(entry.id, entry.box_value);
        }
        assert_eq!(rebuilt.size(), incremental.size());
        let mut random = Random32::new(3);
        for _ in 0..120 {
            let query = box_at(
                random.real(-600.0, 600.0),
                random.real(-600.0, 600.0),
                40.0,
                40.0,
            );
            assert_eq!(rebuilt.query(query), incremental.query(query));
        }
    }
    #[test]
    fn clear_leaves_it_empty_and_reusable() {
        let mut index = SpatialIndex::default();
        index.rebuild(&awkward_data(500));
        assert!(!index.empty());
        index.clear();
        assert!(index.empty());
        assert_eq!(index.size(), 0);
        assert!(index.query(box_at(-1e6, -1e6, 2e6, 2e6)).is_empty());
        index.insert(1, box_at(0.0, 0.0, 1.0, 1.0));
        assert_eq!(index.query(box_at(0.0, 0.0, 1.0, 1.0)), vec![1]);
    }
    #[test]
    fn bounds_cover_everything_indexed() {
        let mut index = SpatialIndex::default();
        index.insert(1, box_at(0.0, 0.0, 10.0, 10.0));
        index.insert(2, box_at(-50.0, 20.0, 5.0, 5.0));
        let bounds = index.bounds();
        assert!(!bounds.empty());
        double_eq(bounds.min.x, -50.0);
        double_eq(bounds.min.y, 0.0);
        double_eq(bounds.max.x, 10.0);
        double_eq(bounds.max.y, 25.0);
    }
}
