//! The raster analysis verbs (step 6, wave 2, agent R), one module per C++
//! test file:
//!
//! ```text
//! shade_verb        tests/geo/test_shade_verb.cpp         ShadeVerb, ShadeSession
//! slope_verbs       tests/geo/test_slope_verbs.cpp        SlopeVerbs, SlopeSession
//! viewshed_and_los  tests/geo/test_viewshed_and_los.cpp   Viewshed
//! grid_verb         tests/geo/test_grid_verb.cpp          GridVerb
//! dem_verbs         tests/geo/test_dem_verbs.cpp          DemVerbs
//! ```
//!
//! The files' other suites are twinned elsewhere: the contract suites
//! (ShadeContract, SlopeContract, ViewshedContract, RasterGridContract,
//! DemVerbsContract), ColourRamps and LineOfSight in step 5's and step 2's
//! targets, and GridSession and DemSession, which drive the MCP server, in
//! the MCP's (step 6, agent C). Each gtest suite is a module inside its
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

mod raster_support;

mod dem_verbs;
mod grid_verb;
mod shade_verb;
mod slope_verbs;
mod viewshed_and_los;
