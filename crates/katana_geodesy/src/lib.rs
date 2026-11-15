//! The `geodesy` layer: coordinate reference systems and transformations through PROJ, grid and ground factors, geodesics.
//!
//! The port of src/katana_geodesy and include/katana/geodesy. PROJ (libproj,
//! with Karney's geodesic routines in it) is reached through hand-written
//! declarations of exactly the C functions the C++ calls (`ffi`), and only
//! `proj_internal` calls them, so no PROJ type reaches the public interface.
//! Each PROJ-backed object owns a PROJ context of its own, with the network
//! off, as the C++'s do.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection."
)]

pub mod coordinate;
pub mod coordinate_reference_system;
pub mod coordinate_transformer;
pub mod ellipsoid;
mod ffi;
pub mod geodesic;
pub mod grid_factors;
mod proj_internal;
pub mod similarity_transform2d;
pub mod units;

pub use coordinate::{
    BatchReport, Coordinate, GeographicCoordinate, ProjectedCoordinate, UNSPECIFIED_EPOCH,
};
pub use coordinate_reference_system::{
    AxisInfo, CoordinateReferenceSystem, CrsKind, GeographicExtent, ProjRuntimeInfo, WktVersion,
};
pub use coordinate_transformer::{
    CoordinateTransformer, GridFile, NamedCoordinate, OperationInfo, TransformerOptions,
};
pub use ellipsoid::Ellipsoid;
pub use geodesic::{GeodesicDirectResult, GeodesicInverseResult};
pub use grid_factors::{GridFactorCalculator, GridFactors, GridPosition};
pub use similarity_transform2d::{ControlPointPair, SimilarityFit2D, SimilarityTransform2D};
pub use units::{AngleUnit, LengthUnit, Unit};
