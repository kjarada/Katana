//! The scene and surface display (wave G3), one module per C++ test file:
//!
//! ```text
//! surface_colouring     tests/cad/terrain/test_surface_colouring.cpp
//! surface_plan          tests/cad/terrain/test_surface_plan.cpp
//! scene_surface_style   tests/cad/terrain/test_scene_surface_style.cpp
//! section_cut           tests/cad/terrain/test_section_cut.cpp
//! scene                 tests/cad/test_scene.cpp
//! scene_simd            tests/cad/test_scene_simd.cpp
//! scene_zoom            tests/cad/test_scene_zoom.cpp
//! layer_inheritance     tests/cad/test_layer_inheritance.cpp
//! ```
//!
//! Each gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects.

#![allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    reason = "Preserve decimal constants and expected values copied verbatim from the C++ oracle."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/simd_levels.rs"]
pub mod simd_levels;

mod layer_inheritance;
mod scene;
mod scene_simd;
mod scene_surface_style;
mod scene_zoom;
mod section_cut;
mod surface_colouring;
mod surface_plan;

/// EXPECT_NEAR(actual, expected, epsilon).
#[track_caller]
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
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

/// A surface the test expects to build, with the error's description when it
/// does not.
#[track_caller]
pub fn tin(
    vertices: Vec<katana_geometry::Point3>,
    triangles: Vec<[u32; 3]>,
) -> katana_terrain::TinSurface {
    match katana_terrain::TinSurface::create(vertices, triangles, Vec::new()) {
        Ok(surface) => surface,
        Err(error) => panic!("{}", error.describe()),
    }
}

/// A step the test expects to succeed, with the error's description when it
/// does not.
#[track_caller]
pub fn ok(status: katana_core::Status) {
    if let Err(error) = status {
        panic!("{}", error.describe());
    }
}
