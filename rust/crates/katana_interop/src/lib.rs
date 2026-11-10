//! The `interop` layer: what gis and pointcloud read, converted into the domain model.
//!
//! The port of src/katana_interop and include/katana/interop (docs/rust.md).
//! One module tree, declared here: a subfolder of the headers is an inline
//! block whose files are in `src/<subfolder>/`, and a module follows the
//! header's path (the step-5 plan, "The crates' layout").

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection and branch order."
)]

pub mod geo {
    pub mod colour_ramps;
    pub mod contour_entities;
    pub mod drawing_dataset;

    pub use colour_ramps::{ColourRamp, LegendEntry, RampStop};
    pub use contour_entities::{ContourBoundary, ContourEntityOptions, ContourPlan};
    pub use drawing_dataset::{
        DrawingDataset, DrawingDatasetOptions, DrawingDatasetStats, FeaturePiece,
        FeaturePieceCounts, FeaturePieceOptions, ResultMode, ResultOptions, ResultPlan,
    };
}
mod curve_chords;
pub mod online_catalogue;
pub mod online_discovery;
pub mod online_requests;

pub use online_catalogue::{
    LonLatBox, OnlineCatalogue, OnlineLayer, OnlineLayerKind, OnlineProvider, OnlineService,
    OnlineServiceType,
};
pub use online_discovery::{CogProbe, TextFetcher};
pub use online_requests::{
    CkanResource, ImageRequest, ItemsPage, QueryPage, StacChoice, StacItem, TileIndex, TileMatrix,
    TileMatrixSet,
};
