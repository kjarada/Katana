//! The geo suite's cases of the cad layer's own code (step 5, agent K2), one
//! module per C++ test file:
//!
//! ```text
//! surface_input      tests/geo/test_surface_input.cpp (SurfaceInput)
//! drape_and_sample   tests/geo/test_drape_and_sample.cpp (the Drape suite)
//! ```
//!
//! Twins of code step 4 ported (katana_cad::geo): the C++ tests sit in
//! tests/geo because the geo verbs are what build surfaces and drape, and
//! their twins sit here, beside the code they test (the step-5 plan,
//! section 8, choice 8). The rest of test_drape_and_sample.cpp drives DRAPE
//! and SAMPLE through the app layer (step 6), or GDAL's raster sampler
//! (katana_gis).

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

mod drape_and_sample;
mod surface_input;

use std::path::{Path, PathBuf};

use katana_cad::Document;
use katana_commands::create_entities;
use katana_entity::{Entity, EntityId};

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

/// The entities into a drawing, one step, and their ids in order.
#[track_caller]
pub fn draw(document: &mut Document, entities: Vec<Entity>) -> Vec<EntityId> {
    let status = document.execute(create_entities(entities));
    assert!(status.is_ok(), "{status:?}");
    document.last_created_entities()
}

/// A folder of a case's own under the temporary directory, removed with its
/// contents when dropped: the C++'s fixed names became the case's name and
/// the process id, since libtest runs the cases on threads of one process.
pub struct Scratch {
    pub path: PathBuf,
}

impl Scratch {
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "katana-cad-geo-inputs-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::create_dir_all(&path);
        Self { path }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// std::filesystem::copy(from, to, copy_options::recursive): every file of
/// `from`, its folders followed, copied to the same place under `to`.
pub fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
