//! The spatial index against a scan of every box, on the keys its own hash must tell apart
//! (docs/performance.md, "The spatial index's hashing"; CLAUDE.md section 5).
//!
//! The index hashes entity ids and packed cell numbers itself. A query's answer must not depend
//! on that hash, so each query here is answered twice - by the index and by comparing every box -
//! over ids and cells chosen to look alike to a weak hash: consecutive ids, ids that differ only
//! above bit 32, the largest ids, and cells either side of zero on both axes, whose packed keys
//! differ in a single bit or in every bit of a half.

use katana_geometry::{Box2, Point2, SpatialEntry, SpatialId, SpatialIndex};

fn box_at(east: f64, north: f64, width: f64, height: f64) -> Box2 {
    Box2::new(
        Point2::new(east, north),
        Point2::new(east + width, north + height),
    )
}

fn scan(entries: &[SpatialEntry], query: Box2) -> Vec<SpatialId> {
    let mut found: Vec<SpatialId> = entries
        .iter()
        .filter(|entry| entry.box_value.intersects(query))
        .map(|entry| entry.id)
        .collect();
    found.sort_unstable();
    found
}

/// A 60 x 60 grid of unit-and-a-bit boxes centred on the origin, so the cells run from about
/// -30 to 30 on both axes, with ids from four ranges a weak hash would fold together.
fn entries() -> Vec<SpatialEntry> {
    let mut entries = Vec::new();
    for row in 0..60_u64 {
        for column in 0..60_u64 {
            let number = row * 60 + column;
            let id = match number % 4 {
                0 => number + 1,
                1 => (number + 1) << 32,
                2 => u64::MAX - number,
                _ => ((number + 1) << 32) | (number + 1),
            };
            entries.push(SpatialEntry {
                id,
                box_value: box_at(
                    column as f64 * 2.0 - 60.0,
                    row as f64 * 2.0 - 60.0,
                    1.25,
                    0.75,
                ),
            });
        }
    }
    entries
}

fn queries() -> Vec<Box2> {
    let mut queries = vec![
        // Across both axes' zero, where the cell numbers change sign.
        box_at(-3.0, -3.0, 6.0, 6.0),
        // One cell's worth, in each quadrant.
        box_at(10.0, 10.0, 1.0, 1.0),
        box_at(-10.5, 10.0, 1.0, 1.0),
        box_at(-10.5, -10.5, 1.0, 1.0),
        box_at(10.0, -10.5, 1.0, 1.0),
        // A row and a column of the grid.
        box_at(-61.0, 0.0, 122.0, 0.5),
        box_at(0.0, -61.0, 0.5, 122.0),
        // Everything, which is wider than the index looks up cell by cell.
        box_at(-100.0, -100.0, 200.0, 200.0),
        // Nothing.
        box_at(500.0, 500.0, 1.0, 1.0),
    ];
    for step in 0..40 {
        let at = step as f64 * 3.0 - 60.0;
        queries.push(box_at(at, -at, 4.5, 2.5));
    }
    queries
}

fn assert_answers_as_the_scan(index: &SpatialIndex, entries: &[SpatialEntry], what: &str) {
    let mut reached = 0;
    for (number, query) in queries().into_iter().enumerate() {
        let expected = scan(entries, query);
        reached += expected.len();
        assert_eq!(index.query(query), expected, "{what}, query {number}");
    }
    // The queries found boxes, so an index answering nothing would not pass.
    assert!(reached > entries.len(), "{what}: {reached}");
}

#[test]
fn a_rebuilt_index_answers_as_a_scan_of_every_box_does() {
    let entries = entries();
    let mut index = SpatialIndex::default();
    index.rebuild(&entries);
    assert_eq!(index.size(), entries.len());
    assert_answers_as_the_scan(&index, &entries, "rebuilt");
}

#[test]
fn an_index_filled_one_box_at_a_time_answers_as_the_scan_does() {
    let entries = entries();
    let mut index = SpatialIndex::default();
    for entry in &entries {
        index.insert(entry.id, entry.box_value);
    }
    assert_eq!(index.size(), entries.len());
    assert_answers_as_the_scan(&index, &entries, "inserted");
}

#[test]
fn removing_and_moving_boxes_keeps_the_answers_the_scans() {
    let mut entries = entries();
    let mut index = SpatialIndex::default();
    index.rebuild(&entries);
    // Every third box goes, and every third of the rest moves across the origin.
    let mut kept = Vec::new();
    for (number, mut entry) in entries.drain(..).enumerate() {
        match number % 3 {
            0 => {
                assert!(index.remove(entry.id));
                assert!(!index.contains(entry.id));
            }
            1 => {
                entry.box_value = box_at(
                    -entry.box_value.max.x,
                    -entry.box_value.max.y,
                    1.25,
                    0.75,
                );
                index.insert(entry.id, entry.box_value);
                kept.push(entry);
            }
            _ => kept.push(entry),
        }
    }
    assert_eq!(index.size(), kept.len());
    assert_answers_as_the_scan(&index, &kept, "after removals and moves");
    for entry in &kept {
        assert!(index.contains(entry.id));
    }
}
