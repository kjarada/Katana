//! Plan painting and the style previews (P, step 7 wave 1): the twins of tests/qt_widgets'
//! test_plan_painter (its PlanPainter suite; PlanView is VP's), test_plan_cloud,
//! drawing/test_feedback_painter, terrain/test_surface_plan_painter, annotation/test_annotation_ui
//! (its AnnotationPaint suite) and annotation/test_leader_preview - one module a C++ file, the
//! suites as modules inside.
//!
//! The painted twins draw through the raster backend `raster` names, the one line the integrator
//! points at C's raster canvas (raster.rs says how).

#![allow(
    clippy::float_cmp,
    reason = "Sizes and colours are compared as the C++ compared them, exactly where it did."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]
#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "The C++'s !(a > b) is kept where a test rejects NaN with it."
)]

#[path = "../support/window_support.rs"]
pub mod window_support;

mod raster;

mod annotation_ui;
mod feedback_painter;
mod leader_preview;
mod plan_cloud;
mod plan_painter;
mod surface_plan_painter;
