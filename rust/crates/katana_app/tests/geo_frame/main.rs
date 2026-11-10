//! The geo executor's frame and its shared support (step 6, wave 1, agent
//! G1), one module per C++ test file:
//!
//! ```text
//! gdal_verbs           tests/geo/test_gdal_verbs.cpp           GeoExecutor (33 of its 38)
//! formats              tests/geo/test_formats.cpp              FormatsVerb
//! gis_verbs            tests/geo/test_gis_verbs.cpp            GisRecords
//! buffer_and_dissolve  tests/geo/test_buffer_and_dissolve.cpp  GeoLineWord
//! pipeline_verb        tests/geo/test_pipeline_verb.cpp        PipelineVerb (2 of its 4)
//! ```
//!
//! The files' other suites and cases are wave 2's: those that read the
//! reference data, keep a raster as one or bind a RASTER or SURFACE source
//! (A), and the verbs of B, V and R. Each gtest suite is a module inside its
//! file's module, named as tools/check_rust_parity.py expects.
//!
//! libtest runs these cases on parallel threads of one process, where ctest
//! ran each in its own: every scratch folder is named for its case and the
//! process, and the cases that run GDAL on a file of tests/geo/data take the
//! target's one lock (geo_fixture's `shared_data`), so GDAL's .aux.xml
//! sidecars never race. No input or expected value is changed for it.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/geo_fixture.rs"]
pub mod geo_fixture;

mod buffer_and_dissolve;
mod formats;
mod gdal_verbs;
mod gis_verbs;
mod pipeline_verb;
