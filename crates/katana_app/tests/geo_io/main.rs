//! IMPORT, EXPORT, RASTER SAMPLE, DRAPE and RASTER ZONAL through the one
//! executor and the session (step 6, wave 2, agent B), one module per C++ test
//! file:
//!
//! ```text
//! import_options    tests/geo/test_import_options.cpp   ImportOptions, RasterBands
//! export_options    tests/geo/test_export_options.cpp   ExportOptions
//! gis_verbs         tests/geo/test_gis_verbs.cpp        GisVerbs, GisSession
//! session           tests/app/test_session.cpp          Session (3 of its 18)
//! vector_fidelity   tests/geo/test_vector_fidelity.cpp  VectorFidelity (1 of 13)
//! formats           tests/geo/test_formats.cpp          Formats (1 of 15)
//! verb_usage        tests/geo/test_verb_usage.cpp       GeoVerbUsage
//! drape_and_sample  tests/geo/test_drape_and_sample.cpp DrapeAndSample
//! zonal_verb        tests/geo/test_zonal_verb.cpp       ZonalVerb
//! ```
//!
//! The files' other suites are twinned elsewhere: GisRecords by the frame
//! (tests/geo_frame), RasterSampler and Drape by the lower crates, ZonalContract
//! by the bridge, the rest of Session by the session's targets, the rest of
//! VectorFidelity and Formats by interop's. Each gtest suite is a module
//! inside its file's module, named as tools/check_rust_parity.py expects.
//!
//! Some cases run verbs of step 6's other wave-2 agents through the executor
//! or the session this target drives (REFS and INFO, SURFACE, the raster
//! verbs): written in full, they pass when wave 2 is integrated as one.
//!
//! libtest runs these cases on parallel threads of one process, where ctest
//! ran each in its own: every scratch folder is named for its case and the
//! process, and the cases that run GDAL or PDAL on a file of tests/geo/data or
//! samples/gis take the target's one lock (geo_fixture's `shared_data`), so
//! GDAL's .aux.xml sidecars never race. No input or expected value is changed
//! for it. Nothing here sets an environment variable or the working
//! directory, so there is no process-state target.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]
#![allow(
    clippy::excessive_precision,
    reason = "Expected values are copied digit for digit from the C++ tests."
)]

#[path = "../support/app_support.rs"]
pub mod app_support;
#[path = "../support/geo_fixture.rs"]
pub mod geo_fixture;

mod drape_and_sample;
mod export_options;
mod formats;
mod gis_verbs;
mod import_options;
mod session;
mod vector_fidelity;
mod verb_usage;
mod zonal_verb;

/// EXPECT_DOUBLE_EQ: within 4 units in the last place (gtest's
/// AlmostEquals), NaN never equal.
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

/// A path quoted for a line.
pub fn quoted(path: &str) -> String {
    format!("\"{path}\"")
}
