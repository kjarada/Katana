//! The types of GDAL's algorithm framework behind plain data
//! (include/katana/gis/processing.hpp, its types and enums): what GDAL offers,
//! and the data handed to and taken from an algorithm. Nothing here names a
//! GDAL type. They come ahead of the bridge's functions because the adapter's
//! typed tables (`FeatureTable`, `FeatureSet`) are its representation of
//! features with their fields, for the algorithms and for files alike.
//!
//! The C++'s variants are enums here, each variant named after what it holds:
//! `Scalar` and `FieldValue` after `ArgType` and `FieldType` (`Text` for a
//! string, as katana_entity's `PropertyValue` has it; `Null` for
//! `std::monostate`). `DatasetKind` is an unscoped C++ enumeration of bits;
//! here an enum with the C++'s values, `ArgSpec::dataset_kinds` keeping the
//! bits (`DatasetKind::Raster as u32`).

use std::collections::BTreeMap;

use crate::gdal_adapter::{GeometryKind, RasterInfo, VectorGeometry};

// ---- what GDAL offers --------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ArgType {
    Boolean,
    #[default]
    String,
    Integer,
    Real,
    StringList,
    IntegerList,
    RealList,
    Dataset,
    DatasetList,
}

/// Bits of `ArgSpec::dataset_kinds`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum DatasetKind {
    Raster = 1,
    Vector = 2,
    Multidim = 4,
}

/// Safe runs without asking; Confirm changes or removes existing data in
/// place (vsi delete, dataset rename, raster edit, overview add ...) and runs
/// only when the line says CONFIRM (docs/geoprocessing.md, "Safety").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Policy {
    Safe,
    #[default]
    Confirm,
}

/// A bound of a numeric argument. GDAL's minimum for `contour --interval` is
/// exclusive (0 is refused, 0.001 taken); 19 of its 82 minimums are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bound {
    pub value: f64,
    pub inclusive: bool,
}

impl Default for Bound {
    fn default() -> Self {
        Self {
            value: 0.0,
            inclusive: true,
        }
    }
}

/// `std::variant<bool, std::string, int, double, std::vector<std::string>,
/// std::vector<int>, std::vector<double>>`.
#[derive(Clone, Debug, PartialEq)]
pub enum Scalar {
    Boolean(bool),
    Text(String),
    Integer(i32),
    Real(f64),
    TextList(Vec<String>),
    IntegerList(Vec<i32>),
    RealList(Vec<f64>),
}

impl Default for Scalar {
    /// A default-constructed variant holds its first alternative's default.
    fn default() -> Self {
        Self::Boolean(false)
    }
}

/// One argument of an algorithm, as GDAL declares it. Arguments GDAL hides
/// from its API (help, json-usage, config, progress ...) are not listed.
#[derive(Clone, Debug, PartialEq)]
pub struct ArgSpec {
    pub name: String,
    pub short_name: String,
    pub description: String,
    pub meta_var: String,
    /// Base|Advanced|Esoteric|Common|...
    pub category: String,
    /// Public only.
    pub aliases: Vec<String>,
    pub r#type: ArgType,
    pub required: bool,
    pub positional: bool,
    pub is_input: bool,
    pub is_output: bool,
    pub min_count: i32,
    /// -1 = unbounded.
    pub max_count: i32,
    pub packed_values: bool,
    pub repeatable: bool,
    pub in_pipeline_step: bool,
    pub default_value: Option<Scalar>,
    pub choices: Vec<String>,
    /// Absent when GDAL's bound is NaN, which is how it says "no bound":
    /// absent is not zero.
    pub min: Option<Bound>,
    pub max: Option<Bound>,
    pub dataset_kinds: u32,
    /// The dataset is opened for update: the algorithm changes it in place.
    pub dataset_update: bool,
    /// How a dataset argument may be given: by name, as an open dataset, or
    /// both (GDAL's GADV_NAME and GADV_OBJECT).
    pub accepts_name: bool,
    pub accepts_object: bool,
    pub exclusion_group: String,
    pub dependency_group: String,
    pub depends_on: Vec<String>,
    /// required_capabilities, ...
    pub metadata: BTreeMap<String, Vec<String>>,
}

impl Default for ArgSpec {
    fn default() -> Self {
        Self {
            name: String::new(),
            short_name: String::new(),
            description: String::new(),
            meta_var: String::new(),
            category: String::new(),
            aliases: Vec::new(),
            r#type: ArgType::String,
            required: false,
            positional: false,
            is_input: true,
            is_output: false,
            min_count: 0,
            max_count: 1,
            packed_values: false,
            repeatable: false,
            in_pipeline_step: true,
            default_value: None,
            choices: Vec::new(),
            min: None,
            max: None,
            dataset_kinds: 0,
            dataset_update: false,
            accepts_name: false,
            accepts_object: false,
            exclusion_group: String::new(),
            dependency_group: String::new(),
            depends_on: Vec::new(),
            metadata: BTreeMap::new(),
        }
    }
}

