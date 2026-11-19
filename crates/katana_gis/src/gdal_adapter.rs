//! GDAL raster and vector access, behind a Katana interface
//! (include/katana/gis/gdal_adapter.hpp, src/katana_io/gdal_adapter.cpp).
//!
//! Nothing public here names a GDAL type: geometry comes back as plain
//! coordinate arrays and rasters as 8-bit RGBA, so GDAL stays inside the
//! crate and the rest of the program cannot accidentally depend on it. This
//! layer is format-agnostic and domain-agnostic: converting to and from the
//! domain model is katana_interop's job.
//!
//! `GdalDataset`'s Rust shape: it owns an ffi::dataset::Dataset, whose `Drop`
//! closes it (`GDALClose`) - and never calls `GDALDestroyDriverManager`, which
//! would invalidate every other open dataset (gdal_registry.rs). It is `Send`
//! and not `Sync`: GDAL lets a dataset be used by one thread at a time and
//! handed between threads, never used by two at once. Its methods take
//! `&self`, as the C++'s are `const`; GDAL's own state behind the handle (a
//! layer's filters, its reading position) changes under them as it does in
//! the C++, which the missing `Sync` keeps to one thread. `open` returns the
//! dataset by value where the C++ returns a `unique_ptr`. Every GDAL call is a
//! safe method of the FFI modules' owners and views (ffi/dataset.rs), so this
//! module is safe code.
//!
//! The C++'s overloads have names of their own: `open` and
//! `open_with_options`, `read_image` and `read_image_band`, `read_features`
//! and `read_features_with_report`.

use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_int};
use std::marker::PhantomData;
use std::path::Path;

use katana_core::error::make_error;
use katana_core::path_text::path_exists;
use katana_core::text::{format_exact_real, trimmed};
use katana_core::{Error, ErrorCode, Result, Status};

use crate::cpl_error_collector::{CplErrorCollector, QuietHandler};
use crate::ffi::dataset::{
    self, Band, Dataset, DatasetHandle, Driver, Feature as OgrFeature, FieldDefn, FieldDefnRef,
    Layer,
};
use crate::ffi::error::{last_error_message, reset_last_error};
use crate::ffi::geometry;
use crate::ffi::{cpl, gdal, ogr, osr};
use crate::formats::{is_virtual_path, vector_writer_for};
use crate::formats_detail::{extension_of_file_name, file_name_of, open_names, remove_path};
use crate::gdal_registry::ensure_registered;
use crate::ogr_detail::{
    OgrGeometry, SpatialReference, flatten, geometry_type_name, make_ogr_geometry, parse_crs,
};
use crate::processing::{Feature, FeatureSet, FeatureTable, FieldDef, FieldType, FieldValue};
use crate::reproject::reproject_features;

/// GDAL's own default affine: the image at the origin, one world unit per
/// pixel, rows going down.
const DEFAULT_GEOTRANSFORM: [f64; 6] = [0.0, 1.0, 0.0, 0.0, 0.0, -1.0];

// ---- raster ---------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct RasterInfo {
    pub width: i32,
    pub height: i32,
    pub band_count: i32,
    pub projection_wkt: String,
    /// Affine pixel -> world mapping, GDAL order:
    /// world.x = gt[0] + px*gt[1] + py*gt[2];
    /// world.y = gt[3] + px*gt[4] + py*gt[5].
    pub geotransform: [f64; 6],
    /// False when the file carries no georeferencing, in which case the
    /// geotransform above is GDAL's default and places the image at the
    /// origin one world unit per pixel. Callers that need real coordinates
    /// must check.
    pub has_geotransform: bool,
    /// Band 1's no-data value, when the file declares one. Absent is not a
    /// value: a DEM without one has every pixel meaningful.
    pub no_data_value: Option<f64>,
}

impl Default for RasterInfo {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            band_count: 0,
            projection_wkt: String::new(),
            geotransform: DEFAULT_GEOTRANSFORM,
            has_geotransform: false,
            no_data_value: None,
        }
    }
}

/// One band at full precision on a regular sub-grid: sample (i, j) is SOURCE
/// pixel (i * stride, j * stride), exactly as the file holds it - no
/// resampling, so an elevation is a surveyed post and not an average of
/// neighbours, some of which may be no-data sentinels. Row major, top row
/// first.
#[derive(Clone, Debug, PartialEq)]
pub struct RasterSamples {
    pub columns: i32,
    pub rows: i32,
    pub stride: i32,
    /// columns * rows.
    pub values: Vec<f64>,
    /// The band's own no-data value, when it declares one. The caller decides
    /// what a no-data sample means; this layer only reports it.
    pub no_data_value: Option<f64>,
}

impl Default for RasterSamples {
    fn default() -> Self {
        Self {
            columns: 0,
            rows: 0,
            stride: 1,
            values: Vec::new(),
            no_data_value: None,
        }
    }
}

/// A decimated 8-bit RGBA copy of a raster, ready to hand to a painter.
/// Decimated on read by GDAL itself, so a 2 GB GeoTIFF never needs to be
/// fully resident.
#[derive(Clone, Debug, PartialEq)]
pub struct RasterImage {
    pub width: i32,
    pub height: i32,
    /// width * height * 4, row major, top row first.
    pub rgba: Vec<u8>,
    /// Geotransform of THIS image, already adjusted for the decimation, so it
    /// maps the decimated pixel grid to world coordinates directly.
    pub geotransform: [f64; 6],
    pub has_geotransform: bool,
    pub projection_wkt: String,
}

impl Default for RasterImage {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            rgba: Vec::new(),
            geotransform: DEFAULT_GEOTRANSFORM,
            has_geotransform: false,
            projection_wkt: String::new(),
        }
    }
}

// ---- vector ---------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GeoPoint {
    pub x: f64,
    pub y: f64,
    /// Meaningful only in a geometry whose has_z is true.
    pub z: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GeometryKind {
    #[default]
    Unknown,
    Point,
    LineString,
    /// parts[0] is the exterior ring; any further parts are holes.
    Polygon,
}

/// One geometry as coordinate arrays. A multi-geometry is delivered as
/// several VectorGeometry values (one per member) rather than a nested
/// structure, which keeps the consumer simple: every geometry has exactly one
/// kind.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorGeometry {
    pub kind: GeometryKind,
    pub parts: Vec<Vec<GeoPoint>>,
    /// Whether the geometry carries heights: read from the file's own
    /// geometry, and on writing, whether a 3D geometry is written. Without it
    /// a 2D source read as z = 0 everywhere, indistinguishable from real
    /// heights at the datum (audit IO-01). Absent is not zero.
    pub has_z: bool,
    /// The circular arcs in a curved line or ring (a file's CircularString,
    /// CompoundCurve or CurvePolygon), one list per part: each index i says
    /// that parts[p][i], [i + 1] and [i + 2] are an arc's start, a point on
    /// it and its end; the part is straight everywhere else. Empty for
    /// straight geometry, which is all a reader gives unless it is asked to
    /// keep arcs (VectorReadOptions::keep_arcs). The middle point is NOT a
    /// vertex: whoever reads a geometry with arcs draws them or makes them
    /// chords, never a polyline through it, which would cut every curve.
    pub arcs: Vec<Vec<usize>>,
}

impl VectorGeometry {
    pub fn has_arcs(&self) -> bool {
        self.arcs.iter().any(|part| !part.is_empty())
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorFeature {
    pub geometry: VectorGeometry,
    /// Ordered so that exported field order is deterministic (Rule 7).
    pub attributes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VectorLayerInfo {
    pub name: String,
    pub feature_count: u64,
    pub projection_wkt: String,
    /// GDAL's spelling, e.g. "Polygon", for display.
    pub geometry_type: String,
}

/// How a layer's features are read by GdalDataset::read_table.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorReadOptions {
    /// 0 reads them all; otherwise at most this many of the file's features.
    pub max_features: u64,
    /// A curve is kept as its arcs (VectorGeometry::arcs), for whoever turns
    /// them into Katana's arcs and circles and makes the rest chords by
    /// Katana's one rule - IMPORT. Otherwise every curve is chords at GDAL's
    /// own step (OGR_ARC_STEPSIZE, 4 degrees), counted in the report.
    pub keep_arcs: bool,
    /// OGR's attribute filter (OGRLayer::SetAttributeFilter): the WHERE
    /// clause of an OGR SQL statement, "kind = 'lot'", read by the driver.
    /// Empty reads every feature. IMPORT's where= (docs/interop.md, "Import
    /// options").
    pub attribute_filter: String,
    /// Only the features whose geometry meets this box - min x, min y, max x,
    /// max y in the layer's own coordinates - by
    /// OGRLayer::SetSpatialFilterRect, so a driver with a spatial index reads
    /// only those. GDAL's test is the envelope's. IMPORT's scope.
    pub spatial_filter: Option<[f64; 4]>,
}

/// What a read could not carry, said rather than dropped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VectorReadReport {
    /// Features of the file read, with or without geometry.
    pub features_read: u64,
    /// Geometry left out, by what it was: "tin", "polyhedral surface",
    /// "unsupported <GDAL's type name>", "nested too deep". A feature without
    /// geometry is kept, with its fields, and counted as "no geometry".
    pub skipped: BTreeMap<String, u64>,
    pub curves_made_chords: u64,
    /// GDAL's own warnings, raised while this read ran on this thread.
    pub warnings: Vec<String>,
}

/// What a write did beside writing what it was given.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VectorWriteReport {
    pub features_written: u64,
    /// Features with no geometry the format can hold (a line of one point).
    pub features_skipped: u64,
    /// Fields a format with a fixed set of fields (DXF) has nowhere to put.
    pub fields_not_written: Vec<String>,
    /// GDAL's own warnings (a shapefile shortening a field's name), and what
    /// Katana changed on the way (coordinates converted to longitude and
    /// latitude for a format that holds nothing else).
    pub warnings: Vec<String>,
}

// ---- export options -------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct RasterExportOptions {
    pub width: i32,
    pub height: i32,
    /// Any driver GDAL can write, including those that can only COPY a
    /// finished dataset rather than create one (AAIGrid): write_raster builds
    /// those in memory first and copies, so the caller need not know which
    /// kind a format is.
    pub driver: String,
    pub projection_wkt: String,
    pub geotransform: [f64; 6],
    pub no_data_value: Option<f64>,
}

