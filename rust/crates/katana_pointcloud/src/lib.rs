//! The `pointcloud` layer: point clouds through PDAL.
//!
//! The port of src/katana_io's point-cloud engine, its PROJ search path and
//! include/katana/pointcloud (docs/rust.md). PDAL has only a C++ interface, so
//! it is reached through native/pdal_shim.cpp, a thin shim of its primitives
//! behind a C ABI that build.rs compiles; every decision of the C++ engine is
//! Rust, in point_cloud_engine.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection and branch order."
)]

mod ffi;
pub mod point_cloud_engine;
#[doc(hidden)]
pub mod proj_search_paths;

pub use point_cloud_engine::{
    PointCloud, PointCloudBounds, PointCloudEngine, PointCloudHeader, PointCloudPoint,
    PointCloudReadOptions, pdal_version,
};
