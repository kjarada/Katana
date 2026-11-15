//! What the twins of the reference layers, SURFACE, REFS, INFO and CONTOUR
//! share (step 6, wave 2, agent A): the data folders as the C++ names them,
//! the records of a reply, and the asserts the C++ files write the same way.
//!
//! The C++'s KATANA_GIS_SAMPLES and KATANA_GEO_TEST_DATA are absolute and
//! normal (CMake writes them so); the checkout's path as cargo gives it holds
//! "../../..", and a record that keeps a path normalised (a project's
//! reference record) would never equal it. So the folders here are made
//! normal, lexically, as the C++'s are.
//!
//! One owner (A); a test target includes it with
//! `#[path = "../support/surface_support.rs"] pub mod surface_support;`.

#![allow(
    dead_code,
    reason = "Each test target that includes the helpers uses a part of them."
)]

use std::path::{Component, Path, PathBuf};

use katana_app::geo::replies::{Record, parse_records};
use katana_core::text::parse_finite_double;
use katana_core::{ErrorCode, Result};

/// `path` with its "." and ".." taken out by its names alone, as
/// std::filesystem's lexically_normal does.
pub fn normal(path: &Path) -> PathBuf {
    let mut kept = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !kept.pop() {
                    kept.push("..");
                }
            }
            other => kept.push(other.as_os_str()),
        }
    }
    kept
}

/// A folder of the checkout, normal, as the text a line names it by ('/'
/// between the names).
fn checkout_text(relative: &str) -> String {
    let path =
        normal(&PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../..")).join(relative));
    path.to_string_lossy().replace('\\', "/")
}

/// KATANA_GIS_SAMPLES: samples/gis.
pub fn samples() -> String {
    checkout_text("samples/gis")
}

/// KATANA_GEO_TEST_DATA: tests/geo/data.
pub fn data() -> String {
    checkout_text("tests/geo/data")
}

/// tests/geo/data/plane.asc: 40 x 30 cells of 1 m from (0,0), z = 100 +
/// 0.05 x at the cell centres.
pub fn plane() -> String {
    format!("{}/plane.asc", data())
}

/// `"<path>"`, as a line quotes a path.
pub fn quoted(path: &str) -> String {
    format!("\"{path}\"")
}

/// Every record of `kind` in a reply.
pub fn records_of(reply: &str, kind: &str) -> Vec<Record> {
    parse_records(reply)
        .into_iter()
        .filter(|record| record.kind == kind)
        .collect()
}

/// The first record of `kind`: ASSERT_TRUE(record) << reply.
#[track_caller]
pub fn record_of(reply: &str, kind: &str) -> Record {
    match parse_records(reply)
        .into_iter()
        .find(|record| record.kind == kind)
    {
        Some(record) => record,
        None => panic!("no {kind} record in:\n{reply}"),
    }
}

/// A field of a record, "<absent>" when it has none.
pub fn field(record: &Record, key: &str) -> String {
    String::from(record.get(key).unwrap_or("<absent>"))
}

/// A field of a record, "" when it has none: the C++'s value_or("").
pub fn get(record: &Record, key: &str) -> String {
    String::from(record.get(key).unwrap_or(""))
}

/// A field read as a number; NaN when it is none.
pub fn number_in(record: &Record, key: &str) -> f64 {
    parse_finite_double(record.get(key).unwrap_or("")).unwrap_or(f64::NAN)
}

/// The code of a refusal: the line must fail.
#[track_caller]
pub fn code<T>(result: Result<T>) -> ErrorCode {
    match result {
        Ok(_) => panic!("the line should have been refused"),
        Err(error) => error.code,
    }
}

/// EXPECT_DOUBLE_EQ: within 4 units in the last place.
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