impl Default for RasterExportOptions {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            driver: String::from("GTiff"),
            projection_wkt: String::new(),
            geotransform: DEFAULT_GEOTRANSFORM,
            no_data_value: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorExportOptions {
    /// Empty means "the writer for the path's name" (vector_driver_for_path):
    /// .shp, .geojson, .gpkg, .kml and .kmz (LIBKML), .fgb, .parquet, .gpx,
    /// .shp.zip ... An explicit name overrides it.
    pub driver: String,
    pub layer_name: String,
    /// The coordinate system of a table that names none. A format that holds
    /// only longitude and latitude on WGS 84 (KML, KMZ, GPX) is written in
    /// them, converted from this; given none, such a write is refused with
    /// InvalidCRS rather than written without its geometry.
    pub projection_wkt: String,
    /// KEY=VALUE, the driver's own options, over the ones Katana chooses for
    /// a format (CSV's geometry column, a MapInfo table's bounds): a key given
    /// here wins.
    pub creation_options: Vec<String>,
    pub layer_creation_options: Vec<String>,
    /// Add the tables as new layers of the file at `path` when there is one
    /// (a GeoPackage of several layers, written one EXPORT at a time) rather
    /// than replacing it; a file that is not there is created. EXPORT's
    /// append (docs/interop.md, "Export options").
    pub append: bool,
}

impl Default for VectorExportOptions {
    fn default() -> Self {
        Self {
            driver: String::new(),
            layer_name: String::from("katana"),
            projection_wkt: String::new(),
            creation_options: Vec::new(),
            layer_creation_options: Vec::new(),
            append: false,
        }
    }
}

// ---- helpers ----------------------------------------------------------------------------------

/// GDAL's last error, for the context field of an Error. Empty when GDAL did
/// not set one, in which case the caller's own message has to carry the
/// meaning.
fn last_gdal_error() -> String {
    last_error_message()
}

fn lower_extension(path: &Path) -> String {
    let bytes = cpl::native_path(path);
    let extension = extension_of_file_name(file_name_of(&bytes));
    let extension = extension.strip_prefix(b".").unwrap_or(extension);
    // ASCII only, whatever locale the GUI toolkit has set (core/text.hpp).
    String::from_utf8_lossy(extension).to_ascii_lowercase()
}

/// The raster formats a DEM is written in: the three every GIS and every
/// survey package reads. AAIGrid can only COPY a finished dataset, which is
/// what write_raster's in-memory build is for.
const RASTER_DRIVERS: [(&str, &str); 4] = [
    ("tif", "GTiff"),
    ("tiff", "GTiff"),
    ("asc", "AAIGrid"),
    ("img", "HFA"),
];

/// A failed write must not leave a file behind that looks like a finished
/// one: the driver is asked to delete it (it knows a format's sidecars), and
/// when it cannot - its generic Delete OPENS the file to list them, which a
/// truncated file may not survive - the file itself is removed. Quietly,
/// because this runs only on a path that is already reporting a failure,
/// whose message matters more than GDAL's complaint about the clean-up.
fn discard_output(driver: Driver, path: &Path) {
    let name = cpl::c_string(cpl::native_path(path));
    let quiet = QuietHandler::push();
    let deleted = driver.delete_dataset(&name);
    drop(quiet);
    if deleted != cpl::CE_NONE {
        remove_path(path);
    }
}

/// A GDAL configuration option set for this thread only, until the guard
/// goes, and then put back as it was. Thread-local so that an export does not
/// change how another thread's GDAL call behaves, and restored so that it
/// does not change how the next one on this thread does.
struct ScopedThreadConfig {
    key: &'static CStr,
    previous: Option<CString>,
    // Not Send: the option is this thread's.
    _thread: PhantomData<*const ()>,
}

impl ScopedThreadConfig {
    fn new(key: &'static CStr, value: &CStr) -> Self {
        let previous = cpl::thread_local_config_option(key);
        cpl::set_thread_local_config_option(key, Some(value));
        Self {
            key,
            previous,
            _thread: PhantomData,
        }
    }
}

impl Drop for ScopedThreadConfig {
    fn drop(&mut self) {
        // On the thread that set the option (the guard is not Send).
        cpl::set_thread_local_config_option(self.key, self.previous.as_deref());
    }
}

// ---- typed fields ---------------------------------------------------------------------------

/// The field's type as Katana reads it.
fn field_type_of(field: FieldDefnRef<'_>) -> FieldType {
    let (field_type, sub_type) = (field.field_type(), field.sub_type());
    match field_type {
        ogr::OFT_INTEGER => {
            if sub_type == ogr::OFST_BOOLEAN {
                FieldType::Boolean
            } else {
                FieldType::Integer64
            }
        }
        ogr::OFT_INTEGER64 => FieldType::Integer64,
        ogr::OFT_REAL => FieldType::Real,
        ogr::OFT_DATE => FieldType::Date,
        ogr::OFT_DATE_TIME => FieldType::DateTime,
        // Text, a time of day, lists and binary: read as GDAL writes them out.
        _ => FieldType::String,
    }
}

fn field_as_string(feature: &OgrFeature<'_>, index: c_int) -> String {
    feature.field_as_string(index)
}

/// An ISO 8601 date or date-time, as FieldType::Date and DateTime carry them.
fn iso_date(feature: &OgrFeature<'_>, index: c_int, with_time: bool) -> String {
    let Some(dataset::FieldDateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
    }) = feature.field_as_date_time(index)
    else {
        return field_as_string(feature, index);
    };
    // Whole numbers only: printf's %f would follow the C locale's decimal point.
    let two = |value: c_int| format!("{}{value}", if value < 10 { "0" } else { "" });
    let mut text = format!("{year}-{}-{}", two(month), two(day));
    if with_time {
        // A float's truncation and rounding, as the C++'s casts and lround of
        // a float (second is 0 to 61).
        let whole = second as c_int;
        let millis = ((second - whole as f32) * 1000.0_f32).round() as c_int;
        text.push_str(&format!("T{}:{}:{}", two(hour), two(minute), two(whole)));
        if millis > 0 {
            let pad = if millis < 100 {
                if millis < 10 { "00" } else { "0" }
            } else {
                ""
            };
            text.push_str(&format!(".{pad}{millis}"));
        }
    }
    text
}

fn field_value(feature: &OgrFeature<'_>, index: c_int, field_type: FieldType) -> FieldValue {
    if !feature.is_field_set_and_not_null(index) {
        return FieldValue::Null;
    }
    match field_type {
        FieldType::Boolean => FieldValue::Boolean(feature.field_as_integer(index) != 0),
        FieldType::Integer64 => FieldValue::Integer64(feature.field_as_integer64(index)),
        FieldType::Real => FieldValue::Real(feature.field_as_double(index)),
        FieldType::Date => FieldValue::Text(iso_date(feature, index, false)),
        FieldType::DateTime => FieldValue::Text(iso_date(feature, index, true)),
        FieldType::String => FieldValue::Text(field_as_string(feature, index)),
    }
}

/// A value as text, for read_features: what GDAL's GetFieldAsString gives for
/// the same field, a real at the digits that read back as the same double.
fn text_of(value: &FieldValue) -> String {
    match value {
        FieldValue::Null => String::new(),
        FieldValue::Boolean(held) => String::from(if *held { "1" } else { "0" }),
        FieldValue::Integer64(held) => format!("{held}"),
        FieldValue::Real(held) => format_exact_real(*held),
        FieldValue::Text(held) => held.clone(),
    }
}

fn kind_of_layer(geometry_type: ogr::OgrWkbGeometryType) -> GeometryKind {
    match geometry::flattened(geometry_type) {
        ogr::WKB_POINT | ogr::WKB_MULTI_POINT => GeometryKind::Point,
        ogr::WKB_LINE_STRING
        | ogr::WKB_MULTI_LINE_STRING
        | ogr::WKB_CIRCULAR_STRING
        | ogr::WKB_COMPOUND_CURVE
        | ogr::WKB_MULTI_CURVE => GeometryKind::LineString,
        ogr::WKB_POLYGON
        | ogr::WKB_MULTI_POLYGON
        | ogr::WKB_CURVE_POLYGON
        | ogr::WKB_MULTI_SURFACE
        | ogr::WKB_TRIANGLE => GeometryKind::Polygon,
        _ => GeometryKind::Unknown,
    }
}

fn wkt_of_layer(layer: Layer<'_>) -> String {
    layer
        .spatial_ref()
        .and_then(|reference| reference.exported_wkt())
        .unwrap_or_default()
}

/// KML's altitude is a height only in its "absolute" mode. Its default,
/// clampToGround, puts every coordinate on the ground whatever it says, and
/// GDAL's KMZ writer gives a 2D coordinate an altitude of 0 - read as a
/// height, that would be every feature surveyed at the datum.
fn height_is_altitude(feature: &OgrFeature<'_>, altitude_field: c_int) -> bool {
    if altitude_field < 0 || !feature.is_field_set_and_not_null(altitude_field) {
        return false;
    }
    field_as_string(feature, altitude_field).eq_ignore_ascii_case("absolute")
}

fn drop_heights(part: &mut VectorGeometry) {
    part.has_z = false;
    for ring in &mut part.parts {
        for point in ring {
            point.z = 0.0;
        }
    }
}

/// A read's attribute and spatial filters set on `layer` (VectorReadOptions).
/// InvalidArgument, with GDAL's reason, for an attribute filter the driver
/// cannot parse - said, never read as "no features match".
fn set_filters(layer: Layer<'_>, options: &VectorReadOptions) -> Status {
    if let Some(rect) = options.spatial_filter {
        layer.set_spatial_filter_rect(rect[0], rect[1], rect[2], rect[3]);
    }
    if !options.attribute_filter.is_empty() {
        let filter = cpl::c_string(&options.attribute_filter);
        reset_last_error();
        let status = layer.set_attribute_filter(Some(filter.as_c_str()));
        if status != ogr::OGRERR_NONE {
            let why = last_gdal_error();
            layer.clear_spatial_filter();
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "GDAL could not read the filter{}",
                    if why.is_empty() {
                        String::new()
                    } else {
                        format!(": {why}")
                    }
                ),
                options.attribute_filter.clone(),
            ));
        }
    }
    Ok(())
}

fn clear_filters(layer: Layer<'_>) {
    layer.clear_spatial_filter();
    layer.set_attribute_filter(None);
}

/// Every feature of `layer` as a typed table (GdalDataset::read_table).
fn read_layer(
    layer: Layer<'_>,
    driver: &str,
    options: &VectorReadOptions,
    report: &mut VectorReadReport,
) -> FeatureTable {
    let (name, geometry_type, definition) =
        (layer.name(), layer.geometry_type(), layer.definition());
    let mut table = FeatureTable {
        name,
        kind: kind_of_layer(geometry_type),
        has_z: geometry::has_z(geometry_type),
        crs_wkt: wkt_of_layer(layer),
        ..FeatureTable::default()
    };
    let field_count = definition.map_or(0, |definition| definition.field_count());
    for f in 0..field_count {
        let field = definition.and_then(|definition| definition.field(f));
        table.fields.push(FieldDef {
            name: field.map(|field| field.name()).unwrap_or_default(),
            r#type: field.map_or(FieldType::String, field_type_of),
        });
    }
    let kml = driver == "KML" || driver == "LIBKML";
    let altitude_field = match definition {
        Some(definition) if kml => definition.field_index(c"altitudeMode"),
        _ => -1,
    };

    layer.reset_reading();
    let mut read: u64 = 0;
    // The feature GetNextFeature hands over is destroyed when each pass of
    // the loop drops it, after the last use of its geometry's view.
    while let Some(ogr_feature) = layer.next_feature() {
        read += 1;
        report.features_read += 1;
        let mut feature = Feature::default();
        let geometry = ogr_feature.geometry();
        flatten(geometry, &mut feature.parts, options.keep_arcs, report, 0);
        if geometry.is_none_or(|geometry| geometry.is_empty()) {
            *report
                .skipped
                .entry(String::from("no geometry"))
                .or_insert(0) += 1;
        }
        if kml && !height_is_altitude(&ogr_feature, altitude_field) {
            for part in &mut feature.parts {
                drop_heights(part);
            }
        }
        for part in &feature.parts {
            table.has_z = table.has_z || part.has_z;
        }
        feature.values.reserve(table.fields.len());
        for (f, field) in (0..field_count).zip(&table.fields) {
            feature
                .values
                .push(field_value(&ogr_feature, f, field.r#type));
        }
        table.features.push(feature);
        drop(ogr_feature);
        if options.max_features != 0 && read >= options.max_features {
            break;
        }
    }
    table
}

// ---- raster image helpers -------------------------------------------------------------------

/// What each band of a raster is shown as; None for none.
#[derive(Default)]
struct BandRoles<'a> {
    red: Option<Band<'a>>,
    green: Option<Band<'a>>,
    blue: Option<Band<'a>>,
    alpha: Option<Band<'a>>,
    palette: Option<Band<'a>>,
    grey: Option<Band<'a>>,
}

fn classify_bands(dataset: &DatasetHandle) -> BandRoles<'_> {
    let mut roles = BandRoles::default();
    let count = dataset.raster_count();
    for i in 1..=count {
        let Some(band) = dataset.band(i) else {
            continue;
        };
        match band.color_interpretation() {
            gdal::GCI_RED_BAND => roles.red = Some(band),
            gdal::GCI_GREEN_BAND => roles.green = Some(band),
            gdal::GCI_BLUE_BAND => roles.blue = Some(band),
            gdal::GCI_ALPHA_BAND => roles.alpha = Some(band),
            gdal::GCI_PALETTE_INDEX => roles.palette = Some(band),
            gdal::GCI_GRAY_INDEX => roles.grey = Some(band),
            _ => {}
        }
    }
    // Plenty of GeoTIFFs carry three or four undesignated bands; treating the
    // first three as RGB is what every other GIS does and is far better than
    // rendering an aerial photo as a grey ramp of its red channel.
    if roles.red.is_none() && roles.palette.is_none() && count >= 3 {
        roles.red = dataset.band(1);
        roles.green = dataset.band(2);
        roles.blue = dataset.band(3);
    }
    if roles.red.is_none() && roles.palette.is_none() && roles.grey.is_none() && count >= 1 {
        roles.grey = dataset.band(1);
    }
    roles
}

