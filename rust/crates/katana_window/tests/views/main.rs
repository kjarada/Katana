//! The 3D, elevation and section views and the GPU view (V3, step 7 wave 3): the twins of
//! tests/qt_widgets' test_render_view, test_render_view_gpu, test_section_view and
//! test_section_recut, and of tests/gpu/test_gpu_scene_view - one module a C++ file, the suites as
//! modules inside. Wave 4's stragglers added terrain/test_surface_legend_3d, whose 3D views the
//! workspace opens.
//!
//! The views are driven alone, as the C++ tests drive a widget with no window: their events are
//! their own methods at a pixel of the view, and what they draw is read from the framebuffer or
//! the picture. Every view is lent its ViewState and its scene for each call (support.rs), as the
//! workspace lends them. The cases that open their views through the workspace (view_workspace,
//! wave 3) are left to it; each file names its own.
//!
//! The GPU cases are device cases (the foundation's rule, docs/rust.md "Step 7"): their body runs
//! on the Hardware device and then the Software one, on devices made once and shared by the
//! binary (katana_gpu's tests' gpu_support), and skips a device that is not there.

#![allow(
    clippy::float_cmp,
    reason = "Coordinates and cameras are compared exactly where the C++ compared them exactly."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]
#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "The C++ tests' negated comparisons, which count a NaN as failing, are kept."
)]
#![allow(
    clippy::excessive_precision,
    reason = "Expected values are written as the C++ tests wrote them."
)]

#[allow(
    dead_code,
    reason = "The GPU layer's test support is shared; these twins use only its devices."
)]
#[path = "../../../katana_gpu/tests/support/gpu_support.rs"]
pub mod gpu_support;

mod support;

mod gpu_scene_view;
mod render_view;
mod render_view_gpu;
mod section_recut;
mod section_view;
mod surface_legend_3d;
