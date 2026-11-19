//! The `gis` layer: GDAL: rasters, vectors, reprojection, web access, the geoprocessing bridge.
//!
//! The port of src/katana_io (its GDAL half) and include/katana/gis. GDAL is
//! reached through hand-written declarations of exactly the C functions the
//! C++ calls (`ffi`, checked at build time by native/ffi_check.c), and no
//! GDAL type reaches the public interface.
//!
//! Every `unsafe` of the crate is in `ffi`: the declarations, and the safe
//! owners and views over them (docs/rust.md, "GDAL's unsafe confined to
//! katana_gis's FFI modules"). Every other module is covered by the
//! workspace's `unsafe_code = "deny"`.

mod ffi {
    // The C declarations, one file per header family (cpl.rs says how they
    // are checked), with the boundary's text helpers in cpl.rs.
    pub(crate) mod algorithm;
    pub(crate) mod cpl;
    pub(crate) mod curl;
    pub(crate) mod gdal;
    pub(crate) mod http;
    pub(crate) mod minixml;
    pub(crate) mod ogr;
    pub(crate) mod osr;

    // The safe owners and views the rest of the crate calls.
    pub(crate) mod dataset;
    pub(crate) mod error;
    pub(crate) mod gdal_algorithm;
    pub(crate) mod geometry;
    pub(crate) mod srs;
    pub(crate) mod transfer;
    pub(crate) mod xml;

    #[cfg(test)]
    mod tests;
}

mod cpl_error_collector;
mod formats_detail;
mod gdal_registry;
mod ogr_detail;

pub mod formats;
pub mod gdal_adapter;
pub mod raster_sampling;
pub mod reproject;
pub mod web_access;
pub mod zip_container;

pub mod processing {
    mod types;
    pub use types::{
        AlgorithmInfo, AlgorithmSpec, ArgSpec, ArgType, ArgValue, Bound, DatasetKind, DatasetPath,
        DatasetValue, Diagnostic, Feature, FeatureSet, FeatureTable, FieldDef, FieldType,
        FieldValue, OutputTo, Policy, RasterGrid, RunOutputs, RunRequest, Scalar, TailEffects,
        Versions,
    };

    mod bridge;
    mod policy;
    mod run_detail;
    pub use bridge::{
        Words, arguments_given, catalogue, dataset_kinds_text, describe, path_text, resolve, run,
        staged_dataset_count, suggest, to_string, validate, versions,
    };
    pub use policy::{check_tokens, reads_heights, step_policy, tail_effects};
}

pub use formats::{Content, DataKind, Format, FormatOption, FormatOptions, OptionList, SaveChoice};
pub use gdal_adapter::{
    GdalDataset, GeoPoint, GeometryKind, RasterExportOptions, RasterImage, RasterInfo,
    RasterSamples, VectorExportOptions, VectorFeature, VectorGeometry, VectorLayerInfo,
    VectorReadOptions, VectorReadReport, VectorWriteReport,
};
pub use raster_sampling::{RasterSampler, Resampling};
pub use reproject::{CrsBox, RasterProbe, WarpOptions, WarpResult, WarpSource};
pub use web_access::{
    HttpExchange, HttpFailure, HttpRequest, HttpResponse, TransferProgress, XmlElement,
};
pub use zip_container::{ZipContent, ZipMember};