/// The decimated read of one band: what a read of the whole band into a
/// buffer of the output's size gives.
struct Decimation {
    width: c_int,
    height: c_int,
    out_width: c_int,
    out_height: c_int,
    pixels: usize,
}

impl Decimation {
    fn read_bytes(&self, band: Band<'_>) -> Option<Vec<u8>> {
        let mut out = vec![0u8; self.pixels];
        let status = band.read_u8(
            0,
            0,
            self.width,
            self.height,
            &mut out,
            self.out_width,
            self.out_height,
        );
        (status == cpl::CE_NONE).then_some(out)
    }
}

// ---- the dataset ----------------------------------------------------------------------------

#[derive(Debug)]
pub struct GdalDataset {
    /// Closed when the value goes; Send and not Sync (ffi/dataset.rs).
    dataset: Dataset,
}

impl GdalDataset {
    fn handle(&self) -> &DatasetHandle {
        &self.dataset
    }

    /// Opens a raster or vector dataset: a file, a folder GDAL reads (a file
    /// geodatabase), a /vsi path, a URL, or a driver's connection string
    /// (formats.rs, is_virtual_path). An archive GDAL does not open as it is
    /// (a .zip of a shapefile, a .tar.gz of a GeoTIFF) is opened by its
    /// inside: as a folder, or its one dataset. NotFound for a local path that
    /// does not exist; FileImportFailure, with GDAL's own message as context,
    /// when GDAL cannot open it; InvalidArgument, naming them, for an archive
    /// of several datasets.
    pub fn open(path: &Path) -> Result<GdalDataset> {
        Self::open_with_options(path, &[])
    }

