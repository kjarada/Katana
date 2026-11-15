//! The subsurface utility tools: buried services graded by AS 5488.1 quality
//! level, their schedules, delivery schemas, clearance and verification.
//!
//! The port of include/katana/survey/subsurface and src/katana_survey/subsurface_*.cpp,
//! one module per header. The C++ namespace `katana::survey::subsurface` is
//! flat, so every public item of the modules is re-exported here too.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, which count a NaN as failing."
)]

pub mod clearance;
pub mod delivery_schema;
pub mod quality_level;
mod table;
pub mod utility_csv;
pub mod utility_network;
pub mod utility_report;
pub mod verification;

pub use clearance::*;
pub use delivery_schema::*;
pub use quality_level::*;
pub use utility_csv::*;
pub use utility_network::*;
pub use utility_report::*;
pub use verification::*;
