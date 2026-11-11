//! The reference layers, the SURFACE verb, REFS, INFO <file> and CONTOUR,
//! and the geo frame's cases that read the reference data (step 6, wave 2,
//! agent A), one module per C++ test file:
//!
//! ```text
//! surface_verbs  tests/geo/test_surface_verbs.cpp   SurfaceVerbs, SurfaceSession
//! refs_verbs     tests/geo/test_refs_verbs.cpp      RefsVerbs, RefsSession (2 of its 3)
//! info_verb      tests/geo/test_info_verb.cpp       InfoVerb
//! contour_verb   tests/geo/test_contour_verb.cpp    ContourVerb, ContourSession
//! gdal_verbs     tests/geo/test_gdal_verbs.cpp      GeoExecutor (5 of its 38), GeoSession
//! pipeline_verb  tests/geo/test_pipeline_verb.cpp   PipelineVerb (2 of its 4)
//! session        tests/app/test_session.cpp         Session (1 of its 18)
//! ```
//!
//! The cases that change the working directory are in tests/geo_surfaces_process.
//! The files' other suites and cases are other agents': ContourContract and
//! ContourEntities step 5's (katana_gis, katana_interop), the other
//! GeoExecutor and PipelineVerb cases G1's (tests/geo_frame), the other
//! Session cases S1's and B's. Each gtest suite is a module inside its file's
//! module, named as tools/check_rust_parity.py expects.
//!
//! libtest runs these cases on parallel threads of one process, where ctest
//! ran each in its own: every scratch folder is named for its case and the
//! process, and the cases that run GDAL on a file of tests/geo/data or
//! samples/gis take the target's one lock (geo_fixture's `shared_data`), so
//! GDAL's .aux.xml sidecars never race. No input or expected value is changed
//! for it.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/app_support.rs"]
pub mod app_support;
#[path = "../support/geo_fixture.rs"]
pub mod geo_fixture;
#[path = "../support/surface_support.rs"]
pub mod surface_support;

mod contour_verb;
mod gdal_verbs;
mod info_verb;
mod pipeline_verb;
mod refs_verbs;
mod session;
mod surface_verbs;