    /// The same with the driver's open options, KEY=VALUE.
    pub fn open_with_options(path: &Path, open_options: &[String]) -> Result<GdalDataset> {
        ensure_registered();

        // A local file that is not there is said so plainly. A /vsi path, a
        // URL or a connection string is no file on this disk, and whether it
        // exists is GDAL's to find out.
        if !is_virtual_path(path) && !path_exists(path).unwrap_or(false) {
            return Err(make_error(
                ErrorCode::NotFound,
                "file does not exist",
                cpl::path_text(path),
            ));
        }

        let mut options = cpl::CStringList::new();
        for option in open_options {
            options.add(option);
        }
        // The path as it is, then - for an archive GDAL does not open as it
        // is, a .zip of a shapefile - its inside (formats_detail.rs). The
        // first failure's message is the one worth reporting.
        let names = open_names(path);
        let mut opened: Option<Dataset> = None;
        let mut failure = String::new();
        for name in &names.names {
            let name = cpl::c_string(name);
            reset_last_error();
            opened = Dataset::open(
                &name,
                gdal::GDAL_OF_READONLY | gdal::GDAL_OF_RASTER | gdal::GDAL_OF_VECTOR,
                &options,
            );
            if opened.is_some() {
                break;
            }
            if failure.is_empty() {
                failure = last_gdal_error();
            }
        }
        let Some(dataset) = opened else {
            if !names.ambiguity.is_empty() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    names.ambiguity,
                    cpl::path_text(path),
                ));
            }
            return Err(make_error(
                ErrorCode::FileImportFailure,
                format!("GDAL could not open '{}'", cpl::path_text(path)),
                failure,
            ));
        };
        Ok(GdalDataset { dataset })
    }

    fn raster_count(&self) -> c_int {
        self.dataset.raster_count()
    }

    fn layer_count(&self) -> c_int {
        self.dataset.layer_count()
    }

    pub fn has_raster(&self) -> bool {
        self.raster_count() > 0
    }

    pub fn has_vector(&self) -> bool {
        self.layer_count() > 0
    }

    pub fn raster_info(&self) -> Result<RasterInfo> {
        let dataset = self.handle();
        if self.raster_count() == 0 {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "dataset holds no raster bands",
            ));
        }

        let mut info = RasterInfo::default();
        let mut geotransform = [0.0_f64; 6];
        info.width = dataset.raster_x_size();
        info.height = dataset.raster_y_size();
        info.band_count = dataset.raster_count();
        if let Some(projection) = dataset.projection_ref() {
            info.projection_wkt = projection;
        }
        if dataset.geo_transform(&mut geotransform) == cpl::CE_NONE {
            info.geotransform = geotransform;
            info.has_geotransform = true;
        }
        info.no_data_value = dataset.band(1).and_then(Band::no_data_value);
        Ok(info)
    }

    /// The short name of the driver that opened the dataset: "GTiff",
    /// "ESRI Shapefile", "GPKG". For display and for bug reports.
    pub fn driver_name(&self) -> String {
        // GDAL keeps a driver's short name in its description.
        self.dataset
            .driver()
            .map(Driver::description)
            .unwrap_or_default()
    }

    fn check_band(&self, band_index: c_int) -> Status {
        let count = self.raster_count();
        if band_index < 1 || band_index > count {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "raster band index is out of range",
                format!("requested {band_index} of {count}"),
            ));
        }
        Ok(())
    }

    /// Band values as doubles, for analysis (elevation, for instance). The
    /// whole band is materialised, so prefer read_image() for display.
    pub fn read_band(&self, band_index: i32) -> Result<Vec<f64>> {
        self.check_band(band_index)?;
        let dataset = self.handle();
        let (width, height) = (dataset.raster_x_size(), dataset.raster_y_size());
        let mut values = vec![0.0_f64; width as usize * height as usize];
        let status = dataset.band(band_index).map_or(cpl::CE_FAILURE, |band| {
            band.read_f64(0, 0, width, height, &mut values, width, height)
        });
        if status != cpl::CE_NONE {
            return Err(make_error(
                ErrorCode::FileImportFailure,
                "GDAL failed to read raster band",
                last_gdal_error(),
            ));
        }
        Ok(values)
    }

    /// Band values on every `stride`-th pixel of every `stride`-th row, read
    /// one source row at a time so a 2 GB DEM never needs to be resident: the
    /// memory is one row plus the samples kept. InvalidArgument for a band out
    /// of range or a stride below 1.
    pub fn read_band_sampled(&self, band_index: i32, stride: i32) -> Result<RasterSamples> {
        self.check_band(band_index)?;
        if stride < 1 {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the sampling stride must be at least 1",
                format!("stride {stride}"),
            ));
        }

        let dataset = self.handle();
        let (width, height) = (dataset.raster_x_size(), dataset.raster_y_size());
        // In 64 bits: width + stride - 1 overflows an int for a stride near
        // i32::MAX, which a one-sample cap can legitimately ask for.
        let samples_along = |extent: c_int| {
            ((i64::from(extent) + i64::from(stride) - 1) / i64::from(stride)) as c_int
        };

        let mut samples = RasterSamples {
            stride,
            columns: samples_along(width),
            rows: samples_along(height),
            ..RasterSamples::default()
        };
        let band = dataset.band(band_index);
        if let Some(no_data) = band.and_then(Band::no_data_value) {
            samples.no_data_value = Some(no_data);
        }

        // One whole source row per kept row, and the kept pixels picked out
        // of it. Asking RasterIO for a smaller buffer instead would make GDAL
        // RESAMPLE - nearest neighbour at pixel (i + 0.5) * step, not
        // i * step - which moves every sample off the pixel the caller will
        // georeference it to.
        samples
            .values
            .reserve(samples.columns as usize * samples.rows as usize);
        let mut row = vec![0.0_f64; width as usize];
        let step = stride as usize;
        for j in 0..samples.rows {
            // (rows - 1) * stride < height, so this stays within an int.
            let source_row = j * stride;
            let status = band.map_or(cpl::CE_FAILURE, |band| {
                band.read_f64(0, source_row, width, 1, &mut row, width, 1)
            });
            if status != cpl::CE_NONE {
                return Err(make_error(
                    ErrorCode::FileImportFailure,
                    "GDAL failed to read raster band",
                    format!("row {source_row}: {}", last_gdal_error()),
                ));
            }
            samples.values.extend(
                row.iter()
                    .step_by(step)
                    .take(samples.columns as usize)
                    .copied(),
            );
        }
        Ok(samples)
    }

    /// A display copy, decimated so neither dimension exceeds max_pixels.
    /// Uses the band colour interpretation: RGB(A) where present, a grey ramp
    /// otherwise, and the palette for paletted rasters. Pixels equal to the
    /// band's no-data value become transparent.
    pub fn read_image(&self, max_pixels: i32) -> Result<RasterImage> {
        self.read_image_band(max_pixels, 0)
    }

    /// The same with `band` (1-based) alone, shown as grey stretched over its
    /// own range - or through its colour table when it has one - whatever
    /// the file says its bands are: IMPORT's band=. 0 is read_image.
    /// InvalidArgument naming the count for a band the file does not have.
    pub fn read_image_band(&self, max_pixels: i32, only_band: i32) -> Result<RasterImage> {
        let count = self.raster_count();
        if only_band < 0 || only_band > count {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("the raster has {count} bands"),
                format!("band {only_band}"),
            ));
        }
        if max_pixels < 1 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "maxPixels must be at least 1",
            ));
        }

        let info = self.raster_info()?;
        let dataset = self.handle();

        let width = info.width;
        let height = info.height;
        if width <= 0 || height <= 0 {
            return Err(Error::new(ErrorCode::InvalidState, "raster has no extent"));
        }

        // Decimate on READ: GDAL resamples into the smaller buffer, so a 2 GB
        // GeoTIFF never becomes resident. The C++'s int arithmetic, which
        // wraps here where it would overflow there (a budget near i32::MAX).
        let longest = width.max(height);
        let step = (longest.wrapping_add(max_pixels).wrapping_sub(1) / max_pixels).max(1);
        let out_width = (width.wrapping_add(step).wrapping_sub(1) / step).max(1);
        let out_height = (height.wrapping_add(step).wrapping_sub(1) / step).max(1);
        let pixels = out_width as usize * out_height as usize;

        // The affine must be rescaled for the decimated grid: a step of n
        // means one output pixel now spans n input pixels, so the pixel-size
        // and rotation terms scale by the ACTUAL ratio (width/out_width), not
        // by `step` - those differ whenever the size is not an exact multiple
        // of the step.
        let mut geotransform = info.geotransform;
        let scale_x = f64::from(width) / f64::from(out_width);
        let scale_y = f64::from(height) / f64::from(out_height);
        geotransform[1] *= scale_x;
        geotransform[2] *= scale_y;
        geotransform[4] *= scale_x;
        geotransform[5] *= scale_y;
        let mut image = RasterImage {
            width: out_width,
            height: out_height,
            rgba: vec![255u8; pixels * 4],
            geotransform,
            has_geotransform: info.has_geotransform,
            projection_wkt: info.projection_wkt,
        };

        // One band asked for is that band alone, through its colour table
        // when it has one - never mixed into a colour the file says its bands
        // make.
        let roles = if only_band > 0 {
            let mut roles = BandRoles::default();
            let chosen = dataset.band(only_band);
            let paletted = chosen.is_some_and(|chosen| {
                chosen.color_interpretation() == gdal::GCI_PALETTE_INDEX
                    && chosen.color_table().is_some()
            });
            if paletted {
                roles.palette = chosen;
            } else {
                roles.grey = chosen;
            }
            roles
        } else {
            classify_bands(dataset)
        };

        let decimation = Decimation {
            width,
            height,
            out_width,
            out_height,
            pixels,
        };

        if let (Some(red), Some(green), Some(blue)) = (roles.red, roles.green, roles.blue) {
            let rgb_failure = || {
                make_error(
                    ErrorCode::FileImportFailure,
                    "GDAL failed to read RGB bands",
                    last_gdal_error(),
                )
            };
            let Some(r) = decimation.read_bytes(red) else {
                return Err(rgb_failure());
            };
            let Some(g) = decimation.read_bytes(green) else {
                return Err(rgb_failure());
            };
            let Some(b) = decimation.read_bytes(blue) else {
                return Err(rgb_failure());
            };
            let a = roles.alpha.and_then(|alpha| decimation.read_bytes(alpha));
            for (i, pixel) in image.rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                pixel[0] = r[i];
                pixel[1] = g[i];
                pixel[2] = b[i];
                pixel[3] = a.as_ref().map_or(255, |a| a[i]);
            }
            return Ok(image);
        }

        if let Some(palette) = roles.palette {
            let Some(indices) = decimation.read_bytes(palette) else {
                return Err(make_error(
                    ErrorCode::FileImportFailure,
                    "GDAL failed to read palette band",
                    last_gdal_error(),
                ));
            };
            let table = palette.color_table();
            let entries = table.map_or(0, |table| table.entry_count());
            for (pixel, &index) in image.rgba.as_chunks_mut::<4>().0.iter_mut().zip(&indices) {
                let index = c_int::from(index);
                let entry = match table {
                    Some(table) if index < entries => table.entry(index),
                    _ => None,
                };
                match entry {
                    Some(entry) => {
                        pixel[0] = entry.c1 as u8;
                        pixel[1] = entry.c2 as u8;
                        pixel[2] = entry.c3 as u8;
                        pixel[3] = entry.c4 as u8;
                    }
                    None => {
                        pixel[0] = index as u8;
                        pixel[1] = index as u8;
                        pixel[2] = index as u8;
                        pixel[3] = 255;
                    }
                }
            }
            return Ok(image);
        }

        let Some(grey) = roles.grey else {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "raster has no band this reader can display",
            ));
        };

        // Single band of arbitrary type - elevation, for instance. Read as
        // double and stretch over the band's real range: casting a Float32 DEM
        // straight to a byte would clamp every elevation above 255 to white.
        let mut values = vec![0.0_f64; pixels];
        let status = grey.read_f64(0, 0, width, height, &mut values, out_width, out_height);
        if status != cpl::CE_NONE {
            return Err(make_error(
                ErrorCode::FileImportFailure,
                "GDAL failed to read raster band",
                last_gdal_error(),
            ));
        }

        let no_data = grey.no_data_value();
        let is_no_data =
            |value: f64| !value.is_finite() || no_data.is_some_and(|no_data| value == no_data);

        // The stretch is the range of the values just read, never GDAL's band
        // statistics (audit IO-13). Forcing those makes GDAL's persistent
        // auxiliary metadata write <file>.aux.xml beside the user's file, and
        // on the next open GDAL trusts that cache without checking it against
        // the file, so a DEM replaced under the same name was stretched by the
        // OLD DEM's range. The decimated copy is the image being drawn, so its
        // own range is the right one to stretch over anyway.
        let mut minimum = f64::MAX;
        let mut maximum = f64::MIN;
        for &value in &values {
            if is_no_data(value) {
                continue;
            }
            // std::min and std::max, as the C++ has them.
            if value < minimum {
                minimum = value;
            }
            if maximum < value {
                maximum = value;
            }
        }
        let have_range = maximum > minimum;

        let span = if have_range { maximum - minimum } else { 1.0 };
        for (pixel, &value) in image.rgba.as_chunks_mut::<4>().0.iter_mut().zip(&values) {
            if is_no_data(value) {
                pixel[3] = 0; // transparent, so no-data shows the drawing beneath
                continue;
            }
            // f64::clamp is std::clamp here: neither bound is NaN.
            let normalised = if have_range {
                ((value - minimum) / span).clamp(0.0, 1.0)
            } else {
                0.5
            };
            let grey = (normalised * 255.0).round() as u8;
            pixel[0] = grey;
            pixel[1] = grey;
            pixel[2] = grey;
            pixel[3] = 255;
        }
        Ok(image)
    }

    pub fn vector_layers(&self) -> Result<Vec<VectorLayerInfo>> {
        let dataset = self.handle();
        let mut result = Vec::new();
        for index in 0..self.layer_count() {
            let Some(layer) = dataset.layer(index) else {
                continue;
            };
            // Force the count: a lazily-counted layer returns -1 otherwise,
            // which would become a huge number in the unsigned field.
            let (name, count, geometry_type) =
                (layer.name(), layer.feature_count(true), layer.geometry_type());
            result.push(VectorLayerInfo {
                name,
                feature_count: if count > 0 { count as u64 } else { 0 },
                projection_wkt: wkt_of_layer(layer),
                geometry_type: geometry_type_name(geometry_type),
            });
        }
        Ok(result)
    }

    /// One layer as a typed table: its fields as the file declares them
    /// (integers, reals, booleans, text; dates as ISO 8601 text), and each
    /// feature's geometry as its simple parts, a multi-geometry's members in
    /// one feature. The table carries the layer's name and coordinate system.
    /// What the read could not carry - a TIN, a curve made chords - is
    /// counted in `report`, with GDAL's warnings. InvalidArgument for a layer
    /// index out of range.
    pub fn read_table(
        &self,
        layer_index: i32,
        options: &VectorReadOptions,
        report: Option<&mut VectorReadReport>,
    ) -> Result<FeatureTable> {
        let count = self.layer_count();
        if layer_index < 0 || layer_index >= count {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "vector layer index is out of range",
                format!("requested {layer_index} of {count}"),
            ));
        }
        let Some(layer) = self.handle().layer(layer_index) else {
            return Err(Error::new(
                ErrorCode::Internal,
                "GDAL returned a null layer",
            ));
        };
        let mut local = VectorReadReport::default();
        let out = report.unwrap_or(&mut local);
        // Warnings GDAL raises while this read runs, on this thread; the
        // process-wide quiet handler keeps them off stderr, and this keeps
        // them from being lost.
        let errors = CplErrorCollector::new();
        set_filters(layer, options)?;
        let table = read_layer(layer, &self.driver_name(), options, out);
        // The layer belongs to the dataset, which may be read again: a filter
        // left on it would quietly narrow the next read.
        clear_filters(layer);
        out.warnings.extend(errors.warnings());
        Ok(table)
    }

    /// The rows of an SQL statement run on the dataset (GDALDataset::
    /// ExecuteSQL) as a typed table, as read_table reads a layer: `dialect`
    /// is "OGRSQL", "SQLITE" or "" for the driver's own. options.spatial_filter
    /// is handed to ExecuteSQL as its spatial filter, options.attribute_filter
    /// set on the result. InvalidArgument, with GDAL's message, for a
    /// statement GDAL refuses, and for one that gives no rows as a layer (an
    /// UPDATE): IMPORT reads, it never writes to the file it imports.
    pub fn read_sql(
        &self,
        statement: &str,
        dialect: &str,
        options: &VectorReadOptions,
        report: Option<&mut VectorReadReport>,
    ) -> Result<FeatureTable> {
        // IMPORT reads; it never writes to the file it imports. The dataset is
        // open read-only, so a write would fail anyway - but with a driver's
        // message about permissions, not the reason.
        let text = trimmed(statement.as_bytes());
        let end = text
            .iter()
            .position(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | b'('))
            .unwrap_or(text.len());
        let first = text[..end].to_ascii_lowercase();
        if first != b"select" && first != b"with" {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "only a SELECT statement is read: an import never changes its file",
                String::from_utf8_lossy(text).into_owned(),
            ));
        }
        let folded = dialect.to_ascii_lowercase();
        if !folded.is_empty() && folded != "ogrsql" && folded != "sqlite" {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the SQL dialect is ogrsql or sqlite",
                dialect,
            ));
        }
        let dataset = self.handle();
        let mut local = VectorReadReport::default();
        let out = report.unwrap_or(&mut local);
        let errors = CplErrorCollector::new();
        let area = options.spatial_filter.map(|rect| {
            let ring = OgrGeometry::create(ogr::WKB_LINEAR_RING);
            ring.add_point_2d(rect[0], rect[1]);
            ring.add_point_2d(rect[2], rect[1]);
            ring.add_point_2d(rect[2], rect[3]);
            ring.add_point_2d(rect[0], rect[3]);
            ring.add_point_2d(rect[0], rect[1]);
            let polygon = OgrGeometry::create(ogr::WKB_POLYGON);
            polygon.add_geometry_directly(ring);
            polygon
        });
        let sql = cpl::c_string(text);
        let dialect_name = match folded.as_str() {
            "" => None,
            "sqlite" => Some(c"SQLITE"),
            _ => Some(c"OGRSQL"),
        };
        reset_last_error();
        // The result set is the dataset's to free, whatever happens below:
        // dropping it releases it.
        let rows = dataset.execute_sql(&sql, area.as_ref(), dialect_name);
        let Some(held) = rows else {
            let why = last_gdal_error();
            return Err(make_error(
                ErrorCode::InvalidArgument,
                if why.is_empty() {
                    String::from("the statement gives no rows to import")
                } else {
                    format!("GDAL could not run the statement: {why}")
                },
                String::from_utf8_lossy(text).into_owned(),
            ));
        };
        let rest = VectorReadOptions {
            spatial_filter: None, // ExecuteSQL has it
            ..options.clone()
        };
        set_filters(held.layer(), &rest)?;
        let table = read_layer(held.layer(), &self.driver_name(), &rest, out);
        out.warnings.extend(errors.warnings());
        Ok(table)
    }

    /// Features of one layer, with every value as text. max_features == 0
    /// reads them all. Geometries are flattened: a MultiPolygon feature yields
    /// one VectorFeature per polygon, each carrying a copy of the attributes.
    /// The same read as read_table, with the values written out as text.
    pub fn read_features(&self, layer_index: i32, max_features: u64) -> Result<Vec<VectorFeature>> {
        let mut report = VectorReadReport::default();
        self.read_features_with_report(layer_index, max_features, &mut report)
    }

    /// The same, saying what it could not carry.
    pub fn read_features_with_report(
        &self,
        layer_index: i32,
        max_features: u64,
        report: &mut VectorReadReport,
    ) -> Result<Vec<VectorFeature>> {
        let options = VectorReadOptions {
            max_features,
            ..VectorReadOptions::default()
        };
        let table = self.read_table(layer_index, &options, Some(report))?;
        let mut features = Vec::new();
        for feature in table.features {
            let mut attributes = BTreeMap::new();
            for (field, value) in table.fields.iter().zip(&feature.values) {
                if *value != FieldValue::Null {
                    // The first field of a name wins, as std::map::emplace has it.
                    attributes
                        .entry(field.name.clone())
                        .or_insert_with(|| text_of(value));
                }
            }
            for part in feature.parts {
                features.push(VectorFeature {
                    geometry: part,
                    attributes: attributes.clone(),
                });
            }
        }
        Ok(features)
    }

    // ---- writing ----------------------------------------------------------------------------

    /// The driver a vector path is written with - formats::vector_writer_for,
    /// from GDAL's registry and its own choice for the name - or an error
    /// naming the extension when no writer claims it. Exposed so a UI can
    /// reject an unsupported extension before the user fills in a whole
    /// export dialog.
    pub fn vector_driver_for_path(path: &Path) -> Result<String> {
        // GDAL's registry, and its own choice among the writers that claim a
        // name, with Katana's overlay (formats.rs): the hand-kept table of ten
        // extensions this replaced could not write a FlatGeobuf, a GeoParquet,
        // a KMZ or a GPX that GDAL has writers for.
        vector_writer_for(path)
    }

    /// The same for a raster written by write_raster: .tif/.tiff -> GTiff,
    /// .asc -> AAIGrid, .img -> HFA. Unsupported, naming the extension, for
    /// anything else. Kept a table on purpose, not the registry: write_raster
    /// writes a DEM's Float64 heights, and most raster writers GDAL has (PNG,
    /// JPEG, GIF) hold 8- or 16-bit integers.
    pub fn raster_driver_for_path(path: &Path) -> Result<String> {
        ensure_registered();
        let extension = lower_extension(path);
        for (claimed, driver) in RASTER_DRIVERS {
            if extension == claimed {
                if Driver::by_name(driver).is_none() {
                    return Err(make_error(
                        ErrorCode::Unsupported,
                        format!("this GDAL build has no '{driver}' driver"),
                        cpl::path_text(path),
                    ));
                }
                return Ok(String::from(driver));
            }
        }
        if extension.is_empty() {
            return Err(make_error(
                ErrorCode::Unsupported,
                "cannot choose a raster driver: the path has no extension",
                cpl::path_text(path),
            ));
        }
        Err(make_error(
            ErrorCode::Unsupported,
            format!("no raster driver is registered for '.{extension}'"),
            cpl::path_text(path),
        ))
    }

    pub fn write_raster(path: &Path, options: &RasterExportOptions, values: &[f64]) -> Status {
        ensure_registered();
        // A message GDAL left from an earlier call must not become this
        // call's explanation of a failure GDAL did not describe.
        reset_last_error();

        if options.width <= 0 || options.height <= 0 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "raster export dimensions must be positive",
            ));
        }
        let expected = options.width as usize * options.height as usize;
        if values.len() != expected {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "raster export data does not match the stated dimensions",
                format!(
                    "{} values for {}x{}",
                    values.len(),
                    options.width,
                    options.height
                ),
            ));
        }

        let Some(driver) = Driver::by_name(&options.driver) else {
            return Err(make_error(
                ErrorCode::Unsupported,
                "raster driver is unavailable",
                options.driver.clone(),
            ));
        };

        // Two kinds of driver. GTiff and HFA CREATE a dataset and take its
        // pixels afterwards; AAIGrid (and PNG, JPEG...) can only COPY a
        // dataset that is already complete, because they write the file in one
        // pass. For the second kind the dataset is built in GDAL's in-memory
        // driver and copied, so a caller names the format it wants without
        // knowing which kind it is.
        let (can_create, can_copy) = (
            driver.metadata_bool(gdal::GDAL_DCAP_CREATE, false),
            driver.metadata_bool(gdal::GDAL_DCAP_CREATECOPY, false),
        );
        if !can_create && !can_copy {
            return Err(make_error(
                ErrorCode::Unsupported,
                "GDAL cannot write this raster format",
                options.driver.clone(),
            ));
        }
        let builder = if can_create {
            Some(driver)
        } else {
            Driver::by_name("MEM")
        };
        let Some(builder) = builder else {
            return Err(make_error(
                ErrorCode::Unsupported,
                "this GDAL build has no in-memory driver to build the raster in",
                options.driver.clone(),
            ));
        };

        let path_name = cpl::c_string(cpl::native_path(path));
        let build_name = if can_create {
            path_name.as_c_str()
        } else {
            c""
        };
        // Closed on every path below.
        let made = builder.create(
            build_name,
            options.width,
            options.height,
            1,
            gdal::GDT_FLOAT64,
            &cpl::CStringList::new(),
        );
        let Some(dataset) = made else {
            return Err(make_error(
                ErrorCode::FileExportFailure,
                format!("GDAL could not create '{}'", cpl::path_text(path)),
                last_gdal_error(),
            ));
        };

        // Every failure from here unwinds through this: close, then delete
        // what the driver created, so a raster without its georeferencing -
        // pixels that read back at the origin, one unit each - is never left
        // looking like a finished export (audit IO-14).
        let abandon = |dataset: Dataset, what: &str| -> Error {
            let message = last_gdal_error();
            // Closed once; already failing, so the first error is the one to
            // report.
            drop(dataset);
            if can_create {
                discard_output(driver, path);
            }
            make_error(ErrorCode::FileExportFailure, what, message)
        };

        let geotransform = options.geotransform;
        if dataset.set_geo_transform(&geotransform) != cpl::CE_NONE {
            return Err(abandon(
                dataset,
                "GDAL could not set the raster's georeferencing",
            ));
        }
        if !options.projection_wkt.is_empty() {
            let mut reference = SpatialReference::new();
            if !parse_crs(&options.projection_wkt, &mut reference) {
                return Err(abandon(
                    dataset,
                    "GDAL could not read the coordinate system to write",
                ));
            }
            // GDAL copies the reference.
            if dataset.set_spatial_ref(&reference) != cpl::CE_NONE {
                return Err(abandon(
                    dataset,
                    "GDAL could not set the raster's coordinate system",
                ));
            }
        }
        let band = dataset.band(1);
        if let Some(no_data) = options.no_data_value
            && band.map_or(cpl::CE_FAILURE, |band| band.set_no_data_value(no_data)) != cpl::CE_NONE
        {
            return Err(abandon(
                dataset,
                "GDAL could not set the raster's no-data value",
            ));
        }
        let written = band.map_or(cpl::CE_FAILURE, |band| {
            band.write_f64(
                0,
                0,
                options.width,
                options.height,
                values,
                options.width,
                options.height,
            )
        });
        if written != cpl::CE_NONE {
            return Err(abandon(dataset, "GDAL failed to write raster"));
        }

        let dataset = if can_create {
            dataset
        } else {
            // No options, no progress.
            let copy =
                driver.create_copy(&path_name, &dataset, false, &cpl::CStringList::new());
            let Some(copy) = copy else {
                let message = last_gdal_error();
                drop(dataset); // the in-memory dataset, closed once
                discard_output(driver, path);
                return Err(make_error(
                    ErrorCode::FileExportFailure,
                    format!("GDAL could not write '{}'", cpl::path_text(path)),
                    message,
                ));
            };
            // Closing an in-memory dataset only frees it; there is nothing it
            // could fail to write.
            drop(dataset);
            copy
        };

        // GDAL (3.7 on) reports here what it could only find out while
        // flushing the last blocks and the header: a full disk, a network
        // share gone.
        if dataset.close() != cpl::CE_NONE {
            let message = last_gdal_error();
            discard_output(driver, path);
            return Err(make_error(
                ErrorCode::FileExportFailure,
                format!("GDAL could not finish writing '{}'", cpl::path_text(path)),
                message,
            ));
        }
        Ok(())
    }

    /// Every value as a text field; write_tables with one table of them.
    pub fn write_vector(
        path: &Path,
        features: &[VectorFeature],
        options: &VectorExportOptions,
    ) -> Status {
        // One table of text fields, the attribute names in the order the
        // features first give them, so the field layout is deterministic
        // across runs (Rule 7).
        let mut table = FeatureTable {
            name: options.layer_name.clone(),
            ..FeatureTable::default()
        };
        if let Some(first) = features.first() {
            let first = first.geometry.kind;
            let uniform = features
                .iter()
                .all(|feature| feature.geometry.kind == first);
            table.kind = if uniform {
                first
            } else {
                GeometryKind::Unknown
            };
        }
        for feature in features {
            table.has_z |= feature.geometry.has_z;
            for name in feature.attributes.keys() {
                if !table.fields.iter().any(|field| field.name == *name) {
                    table.fields.push(FieldDef {
                        name: name.clone(),
                        r#type: FieldType::String,
                    });
                }
            }
        }
        for source in features {
            let values = table
                .fields
                .iter()
                .map(|field| match source.attributes.get(&field.name) {
                    None => FieldValue::Null,
                    Some(value) => FieldValue::Text(value.clone()),
                })
                .collect();
            table.features.push(Feature {
                parts: vec![source.geometry.clone()],
                values,
            });
        }
        let set = FeatureSet {
            tables: vec![table],
        };
        Self::write_tables(path, &set, options)?;
        Ok(())
    }

    /// Typed tables to a file, a layer per table, replacing the file - or,
    /// with options.append, added to it: refused, before anything is written,
    /// with InvalidArgument for a layer name the file has already and
    /// Unsupported for a format that adds no layers to a file, and on a
    /// failure part-way the layers this call made are deleted, the file's own
    /// left as they were. A table's field named OGR_STYLE is also each
    /// feature's OGR style string (a text's LABEL), which KML, DXF and MapInfo
    /// draw by. A single table's layer is named `options.layer_name` (when
    /// given), several are named after their tables. Fields keep their types
    /// where the format has them, and the next type it has where it does not.
    /// The format's own needs are Katana's to meet, not the caller's
    /// (docs/interop.md, "Fidelity"): KML, KMZ and GPX are written in
    /// longitude and latitude on WGS 84, converted, and a table with no
    /// coordinate system is refused with InvalidCRS before anything is
    /// written; CSV is written with its geometry and the .csvt that keeps its
    /// field types; a MapInfo table gets bounds from the data; a DXF writes
    /// areas as closed polylines, not solid hatches. A failure GDAL raises
    /// while writing a feature fails the write, and whatever was written is
    /// removed.
    pub fn write_tables(
        path: &Path,
        set: &FeatureSet,
        options: &VectorExportOptions,
    ) -> Result<VectorWriteReport> {
        ensure_registered();
        // See write_raster.
        reset_last_error();

        let driver_name = if options.driver.is_empty() {
            Self::vector_driver_for_path(path)?
        } else {
            options.driver.clone()
        };
        let Some(driver) = Driver::by_name(&driver_name) else {
            return Err(make_error(
                ErrorCode::Unsupported,
                "vector driver is unavailable",
                driver_name,
            ));
        };
        let mut report = VectorWriteReport::default();
        let errors = CplErrorCollector::new();

        // Everything that can refuse does so before a file exists.
        //
        // A coordinate system that cannot be read is refused. It used to be
        // dropped: the file was written with no CRS at all and the export
        // reported success - the layer then reads back as "no coordinate
        // system", indistinguishable from data that never had one.
        let mut references: Vec<Option<SpatialReference>> = Vec::with_capacity(set.tables.len());
        for table in &set.tables {
            let crs = if table.crs_wkt.is_empty() {
                &options.projection_wkt
            } else {
                &table.crs_wkt
            };
            if crs.is_empty() {
                references.push(None);
                continue;
            }
            let mut reference = SpatialReference::new();
            if !parse_crs(crs, &mut reference) {
                return Err(make_error(
                    ErrorCode::InvalidCrs,
                    "GDAL could not read the coordinate system to write",
                    last_gdal_error(),
                ));
            }
            references.push(Some(reference));
        }
        let mut converted: Option<FeatureSet> = None;
        if driver_holds_only_lon_lat(&driver_name) {
            // Said in the format's name: LIBKML, which writes a .kml and a
            // .kmz (formats.rs), is the name of GDAL's library, not of what a
            // person asked for.
            let format = if driver_name == "LIBKML" {
                "KML"
            } else {
                driver_name.as_str()
            };
            let mut copy = set.clone();
            for (table, reference) in copy.tables.iter_mut().zip(references.iter_mut()) {
                let Some(current) = reference.as_ref() else {
                    return Err(make_error(
                        ErrorCode::InvalidCrs,
                        format!(
                            "{format} holds longitude and latitude on WGS 84 and nothing else, \
                             and these coordinates are in no coordinate system Katana was told \
                             of, so they cannot be converted: set the project's coordinate \
                             system (CRS SET <code>) and export again"
                        ),
                        cpl::path_text(path),
                    ));
                };
                if !is_lon_lat(current) {
                    let crs = if table.crs_wkt.is_empty() {
                        options.projection_wkt.clone()
                    } else {
                        table.crs_wkt.clone()
                    };
                    to_lon_lat(table, &crs)?;
                    report.warnings.push(format!(
                        "coordinates were converted from {} to longitude and latitude on WGS 84 \
                         (EPSG:4326), the only coordinates {format} holds",
                        describe_crs(&crs)
                    ));
                }
                let mut wgs84 = SpatialReference::new();
                wgs84.set_well_known_geog_cs(c"WGS84");
                wgs84.set_axis_mapping_strategy(osr::OAMS_TRADITIONAL_GIS_ORDER);
                *reference = Some(wgs84);
                table.crs_wkt.clear();
            }
            converted = Some(copy);
        }
        let tables = converted.as_ref().unwrap_or(set);
        if driver_holds_no_areas(&driver_name) {
            for table in &tables.tables {
                if table.kind != GeometryKind::Point
                    && table.kind != GeometryKind::LineString
                    && !table.features.is_empty()
                {
                    return Err(make_error(
                        ErrorCode::Unsupported,
                        format!(
                            "{driver_name} holds points and lines, a layer of each; this table \
                             holds areas or a mix: write areas as their closed lines"
                        ),
                        table.name.clone(),
                    ));
                }
            }
        }

        // GDAL's DXF writer turns every polygon into a HATCH with a SOLID fill
        // unless told otherwise, so a closed polyline or a circle opened in a
        // CAD program as a filled solid. With the hatch off, the same driver
        // writes each ring as an LWPOLYLINE with its closed flag set.
        let _outline_not_hatch =
            (driver_name == "DXF").then(|| ScopedThreadConfig::new(c"DXF_WRITE_HATCH", c"NO"));

        let any_z = tables.tables.iter().any(|table| table.has_z);
        let mut katana_dataset: Vec<(&str, String)> = Vec::new();
        if driver_name == "GPX" {
            // GPX's own schema has no place for a field of ours; its
            // extensions element does, and GDAL reads it back.
            katana_dataset.push(("GPX_USE_EXTENSIONS", String::from("YES")));
        } else if driver_name == "KML" && any_z {
            // KML's default, clampToGround, puts every coordinate on the
            // ground whatever its altitude says: a height is only a height in
            // "absolute".
            katana_dataset.push(("AltitudeMode", String::from("absolute")));
        }
        let dataset_options = merged_options(&katana_dataset, &options.creation_options);

        let appending = options.append && path_exists(path).unwrap_or(false);
        let path_name = cpl::c_string(cpl::native_path(path));
        // The layers this call made, so a failed append takes away only those.
        let mut made: Vec<Vec<u8>> = Vec::new();
        let dataset = if appending {
            // The file as it is, opened to be added to: its own layers stay.
            // Closed on every path below.
            let opened = Dataset::open(
                &path_name,
                gdal::GDAL_OF_VECTOR | gdal::GDAL_OF_UPDATE,
                &cpl::CStringList::new(),
            );
            let Some(dataset) = opened else {
                return Err(make_error(
                    ErrorCode::FileExportFailure,
                    format!(
                        "GDAL could not open '{}' to add to it",
                        cpl::path_text(path)
                    ),
                    last_gdal_error(),
                ));
            };
            if !dataset.test_capability(ogr::ODS_C_CREATE_LAYER) {
                drop(dataset); // closed once
                return Err(make_error(
                    ErrorCode::Unsupported,
                    format!(
                        "{driver_name} adds no layer to a file that exists: write a new file, \
                         or a GeoPackage"
                    ),
                    cpl::path_text(path),
                ));
            }
            for t in 0..tables.tables.len() {
                let name = layer_name_for(tables, t, options);
                let name_c = cpl::c_string(&name);
                if dataset.layer_by_name(&name_c).is_some() {
                    drop(dataset); // closed once
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        format!(
                            "the file has a layer '{name}' already: name the new one with layername="
                        ),
                        cpl::path_text(path),
                    ));
                }
            }
            dataset
        } else {
            // Most drivers refuse to overwrite. Remove an existing file first
            // so that re-exporting to the same name behaves the way a user
            // expects a Save As to.
            remove_path(path);
            // Closed on every path below.
            let made = driver.create(&path_name, 0, 0, 0, gdal::GDT_UNKNOWN, &dataset_options);
            let Some(dataset) = made else {
                return Err(make_error(
                    ErrorCode::FileExportFailure,
                    format!("GDAL could not create '{}'", cpl::path_text(path)),
                    last_gdal_error(),
                ));
            };
            dataset
        };

        // A half-written export is worse than none: a Shapefile is three or
        // more files, and leaving a truncated set behind invites someone to
        // open it and believe it. Every failure below unwinds through this,
        // which closes the dataset and asks the DRIVER to delete it - the
        // driver knows about the sidecar files. An append deletes the layers
        // it made and leaves the file's own. The dataset is handed over, so
        // nothing can use it after; the layers borrowed from it are not used
        // again on a path that returns the error.
        let abandon = |mut dataset: Dataset, made: &[Vec<u8>], error: Error| -> Error {
            if appending {
                for name in made {
                    // Each layer's name is read before its layer is deleted.
                    let count = dataset.layer_count();
                    for i in (0..count).rev() {
                        if dataset
                            .layer(i)
                            .is_some_and(|layer| layer.name_bytes() == *name)
                        {
                            dataset.delete_layer(i);
                            break;
                        }
                    }
                }
                drop(dataset); // closed once
                return error;
            }
            drop(dataset); // closed once
            discard_output(driver, path);
            error
        };

        // Every layer and its fields first, then every feature in one
        // transaction: a GeoPackage creates its tables outside it.
        let mut written: Vec<Written<'_>> = Vec::with_capacity(tables.tables.len());
        for (t, table) in tables.tables.iter().enumerate() {
            let name = cpl::c_string(layer_name_for(tables, t, options));
            let layer_options = merged_options(
                &katana_layer_options(&driver_name, table),
                &options.layer_creation_options,
            );
            // The reference is copied by the driver; the layer is the
            // dataset's.
            let created = dataset.create_layer(
                &name,
                references[t].as_ref(),
                layer_type_of(table),
                &layer_options,
            );
            let Some(layer) = created else {
                return Err(abandon(
                    dataset,
                    &made,
                    make_error(
                        ErrorCode::FileExportFailure,
                        "GDAL could not create the vector layer",
                        last_gdal_error(),
                    ),
                ));
            };
            made.push(layer.name_bytes());
            let mut out = Written {
                layer,
                field_index: Vec::new(),
                altitude_index: -1,
                style_field: -1,
            };
            for (f, field) in table.fields.iter().enumerate() {
                if field.name == "OGR_STYLE" {
                    out.style_field = f as c_int;
                }
            }
            // Some formats have a FIXED set of fields and refuse any other: a
            // DXF layer has Layer, Linetype, Text and a few more, and
            // CreateField fails on everything else. Such a layer is written
            // with the fields it has; a value is kept when the format already
            // has a field of that name (OGR matches names case-insensitively,
            // so ours `layer` lands in DXF's `Layer`) and dropped otherwise,
            // and the report names what was dropped.
            let can_create_fields = layer.test_capability(ogr::OLC_CREATE_FIELD);
            let field_count = || layer.definition().map_or(0, |defn| defn.field_count());
            for (f, field) in table.fields.iter().enumerate() {
                let field_name = cpl::c_string(&field.name);
                if !can_create_fields {
                    let index = layer
                        .definition()
                        .map_or(-1, |defn| defn.field_index(&field_name));
                    out.field_index.push(index);
                    if index < 0 && !report.fields_not_written.contains(&field.name) {
                        report.fields_not_written.push(field.name.clone());
                    }
                    continue;
                }
                let (field_type, sub_type) = ogr_field_for(field, table, f, driver);
                // Made, handed to the layer (which copies it) and destroyed
                // here; the counts are the layer's.
                let mut definition = FieldDefn::new(&field_name, field_type);
                definition.set_sub_type(sub_type);
                let before = field_count();
                let created = layer.create_field(&definition, true);
                drop(definition);
                let after = field_count();
                if created != ogr::OGRERR_NONE {
                    return Err(abandon(
                        dataset,
                        &made,
                        make_error(
                            ErrorCode::FileExportFailure,
                            format!("GDAL could not create field '{}'", field.name),
                            last_gdal_error(),
                        ),
                    ));
                }
                // By position, not by name: a shapefile shortens a long name
                // (and says so, a warning in the report).
                out.field_index
                    .push(if after > before { before } else { -1 });
            }
            if driver_name == "LIBKML" && table.has_z {
                // KMZ's writer takes a feature's altitude mode from a field of
                // this name, and gives a 2D coordinate an altitude of 0 that
                // its default mode, clampToGround, keeps on the ground. The
                // field definition made is copied by the layer and destroyed
                // here.
                let altitude_index = || {
                    layer
                        .definition()
                        .map_or(-1, |defn| defn.field_index(c"altitudeMode"))
                };
                out.altitude_index = altitude_index();
                if out.altitude_index < 0 {
                    let altitude = FieldDefn::new(c"altitudeMode", ogr::OFT_STRING);
                    if layer.create_field(&altitude, true) == ogr::OGRERR_NONE {
                        out.altitude_index = altitude_index();
                    }
                    drop(altitude);
                }
            }
            written.push(out);
        }

        // Every feature in ONE transaction, where the format has them. A
        // GeoPackage is SQLite, and SQLite commits - and so syncs the file to
        // disk - once per transaction; given none, every feature was a
        // transaction of its own, and a 27,886-entity corridor drawing took
        // 67 s to export where ogr2ogr, batching, took 3.2 s for the same
        // features. Not forced: a driver without real transactions (a
        // shapefile) would emulate one by copying the whole file.
        let mut in_transaction = false;
        if dataset.test_capability(ogr::ODS_C_TRANSACTIONS) {
            if dataset.start_transaction(false) != ogr::OGRERR_NONE {
                return Err(abandon(
                    dataset,
                    &made,
                    make_error(
                        ErrorCode::FileExportFailure,
                        "GDAL could not start a transaction",
                        last_gdal_error(),
                    ),
                ));
            }
            in_transaction = true;
        }
        // A failure part-way leaves nothing: the transaction is rolled back
        // and then the file is deleted, as a failure outside one always was.
        let abandon_features =
            |dataset: Dataset, in_transaction: bool, made: &[Vec<u8>], error: Error| -> Error {
                if in_transaction {
                    dataset.rollback_transaction();
                }
                abandon(dataset, made, error)
            };

        // The loop breaks out with its failure, and the dataset is abandoned after it: a feature
        // borrows its layer, which borrows the dataset, so the dataset cannot be moved while one
        // is in scope.
        let failure = 'write: {
        for (table, out) in tables.tables.iter().zip(&written) {
            for source in &table.features {
                let Some(geometry) = feature_geometry(source, table.kind) else {
                    report.features_skipped += 1;
                    continue;
                };
                // Asked before the feature takes the geometry.
                let geometry_is_3d = geometry.is_3d();
                // Destroyed below, once the layer has copied it.
                let created = out.layer.definition().and_then(OgrFeature::create);
                let Some(mut feature) = created else {
                    break 'write Some(make_error(
                        ErrorCode::FileExportFailure,
                        "GDAL could not write a feature",
                        last_gdal_error(),
                    ));
                };
                // The geometry is the feature's from here.
                feature.set_geometry_directly(geometry);
                for (value, &index) in source.values.iter().zip(&out.field_index) {
                    if index < 0 {
                        continue;
                    }
                    // Text is copied.
                    match value {
                        FieldValue::Null => feature.set_field_null(index),
                        FieldValue::Boolean(held) => {
                            feature.set_field_integer(index, c_int::from(*held));
                        }
                        FieldValue::Integer64(held) => {
                            feature.set_field_integer64(index, *held);
                        }
                        FieldValue::Real(held) => {
                            feature.set_field_double(index, *held);
                        }
                        FieldValue::Text(held) => {
                            feature.set_field_string(index, &cpl::c_string(held));
                        }
                    }
                }
                if out.altitude_index >= 0 && geometry_is_3d {
                    feature.set_field_string(out.altitude_index, c"absolute");
                }
                if out.style_field >= 0
                    && let Some(FieldValue::Text(style)) =
                        source.values.get(out.style_field as usize)
                {
                    feature.set_style_string(&cpl::c_string(style));
                }
                let before = errors.size();
                // The layer copies the feature, which is destroyed once.
                let status = out.layer.create_feature(&feature);
                drop(feature);
                // A writer that cannot write a geometry may say so and succeed
                // anyway: KML writes the placemark without it. What it says is
                // the failure of the write.
                let failure = errors.first_failure(before);
                if status != ogr::OGRERR_NONE || failure.is_some() {
                    break 'write Some(make_error(
                        ErrorCode::FileExportFailure,
                        "GDAL could not write a feature",
                        failure.map_or_else(last_gdal_error, |message| message.text),
                    ));
                }
                report.features_written += 1;
            }
        }
        None
        };
        if let Some(error) = failure {
            return Err(abandon_features(dataset, in_transaction, &made, error));
        }
        // Where the features reach the file: a full disk shows here.
        if in_transaction && dataset.commit_transaction() != ogr::OGRERR_NONE {
            // A failed commit has already ended the transaction.
            return Err(abandon_features(
                dataset,
                false,
                &made,
                make_error(
                    ErrorCode::FileExportFailure,
                    format!(
                        "GDAL could not commit the features to '{}'",
                        cpl::path_text(path)
                    ),
                    last_gdal_error(),
                ),
            ));
        }

        // A shapefile flushes its .dbf here, and a GeoPackage writes its last
        // metadata (the extent, the spatial index), so a full disk can show
        // here as well as at the commit above (audit IO-14). The layers in
        // `written` go with the dataset and are not used after.
        if dataset.close() != cpl::CE_NONE {
            let message = last_gdal_error();
            if !appending {
                discard_output(driver, path);
            }
            return Err(make_error(
                ErrorCode::FileExportFailure,
                format!("GDAL could not finish writing '{}'", cpl::path_text(path)),
                message,
            ));
        }
        report.warnings.extend(errors.warnings());
        Ok(report)
    }
}

