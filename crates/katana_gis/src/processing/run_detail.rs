//! Shared by the bridge's two modules, never public
//! (src/katana_io/geo/gdal_run_detail.hpp): what the policy table needs to
//! see of a live algorithm, and the pipeline's words.
//!
//! The header includes gdalalgorithm.h; here that is the Rust owners of
//! GDAL's algorithm objects, which bridge.rs and policy.rs both hold. They
//! live in the FFI modules, which say who frees what, and are named here for
//! the bridge:
//!
//! ```text
//! Algorithm   an algorithm GDAL made; released (GDALAlgorithmRelease) on drop
//!             (ffi/gdal_algorithm.rs)
//! Arg         one of its arguments, borrowed from it for as long as it lives;
//!             the handle GDAL's C API hands out is released on drop
//!             (ffi/gdal_algorithm.rs)
//! DatasetItem a value of a dataset list: a name, or an open dataset borrowed
//!             from its owner (ffi/gdal_algorithm.rs)
//! DatasetRef  a reference to a dataset, dropped (GDALReleaseDataset) with it:
//!             the C++'s DatasetRef, whose destructor calls Release()
//!             (ffi/dataset.rs)
//! ```
//!
//! Each method is one call: the C API where it calls what the C++ calls, the
//! shim (native/algorithm_shim.cpp) where it does not - ffi/algorithm.rs says
//! which and why. Neither owner is Send: an algorithm is made, run and
//! released on one thread, as the C++ does, and GDAL's per-thread error
//! handlers rely on it. An argument is looked up as the C++ looks it up, with
//! no suggestion raised for a name it lacks.
//!
//! The functions the header declares - classify, pipeline_words,
//! has_external_step, is_pipeline - are implemented in policy.rs, as the
//! C++'s are in policy.cpp, and named here for the bridge.

pub(crate) use super::policy::{classify, has_external_step, is_pipeline, pipeline_words};
pub(crate) use crate::ffi::dataset::DatasetRef;
pub(crate) use crate::ffi::gdal_algorithm::{Algorithm, Arg, DatasetItem};

/// One word of a pipeline as GDAL's pipeline argument takes it ("read x !
/// slope ! write y", nested steps in [ ]): `step` when it names a step - the
/// first word that is not an option after the text's start, a '!' or a '[' -
/// and `first` while no '!' or '[' has been passed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PipelineWord {
    pub(crate) text: String,
    pub(crate) step: bool,
    pub(crate) first: bool,
}
