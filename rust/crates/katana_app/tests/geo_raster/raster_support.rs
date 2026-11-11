//! What the raster verbs' cases share beyond geo_fixture: the text grids
//! their C++ fixtures write, a reply's records of one kind, gtest's
//! EXPECT_DOUBLE_EQ, and whether a folder is empty. One owner (R), private to
//! this target.

#![allow(
    dead_code,
    reason = "Each file of the target uses a part of the support."
)]

use std::path::Path;

use katana_app::geo::replies::{Record, parse_records};

use crate::geo_fixture::TempDir;

/// An ESRI ASCII grid of `columns` x `rows` cells of 1 m from (0,0), written
/// to `name` in `folder`: the cell at (column, row) as `cell` writes it, row 0
/// the northern (top) row. Its path, as a line names it.
pub fn ascii_grid(
    folder: &TempDir,
    name: &str,
    columns: i32,
    rows: i32,
    cell: impl Fn(i32, i32) -> String,
) -> String {
    let path = folder.file(name);
    let mut text = format!("ncols {columns}\nnrows {rows}\nxllcorner 0\nyllcorner 0\ncellsize 1\n");
    for row in 0..rows {
        for column in 0..columns {
            text += &cell(column, row);
            text.push(if column + 1 < columns { ' ' } else { '\n' });
        }
    }
    if let Err(error) = std::fs::write(&path, text) {
        panic!("{path}: {error}");
    }
    path
}

/// The records of `kind` in a reply, in order.
pub fn records(reply: &str, kind: &str) -> Vec<Record> {
    parse_records(reply)
        .into_iter()
        .filter(|record| record.kind == kind)
        .collect()
}

/// std::stod of a field, "nan" when it is missing: a NaN no tolerance holds.
pub fn stod(text: Option<&str>) -> f64 {
    text.unwrap_or("nan").parse().unwrap_or(f64::NAN)
}

/// EXPECT_DOUBLE_EQ: within four units in the last place, as gtest compares.
#[track_caller]
pub fn double_eq(actual: f64, expected: f64) {
    fn biased(value: f64) -> u64 {
        let bits = value.to_bits();
        if bits & (1 << 63) != 0 {
            (!bits).wrapping_add(1)
        } else {
            bits | (1 << 63)
        }
    }
    assert!(
        !actual.is_nan() && !expected.is_nan() && biased(actual).abs_diff(biased(expected)) <= 4,
        "actual={actual:?}, expected={expected:?}"
    );
}

/// std::filesystem::is_empty of a folder: one with no entries.
pub fn is_empty_dir(path: &Path) -> bool {
    std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
}