/// A layer write_tables made, and where each table field went.
struct Written<'a> {
    layer: Layer<'a>,
    /// Per table field; -1 when the layer has none for it.
    field_index: Vec<c_int>,
    altitude_index: c_int,
    /// The table's OGR_STYLE field, when it has one.
    style_field: c_int,
}

// ---- writing helpers ------------------------------------------------------------------------

/// KEY=VALUE options, Katana's first and the caller's over them: a key the
/// caller gives wins, whatever its case.
fn merged_options(katana: &[(&str, String)], caller: &[String]) -> cpl::CStringList {
    let mut merged = cpl::CStringList::new();
    for (key, value) in katana {
        merged.set_name_value(key.as_bytes(), value.as_bytes());
    }
    for option in caller {
        match option.find('=') {
            // GDAL says what it makes of it.
            None | Some(0) => merged.add(option),
            Some(equals) => merged.set_name_value(
                &option.as_bytes()[..equals],
                &option.as_bytes()[equals + 1..],
            ),
        }
    }
    merged
}

/// Whether a driver's space-separated list has the word.
fn listed(list: Option<&str>, word: &str) -> bool {
    list.is_some_and(|list| list.split(' ').any(|item| !item.is_empty() && item == word))
}

/// The OGR type a field is written as: its own where the driver has it, else
/// the nearest the driver has that holds every value - an Integer64 that fits
/// in 32 bits is an Integer in KML, whose writer knows no Integer64 and would
/// write it as text. A driver that lists nothing takes what it is given.
fn ogr_field_for(
    field: &FieldDef,
    table: &FeatureTable,
    index: usize,
    driver: Driver,
) -> (ogr::OgrFieldType, ogr::OgrFieldSubType) {
    let types = driver.metadata_item(gdal::GDAL_DMD_CREATIONFIELDDATATYPES);
    let sub_types = driver.metadata_item(gdal::GDAL_DMD_CREATIONFIELDDATASUBTYPES);
    let any_type = types.as_deref().is_none_or(str::is_empty);
    let has = |name: &str| any_type || listed(types.as_deref(), name);
    // KML's schema has no 64-bit integer (OGC KML 2.2, 9.5: a SimpleField is
    // int, uint, short, ushort, float, double, bool or string), and LIBKML,
    // which declares Integer64 all the same, writes one as a string: a count
    // came back "3". So for it an Integer64 goes as an int where it fits.
    let integer64_as_text = driver.description() == "LIBKML";
    match field.r#type {
        FieldType::Boolean => {
            if has("Integer") {
                let sub_type = if any_type || listed(sub_types.as_deref(), "Boolean") {
                    ogr::OFST_BOOLEAN
                } else {
                    ogr::OFST_NONE
                };
                return (ogr::OFT_INTEGER, sub_type);
            }
        }
        FieldType::Integer64 => {
            if has("Integer64") && !integer64_as_text {
                return (ogr::OFT_INTEGER64, ogr::OFST_NONE);
            }
            let int32 = i64::from(i32::MIN)..=i64::from(i32::MAX);
            let fits = table
                .features
                .iter()
                .all(|feature| match feature.values.get(index) {
                    Some(FieldValue::Integer64(value)) => int32.contains(value),
                    _ => true,
                });
            if fits && has("Integer") {
                return (ogr::OFT_INTEGER, ogr::OFST_NONE);
            }
        }
        FieldType::Real => {
            if has("Real") {
                return (ogr::OFT_REAL, ogr::OFST_NONE);
            }
        }
        FieldType::Date => {
            if has("Date") {
                return (ogr::OFT_DATE, ogr::OFST_NONE);
            }
        }
        FieldType::DateTime => {
            if has("DateTime") {
                return (ogr::OFT_DATE_TIME, ogr::OFST_NONE);
            }
        }
        FieldType::String => {}
    }
    (ogr::OFT_STRING, ogr::OFST_NONE)
}

