//! The `geometry` layer: 2D primitives and algorithms, spirals, chording, alignments.
//!
//! Behaviour ported from the corresponding C++ geometry layer.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection and branch order."
)]

pub mod alignment;
pub mod chording;
mod curve_offset;
pub mod curves2d;
pub mod editing;
pub mod intersection;
pub mod mesh;
pub mod point_batch;
pub mod point_splat;
pub mod polygon;
pub mod polyline_vertices;
pub mod primitives2d;
pub mod primitives3d;
pub mod profile;
mod scalar;
mod simd;
pub mod spatial_index;
pub mod spiral2;

pub use alignment::{
    AlignmentElement, AlignmentElementKind, AlignmentPi, AlignmentShape, HorizontalAlignment,
    SolvedAlignment, StationOffset,
};
pub use curves2d::{
    CurvePolyline2, CurvePolylineNearest, CurveSegment, CurveVertex, Ellipse2, HeightedPoint,
    Spline2,
};
pub use editing::{ChamferResult, Curve2, FilletResult, TrimResult};
pub use intersection::{IntersectionKind, IntersectionResult};
pub use mesh::TriangleMesh;
pub use point_splat::{PixelProjection, SplatCloud, SplatStats, SplatTile, SplatView};
pub use polygon::{CollinearVertices, Orientation, PolylineClip, PolylineRuns, TriangleIndices};
pub use polyline_vertices::PolylineResult;
pub use primitives2d::*;
pub use primitives3d::{Point3, Segment3, Triangle3, Vec3};
pub use profile::{
    ProfileElement, ProfileElementKind, ProfileExtremum, ProfilePvi, SolvedProfile,
    VerticalAlignment,
};
pub use spatial_index::{SpatialEntry, SpatialId, SpatialIndex};
pub use spiral2::Spiral2;
