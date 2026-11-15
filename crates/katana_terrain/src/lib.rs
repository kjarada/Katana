//! The `terrain` layer: TIN surfaces, constrained triangulation, contours, volumes, tiled terrain.
//!
//! The port of src/katana_terrain and include/katana/terrain (docs/rust.md).

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection and branch order."
)]

mod cdt_backend;
pub mod colour_ramps;
pub mod contours;
pub mod line_of_sight;
pub mod overlay;
mod point_merge;
pub mod section_volume;
mod summation;
pub mod super_surface;
pub mod surface_store;
pub mod tiled_terrain;
pub mod tin_builder;
pub mod tin_codec;
pub mod tin_edit;
mod tin_patch;
pub mod tin_surface;
mod tin_thin;
pub mod volume;

pub use colour_ramps::{
    COLOUR_RAMP_STOPS, ColourRampStops, NamedColourRamp, RampColour, colour_ramp_names,
    colour_ramps, find_colour_ramp,
};
pub use contours::{Contour, MAX_CONTOUR_LEVELS, contour_at, contours};
pub use line_of_sight::{
    DEFAULT_CURVATURE, EARTH_DIAMETER, GroundAt, SightLine, SightOptions, line_of_sight,
};
pub use overlay::{
    Envelope, OVERLAY_BLOCK_TRIANGLES, OverlayPiece, OverlayRegion, OverlaySurface,
    OverlaySurfaceOptions, OverlayVisitor, difference_surface, envelope_surface,
    overlay_block_count, visit_overlay,
};
pub use section_volume::{
    AlignmentVolumeOptions, AlignmentVolumes, CurvatureShift, MAX_VOLUME_SECTIONS, SectionAreas,
    SectionPanel, SegmentPiece, SegmentProfiler, SignedArea, SignedMoment, StationSection,
    section_areas, signed_trapezoid, signed_trapezoid_moments, volumes_along_alignment,
};
pub use super_surface::combine_surfaces;
pub use surface_store::{NamedSurface, SurfaceStore};
pub use tiled_terrain::{TileSample, TiledTerrain, TiledTerrainOptions};
pub use tin_builder::{
    Breakline, CrossingBreaklinePolicy, DuplicatePointPolicy, TinBuildOptions, TinBuildReport,
    TinBuildResult, TinInput, build_tin,
};
pub use tin_codec::{TIN_CODEC_VERSION, decode_tin, encode_tin, encoded_tin_bytes};
pub use tin_edit::{
    BreaklineInput, BreaklineVertex, HeightChange, HeightOperation, HeldPointRule, HoleCutStats,
    MOST_MAGNITUDE, SmoothOptions, TinEdgeRef, TinEditReport, TinEditResult,
};
pub use tin_surface::{
    LocatorGrid, NO_TRIANGLE, NO_VERTEX, SlopeAspect, SurfaceLocation, TinNeighbors, TinSurface,
    TinTriangle,
};
pub use volume::{
    DatumVolume, GridVolume, MAX_GRID_VOLUME_CELLS, StockpileBase, StockpileOptions,
    StockpileVolume, SurfaceComparison, compare_surfaces, compare_surfaces_inside, grid_volume,
    stockpile_volume, volume_to_datum, volume_to_datum_inside,
};

/// The exact orientation predicate the triangulation decides with: +1 when c
/// lies to the left of the directed line a -> b, -1 to its right, 0 when the
/// three are collinear. Public for the later layers' edits (overlay, volumes)
/// that must decide sides as the kernel does.
pub fn orientation(
    a: katana_geometry::Point2,
    b: katana_geometry::Point2,
    c: katana_geometry::Point2,
) -> i32 {
    cdt_backend::orientation(a, b, c)
}