fn layer_type_of(table: &FeatureTable) -> ogr::OgrWkbGeometryType {
    let multi = table.features.iter().any(|feature| feature.parts.len() > 1);
    let curved = table
        .features
        .iter()
        .any(|feature| feature.parts.iter().any(VectorGeometry::has_arcs));
    let geometry_type = match table.kind {
        GeometryKind::Point => {
            if multi {
                ogr::WKB_MULTI_POINT
            } else {
                ogr::WKB_POINT
            }
        }
        GeometryKind::LineString => match (curved, multi) {
            (true, true) => ogr::WKB_MULTI_CURVE,
            (true, false) => ogr::WKB_COMPOUND_CURVE,
            (false, true) => ogr::WKB_MULTI_LINE_STRING,
            (false, false) => ogr::WKB_LINE_STRING,
        },
        GeometryKind::Polygon => match (curved, multi) {
            (true, true) => ogr::WKB_MULTI_SURFACE,
            (true, false) => ogr::WKB_CURVE_POLYGON,
            (false, true) => ogr::WKB_MULTI_POLYGON,
            (false, false) => ogr::WKB_POLYGON,
        },
        GeometryKind::Unknown => return ogr::WKB_UNKNOWN,
    };
    if table.has_z {
        geometry::with_z(geometry_type)
    } else {
        geometry_type
    }
}

