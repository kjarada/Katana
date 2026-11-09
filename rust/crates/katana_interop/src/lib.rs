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

    pub use colour_ramps::{ColourRamp, LegendEntry, RampStop};
}
pub mod online_catalogue;

pub use online_catalogue::{
    LonLatBox, OnlineCatalogue, OnlineLayer, OnlineLayerKind, OnlineProvider, OnlineService,
    OnlineServiceType,
};