impl ArgSpec {
    pub fn is_dataset(&self) -> bool {
        self.r#type == ArgType::Dataset || self.r#type == ArgType::DatasetList
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AlgorithmInfo {
    /// {"raster", "hillshade"}, without the root "gdal".
    pub path: Vec<String>,
    /// Public only.
    pub aliases: Vec<String>,
    pub description: String,
    pub help_url: String,
    pub container: bool,
    pub policy: Policy,
    /// Why the policy is what it is: "listed" (the curated table), "update"
    /// (an input is opened for update) or "unclassified" (no one has judged
    /// it; Confirm until someone does). Containers are "container".
    pub policy_reason: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AlgorithmSpec {
    pub info: AlgorithmInfo,
    pub long_description: String,
    pub args: Vec<ArgSpec>,
    /// GDAL's --json-usage, verbatim.
    pub usage_json: String,
}

// ---- data handed to and taken from an algorithm ------------------------------------------

/// `std::variant<std::monostate, bool, std::int64_t, double, std::string>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum FieldValue {
    #[default]
    Null,
    Boolean(bool),
    Integer64(i64),
    Real(f64),
    Text(String),
}

/// Dates as ISO 8601 text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FieldType {
    Boolean,
    Integer64,
    Real,
    #[default]
    String,
    Date,
    DateTime,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FieldDef {
    pub name: String,
    pub r#type: FieldType,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Feature {
    /// The members of a multi-geometry; one for a simple geometry.
    pub parts: Vec<VectorGeometry>,
    /// Parallel to FeatureTable::fields.
    pub values: Vec<FieldValue>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FeatureTable {
    pub name: String,
    pub kind: GeometryKind,
    pub has_z: bool,
    pub crs_wkt: String,
    pub fields: Vec<FieldDef>,
    pub features: Vec<Feature>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FeatureSet {
    pub tables: Vec<FeatureTable>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RasterGrid {
    pub info: RasterInfo,
    /// GDAL's name: Byte, Int16, Float32, Float64 ...
    pub data_type: String,
    /// Row major, top row first.
    pub bands: Vec<Vec<f64>>,
    /// Per band.
    pub no_data: Vec<Option<f64>>,
}

impl Default for RasterGrid {
    fn default() -> Self {
        Self {
            info: RasterInfo::default(),
            data_type: String::from("Float64"),
            bands: Vec::new(),
            no_data: Vec::new(),
        }
    }
}

/// A dataset GDAL opens itself: a file, a /vsi path or a URL.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DatasetPath {
    pub path: String,
    pub open_options: Vec<String>,
    pub layer: String,
}

/// `std::variant<DatasetPath, RasterGrid, FeatureSet>`.
#[derive(Clone, Debug, PartialEq)]
pub enum DatasetValue {
    Path(DatasetPath),
    Raster(RasterGrid),
    Features(FeatureSet),
}

impl Default for DatasetValue {
    fn default() -> Self {
        Self::Path(DatasetPath::default())
    }
}

/// `std::variant<Scalar, DatasetValue, std::vector<DatasetValue>>`.
#[derive(Clone, Debug, PartialEq)]
pub enum ArgValue {
    Scalar(Scalar),
    Dataset(DatasetValue),
    Datasets(Vec<DatasetValue>),
}

impl Default for ArgValue {
    fn default() -> Self {
        Self::Scalar(Scalar::default())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OutputTo {
    #[default]
    Memory,
    File,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RunRequest {
    /// {"raster","hillshade"}, {"pipeline"}.
    pub path: Vec<String>,
    /// By long name, short name or alias, set before the tail is parsed.
    pub values: Vec<(String, ArgValue)>,
    /// GDAL's own command-line tail, parsed by GDAL after `values` are bound.
    /// An output the tail names ("hillshade dem.tif shade.tif") is written
    /// there, whatever `output_to` says.
    pub tokens: Vec<String>,
    pub output_to: OutputTo,
    pub output_path: String,
    /// File: an empty format is chosen from the extension.
    pub output_format: String,
    /// KEY=VALUE, File only.
    pub creation_options: Vec<String>,
    pub overwrite: bool,
    /// A raster output with more cells (width x height x bands) goes to a
    /// tiled GeoTIFF in spill_directory instead of into memory;
    /// RunOutputs::file names it. 0 spills every raster: how a derived
    /// reference raster is written without passing through a Vec.
    pub max_memory_cells: u64,
    pub spill_directory: String,
    /// Thread-local configuration options, allow-listed (GDAL_NUM_THREADS,
    /// GDAL_CACHEMAX). They do not reach threads GDAL starts itself, so
    /// nothing that must be right may depend on them.
    pub config: Vec<(String, String)>,
}

impl Default for RunRequest {
    fn default() -> Self {
        Self {
            path: Vec::new(),
            values: Vec::new(),
            tokens: Vec::new(),
            output_to: OutputTo::Memory,
            output_path: String::new(),
            output_format: String::new(),
            creation_options: Vec::new(),
            overwrite: false,
            max_memory_cells: 64 << 20,
            spill_directory: String::new(),
            config: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diagnostic {
    pub failure: bool,
    /// GDAL's CPLE_ number.
    pub code: i32,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunOutputs {
    pub raster: Option<RasterGrid>,
    pub features: Option<FeatureSet>,
    pub file: Option<String>,
    pub text: Option<String>,
    pub return_code: Option<i32>,
    /// GDAL's warnings; never a failure on success.
    pub diagnostics: Vec<Diagnostic>,
    pub seconds: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Versions {
    pub gdal: String,
    pub release: String,
    pub proj: String,
    pub geos: String,
    pub raster_drivers: i32,
    pub vector_drivers: i32,
    pub algorithms: i32,
}

/// What a run's own words change of data already there, beyond its
/// algorithm's policy (docs/geoprocessing.md, "Safety"). A pipeline is read
/// word by word however it was quoted - one quoted text, words one by one,
/// or --pipeline=... - nested [ ] steps included.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TailEffects {
    /// The step that makes the run Confirm (a pipeline's `update`, or a step
    /// no one has judged); empty when none does.
    pub confirm_step: String,
    /// The first word that lets GDAL replace or add to a dataset already
    /// there (--overwrite, --overwrite-layer, --append, --update, --upsert,
    /// --add, --resume): the run needs OVERWRITE. Empty when none.
    pub overwrite_word: String,
}