fn feature_geometry(feature: &Feature, kind: GeometryKind) -> Option<OgrGeometry> {
    if feature.parts.is_empty() {
        return None;
    }
    if feature.parts.len() == 1 {
        return make_ogr_geometry(&feature.parts[0]);
    }
    let collection = OgrGeometry::create(match kind {
        GeometryKind::Point => ogr::WKB_MULTI_POINT,
        GeometryKind::LineString => ogr::WKB_MULTI_LINE_STRING,
        GeometryKind::Polygon => ogr::WKB_MULTI_POLYGON,
        GeometryKind::Unknown => ogr::WKB_GEOMETRY_COLLECTION,
    });
    for part in &feature.parts {
        if let Some(member) = make_ogr_geometry(part)
            && !collection.add_geometry_directly(member)
        {
            // A curve cannot join a MultiLineString: the collection that can
            // hold anything takes the whole feature instead. (GDAL has
            // deleted the refused member; the collection goes here.)
            drop(collection);
            let anything = OgrGeometry::create(ogr::WKB_GEOMETRY_COLLECTION);
            for again in &feature.parts {
                if let Some(one) = make_ogr_geometry(again) {
                    anything.add_geometry_directly(one);
                }
            }
            return Some(anything);
        }
    }
    Some(collection)
}

/// The extent of a table's coordinates: MapInfo's bounds.
struct Extent {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Extent {
    fn valid(&self) -> bool {
        self.min_x <= self.max_x && self.min_y <= self.max_y
    }
}

fn extent_of(table: &FeatureTable) -> Extent {
    let mut extent = Extent {
        min_x: f64::INFINITY,
        min_y: f64::INFINITY,
        max_x: f64::NEG_INFINITY,
        max_y: f64::NEG_INFINITY,
    };
    let points = table
        .features
        .iter()
        .flat_map(|feature| &feature.parts)
        .flat_map(|part| &part.parts)
        .flatten();
    for point in points {
        if !point.x.is_finite() || !point.y.is_finite() {
            continue;
        }
        // std::min and std::max, as the C++ has them.
        if point.x < extent.min_x {
            extent.min_x = point.x;
        }
        if point.y < extent.min_y {
            extent.min_y = point.y;
        }
        if extent.max_x < point.x {
            extent.max_x = point.x;
        }
        if extent.max_y < point.y {
            extent.max_y = point.y;
        }
    }
    extent
}

/// A MapInfo table keeps each coordinate as a 32-bit integer across its
/// bounds, and GDAL's default bounds for a table without a known projection
/// are wide enough to make that step about a centimetre: 330100 read back as
/// 330099.99 (measured, docs/interop.md "Fidelity"). The data's own extent,
/// widened by a tenth on each side so an edit that moves a vertex out a
/// little still fits, makes the step span / 2^32: 5e-8 m across a 200 m site,
/// 5e-4 m across 2000 km.
fn map_info_bounds(extent: &Extent) -> String {
    let width = extent.max_x - extent.min_x;
    let height = extent.max_y - extent.min_y;
    // std::max, as the C++ has it.
    let span = if width < height { height } else { width };
    let margin = 0.1 * span + 1.0;
    format!(
        "{},{},{},{}",
        format_exact_real(extent.min_x - margin),
        format_exact_real(extent.min_y - margin),
        format_exact_real(extent.max_x + margin),
        format_exact_real(extent.max_y + margin)
    )
}

fn katana_layer_options(driver: &str, table: &FeatureTable) -> Vec<(&'static str, String)> {
    let mut options = Vec::new();
    if driver == "CSV" {
        // Without a geometry option the CSV writer drops the geometry and
        // writes the fields alone (measured: "katana_id,layer", reported as
        // exported). Points as X, Y (and Z when every one has a height), for
        // a spreadsheet; anything else, or points of mixed dimension, as WKT.
        let points = table.kind == GeometryKind::Point
            && !table.features.iter().any(|feature| feature.parts.len() > 1);
        let mut heighted: usize = 0;
        let mut total: usize = 0;
        for part in table.features.iter().flat_map(|feature| &feature.parts) {
            total += 1;
            heighted += usize::from(part.has_z);
        }
        let geometry = if !points {
            "AS_WKT"
        } else if heighted == 0 {
            "AS_XY"
        } else if heighted == total {
            "AS_XYZ"
        } else {
            "AS_WKT"
        };
        options.push(("GEOMETRY", String::from(geometry)));
        // The .csvt beside it keeps each field's type, and says which columns
        // are X and Y; without it every field reads back as text.
        options.push(("CREATE_CSVT", String::from("YES")));
    } else if driver == "MapInfo File" {
        let extent = extent_of(table);
        if extent.valid() {
            options.push(("BOUNDS", map_info_bounds(&extent)));
        }
    }
    options
}

fn layer_name_for(set: &FeatureSet, index: usize, options: &VectorExportOptions) -> String {
    let table = &set.tables[index];
    if set.tables.len() == 1 && !options.layer_name.is_empty() {
        return options.layer_name.clone();
    }
    if table.name.is_empty() {
        format!("features{}", index + 1)
    } else {
        table.name.clone()
    }
}

fn is_lon_lat(reference: &SpatialReference) -> bool {
    let mut wgs84 = SpatialReference::new();
    wgs84.set_well_known_geog_cs(c"WGS84");
    wgs84.set_axis_mapping_strategy(osr::OAMS_TRADITIONAL_GIS_ORDER);
    reference.is_geographic() && reference.is_same(&wgs84)
}

/// Every coordinate of `table` from `crs` to longitude and latitude on WGS
/// 84, by the one reprojection (reproject::reproject_features), heights
/// untouched.
fn to_lon_lat(table: &mut FeatureTable, crs: &str) -> Status {
    let geometries: Vec<VectorFeature> = table
        .features
        .iter()
        .flat_map(|feature| &feature.parts)
        .map(|part| VectorFeature {
            geometry: part.clone(),
            attributes: BTreeMap::new(),
        })
        .collect();
    let mut moved = reproject_features(geometries, crs, "EPSG:4326")?.into_iter();
    for part in table
        .features
        .iter_mut()
        .flat_map(|feature| &mut feature.parts)
    {
        if let Some(next) = moved.next() {
            *part = next.geometry;
        }
    }
    Ok(())
}

// ---- the drivers' rules ---------------------------------------------------------------------

/// True for formats that store exactly ONE geometry type per layer, so a
/// mixed collection cannot be written in a single file. Shapefile is the
/// notable case: its header fixes the shape type, and GDAL only discovers the
/// mismatch on the first feature of a different type - by which point a
/// partial file exists.
pub fn driver_holds_one_geometry_type(driver: &str) -> bool {
    driver == "ESRI Shapefile" || driver == "MapInfo File"
}

/// True for a format whose layers have a fixed set of fields and accept no
/// others (DXF). write_vector still writes such a file - geometry, and any
/// attribute the format already has a field for - but everything else is
/// dropped, and a caller should say so rather than let it pass unremarked.
pub fn driver_has_fixed_fields(driver: &str) -> bool {
    driver == "DXF"
}

/// True for formats whose specification fixes the coordinate reference
/// system, so any coordinates written are reinterpreted as that CRS
/// regardless of what the data actually is. RFC 7946 pins GeoJSON to WGS 84
/// longitude/latitude.
pub fn driver_assumes_wgs84(driver: &str) -> bool {
    driver == "GeoJSON"
}

/// True for formats that can hold nothing but longitude and latitude on WGS
/// 84 (KML, KMZ, GPX): write_tables converts to them, and refuses a table
/// whose coordinate system it does not know. GDAL's own KML writer, handed a
/// projected coordinate, writes the placemark without its geometry and
/// succeeds.
pub fn driver_holds_only_lon_lat(driver: &str) -> bool {
    driver == "KML" || driver == "LIBKML" || driver == "GPX"
}

/// True for a format of points and lines only, one layer of each (GPX's
/// waypoints and routes): an area goes to it as the closed line around it.
pub fn driver_holds_no_areas(driver: &str) -> bool {
    driver == "GPX"
}

/// Whether two coordinate systems are one, as GDAL's
/// OGRSpatialReference::IsSame judges it, not by their text: a .prj's naming
/// of a system and EPSG's definition of it are the same, and so are a
/// geographic 3D system and its 2D one (GDAL declares a GeoJSON with heights
/// EPSG:4979), and the axis order data is mapped to is not compared. Nothing
/// when either is empty or cannot be read: nothing is known to differ.
pub fn same_crs(a: &str, b: &str) -> Option<bool> {
    if a.is_empty() || b.is_empty() {
        return None;
    }
    if a == b {
        return Some(true);
    }
    ensure_registered();
    let mut first = SpatialReference::new();
    let mut second = SpatialReference::new();
    let quiet = QuietHandler::push();
    let read = parse_crs(a, &mut first) && parse_crs(b, &mut second);
    if read {
        // A 3D geographic system is its 2D one with heights, which Katana
        // keeps apart from the plan coordinates anyway.
        if first.is_geographic() && first.axes_count() == 3 {
            first.demote_to_2d();
        }
        if second.is_geographic() && second.axes_count() == 3 {
            second.demote_to_2d();
        }
    }
    let same = read && first.is_same_with(&second, &[c"IGNORE_DATA_AXIS_TO_SRS_AXIS_MAPPING=YES"]);
    drop(quiet);
    if !read {
        return None;
    }
    Some(same)
}

/// A coordinate system for a person to read: its name, and its authority code
/// where GDAL can identify one - "GDA94 / MGA zone 56 (EPSG:28356)". Parsed by
/// GDAL's own OGRSpatialReference, which reads WKT1, WKT2 and PROJJSON alike,
/// rather than by picking at the text. Empty for an empty WKT; a WKT GDAL
/// cannot parse is described as such, never as blank, because "no CRS" and "a
/// CRS we could not read" are different things to the person importing it.
pub fn describe_crs(wkt: &str) -> String {
    if wkt.is_empty() {
        return String::new();
    }
    ensure_registered();
    let mut reference = SpatialReference::new();
    if !parse_crs(wkt, &mut reference) {
        // Never "": that would read as "no coordinate system", and a file that
        // declares one we cannot read is a different problem for its owner.
        return String::from("unrecognised coordinate system");
    }
    let mut text = reference
        .name()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| String::from("unnamed coordinate system"));

    let authority_code = |authority: Option<String>, code: Option<String>| match (authority, code) {
        (Some(authority), Some(code)) if !authority.is_empty() && !code.is_empty() => {
            format!("{authority}:{code}")
        }
        _ => String::new(),
    };
    let mut identified = authority_code(reference.authority_name(), reference.authority_code());

    // A WKT written without AUTHORITY nodes - every Esri .prj beside a
    // shapefile - is usually still an EPSG system, recognisable by its
    // parameters. When nothing can be recognised the name alone is the honest
    // answer, so a failure below is not an error.
    let quiet = QuietHandler::push();
    if identified.is_empty() {
        reference.auto_identify_epsg();
        identified = authority_code(reference.authority_name(), reference.authority_code());
    }
    if identified.is_empty() {
        // AutoIdentifyEPSG knows a handful of datums by name, and in GDAL
        // 3.13.2 identified none of WGS 84, WGS 84 / UTM 56S or an Esri .prj
        // of GDA94 / MGA 56 (measured 2026-09-23; it returned
        // OGRERR_UNSUPPORTED_SRS for each). FindMatches asks PROJ's database,
        // which named all three at confidence 100 - it is what
        // `gdalsrsinfo -e` reports. PROJ's scale (proj_identify): 100 and 90
        // are the same CRS with matching or similar names, 70 the same CRS by
        // another name, below that only a similarity. A code is shown only
        // for one candidate that is the same CRS; two at the top would make
        // the choice a guess.
        const SAME_CRS: c_int = 70;
        let matches = reference.find_matches();
        if matches.has_candidates()
            && matches.entries() >= 1
            && let Some(best) = matches.confidence(0)
            && best >= SAME_CRS
            && (matches.entries() == 1 || matches.confidence(1).is_some_and(|next| next < best))
        {
            identified = authority_code(matches.authority_name(0), matches.authority_code(0));
        }
    }
    drop(quiet);

    if !identified.is_empty() {
        text.push_str(&format!(" ({identified})"));
    }
    text
}

/// The GDAL version string, for the about box and for bug reports.
pub fn gdal_version() -> String {
    ensure_registered();
    dataset::version_info(c"RELEASE_NAME").unwrap_or_else(|| String::from("unknown"))
}
