//! What a file holds, read WITHOUT importing it
//! (include/katana/interop/dataset_info.hpp,
//! src/katana_interop/dataset_info.cpp) - the gdalinfo / pdal info a person
//! needs before choosing what to import and how: which layer of a GeoPackage
//! and what fields it has, whether a DEM is georeferenced and what its
//! no-data value is, how many points a LAS holds and whether it is COPC.
//!
//! One description, used by the desktop application's import dialogs, by
//! the INFO verb's records and, through INFO, by GIS > Dataset Information,
//! so none of them can disagree about what a file is. A GDAL dataset is
//! described from GDAL's own info JSON - `raster info` and `vector info`
//! through the geoprocessing bridge (katana_gis::processing) - kept verbatim
//! beside the description, so INFO ... JSON and the records are one reading.
//! GDAL opens a /vsi path or a URL itself, so those are described too. A
//! point cloud is PDAL's.

use std::path::{Path, PathBuf};

use katana_core::error::make_error;
use katana_core::json::{Json, JsonError, JsonResult};
use katana_core::path_text::path_exists;
use katana_core::text::{format_exact_real, parse_finite_double};
use katana_core::{ErrorCode, Result};
use katana_geometry::{Box2, Point2};
use katana_gis::gdal_adapter::describe_crs;
use katana_gis::processing::{ArgValue, DatasetPath, DatasetValue, RunRequest, path_text, run};
use katana_pointcloud::{PointCloudBounds, PointCloudEngine};

use crate::import::{SourceKind, kind_for_path, to_string};

/// One band of a raster. Absent is not zero: a band with no no-data value,
/// or whose statistics were never computed, says so.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BandDescription {
    /// 1-based, as GDAL numbers them.
    pub band: i32,
    /// GDAL's name: Byte, Int16, Float32 ...
    pub data_type: String,
    /// GDAL's colour interpretation: Gray, Red, Undefined ...
    pub colour: String,
    pub no_data: Option<f64>,
    /// From statistics the file carries, or computed when asked
    /// (DescribeOptions::statistics).
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub mean: Option<f64>,
    pub std_dev: Option<f64>,
    /// Each overview's width and height, largest first.
    pub overviews: Vec<(i32, i32)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RasterDescription {
    pub width: i32,
    pub height: i32,
    pub band_count: i32,
    pub georeferenced: bool,
    /// World units per pixel along the image's own columns and rows: the
    /// lengths of the geotransform's column and row vectors, so a rotated
    /// image reports its true pixel size. Meaningful only when georeferenced.
    pub pixel_width: f64,
    pub pixel_height: f64,
    /// All four corners through the full affine. Empty when not
    /// georeferenced: GDAL's default transform would put it at the origin,
    /// which is a guess.
    pub bounds: Box2,
    /// Band 1's.
    pub no_data_value: Option<f64>,
    pub bands: Vec<BandDescription>,
}

/// One attribute field of a vector layer, as GDAL types it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldDescription {
    pub name: String,
    /// GDAL's: String, Integer, Integer64, Real, Date, DateTime ...
    pub r#type: String,
    /// Boolean, Int16, Float32, JSON, UUID; empty for none.
    pub subtype: String,
    /// 0 when the format sets none.
    pub width: i32,
    pub precision: i32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorLayerDescription {
    pub name: String,
    pub feature_count: u64,
    /// GDAL's spelling, e.g. "Polygon".
    pub geometry_type: String,
    /// describe_crs of the layer's own CRS; empty when none.
    pub crs: String,
    /// Empty when the layer has none.
    pub extent: Box2,
    pub fields: Vec<FieldDescription>,
}

/// A dataset inside a container (a netCDF variable, a GeoPackage raster
/// table): what `IMPORT ... subdataset=` names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubdatasetDescription {
    /// What GDAL opens it by: NETCDF:"f.nc":elevation.
    pub name: String,
    /// For a person.
    pub description: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointCloudDescription {
    pub point_count: u64,
    pub bounds: PointCloudBounds,
    pub has_color: bool,
    /// A Cloud Optimised Point Cloud, which can answer a resolution query
    /// (PointCloudImportOptions::resolution) instead of being decimated.
    pub copc: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceDescription {
    pub path: PathBuf,
    pub kind: SourceKind,
    /// GDAL's driver short name ("GTiff", "GPKG"), or the PDAL reader
    /// ("readers.las", "readers.copc") for a point cloud.
    pub driver: String,
    /// describe_crs of the dataset's CRS; empty when the file declares none.
    pub crs: String,
    pub projection_wkt: String,
    /// A GeoPackage can hold rasters and vector layers at once, so these are
    /// filled from what the dataset actually contains, not from its kind.
    pub raster: Option<RasterDescription>,
    pub vector_layers: Vec<VectorLayerDescription>,
    pub subdatasets: Vec<SubdatasetDescription>,
    pub point_cloud: Option<PointCloudDescription>,
    /// GDAL's own info JSON, verbatim, for what it described: `raster info`,
    /// `vector info` and - DescribeOptions::multidim, for a multidimensional
    /// format - `mdim info`. Empty for what it is not.
    pub raster_json: String,
    pub vector_json: String,
    pub multidim_json: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DescribeOptions {
    /// Every band's minimum, maximum, mean and standard deviation, computed
    /// from every pixel - no .aux.xml is written beside the file for it (the
    /// bridge's rule, docs/geoprocessing.md "Sidecars").
    pub statistics: bool,
    /// Only this vector layer; empty for every layer. NotFound-like failure
    /// from GDAL for a layer the file does not have.
    pub layer: String,
    /// A multidimensional format's `mdim info` JSON as well.
    pub multidim: bool,
    /// GDAL is asked whatever the extension: INFO says what GDAL reads (a
    /// netCDF, a FlatGeobuf), where an import dialog asks only of what Katana
    /// imports.
    pub any_format: bool,
}

/// The PDAL reader for a point-cloud extension, named as PDAL names it.
/// Written out here because interop may not see PDAL and the engine does not
/// publish the reader it infers; the extensions are exactly
/// point_cloud_extensions(), so a new one there without a line here says
/// "unknown reader" rather than something untrue.
fn pdal_reader_for(path: &Path) -> String {
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    const READERS: [(&str, &str); 7] = [
        ("las", "readers.las"),
        ("laz", "readers.las"),
        ("copc", "readers.copc"),
        ("bpf", "readers.bpf"),
        ("ply", "readers.ply"),
        ("pcd", "readers.pcd"),
        ("e57", "readers.e57"),
    ];
    READERS
        .iter()
        .find(|(each, _)| extension == *each)
        .map_or_else(
            || String::from("unknown reader"),
            |&(_, reader)| String::from(reader),
        )
}

/// A dataset's name as GDAL is given it: UTF-8, whatever the code page.
fn gdal_name(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// `path.extension()` as the C++ spells it: with its dot, empty for none.
fn extension_with_dot(path: &Path) -> String {
    path.extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default()
}

/// GDAL's own `raster info`, `vector info` or `mdim info` of `name`, as its
/// JSON text, through the geoprocessing bridge: the bridge opens the dataset
/// on this thread, collects GDAL's errors for this run alone, and puts back
/// any .aux.xml a --stats would have written beside the file.
fn gdal_info(algorithm: &[&str], name: &str, tokens: Vec<String>, layer: &str) -> Result<String> {
    let request = RunRequest {
        path: algorithm.iter().map(|&word| String::from(word)).collect(),
        tokens,
        values: vec![(
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Path(DatasetPath {
                path: String::from(name),
                open_options: Vec::new(),
                layer: String::from(layer),
            })),
        )],
        ..RunRequest::default()
    };
    let outputs = run(&request, None, None)?;
    match outputs.text {
        Some(text) => Ok(text),
        None => Err(make_error(
            ErrorCode::CommandRejected,
            format!("GDAL {} printed nothing", path_text(&request.path)),
            name,
        )),
    }
}

fn number(value: &Json) -> Option<f64> {
    if value.is_number() {
        return value.get::<f64>().ok();
    }
    // GDAL writes a NaN no-data value as the word: a value all the same, and
    // not the absence of one.
    if let Some(text) = value.as_str()
        && text.eq_ignore_ascii_case("nan")
    {
        return Some(f64::NAN);
    }
    None
}

fn text(object: &Json, key: &str) -> String {
    object
        .find(key)
        .and_then(Json::as_str)
        .map_or_else(String::new, String::from)
}

fn wkt_of(object: &Json) -> String {
    match object.find("coordinateSystem") {
        Some(crs) if crs.is_object() => text(crs, "wkt"),
        _ => String::new(),
    }
}

/// `json.value(key, json::array())`, iterated as the C++'s range-for walks
/// it: an array's elements, an object's values, a primitive itself, nothing
/// for null or a missing member - and nlohmann's type_error 306 for a value
/// that is not an object.
fn members<'a>(json: &'a Json, key: &str) -> JsonResult<Vec<&'a Json>> {
    if !json.is_object() {
        return Err(JsonError::type_error(
            306,
            &format!("cannot use value() with {}", json.type_name()),
        ));
    }
    Ok(json
        .find(key)
        .map(|value| value.iter().collect())
        .unwrap_or_default())
}

/// The statistics GDAL keeps as the band's metadata in its default domain;
/// an empty object when there are none.
fn statistics_of(band: &Json) -> Option<&Json> {
    let metadata = band
        .find("metadata")
        .filter(|metadata| metadata.is_object())?;
    metadata.find("").filter(|domain| domain.is_object())
}

/// `raster info`'s JSON into the description.
fn read_raster(json: &Json, description: &mut SourceDescription) -> JsonResult<()> {
    description.driver = text(json, "driverShortName");
    let mut raster = RasterDescription::default();
    if let Some(size) = json.find("size")
        && size.is_array()
        && size.size() == 2
    {
        raster.width = size[0].get::<i32>()?;
        raster.height = size[1].get::<i32>()?;
    }
    if let Some(transform) = json.find("geoTransform")
        && transform.is_array()
        && transform.size() == 6
    {
        let mut gt = [0.0; 6];
        for (i, value) in gt.iter_mut().enumerate() {
            *value = transform[i].get::<f64>()?;
        }
        raster.georeferenced = true;
        // The lengths of the column and row vectors, so a rotated grid
        // reports the size of its pixels rather than of their shadow on the
        // x axis.
        raster.pixel_width = gt[1].hypot(gt[4]);
        raster.pixel_height = gt[2].hypot(gt[5]);
        let corner = |px: f64, py: f64| {
            Point2::new(
                gt[0] + px * gt[1] + py * gt[2],
                gt[3] + px * gt[4] + py * gt[5],
            )
        };
        let w = f64::from(raster.width);
        let h = f64::from(raster.height);
        for point in [
            corner(0.0, 0.0),
            corner(w, 0.0),
            corner(0.0, h),
            corner(w, h),
        ] {
            raster.bounds.expand(point);
        }
    }
    for each in members(json, "bands")? {
        let mut band = BandDescription {
            band: each.value::<i32>("band", raster.bands.len() as i32 + 1)?,
            data_type: text(each, "type"),
            colour: text(each, "colorInterpretation"),
            ..BandDescription::default()
        };
        if let Some(no_data) = each.find("noDataValue") {
            band.no_data = number(no_data);
        }
        // GDAL's JSON prints a band's minimum, maximum, mean and deviation
        // to three decimals; the STATISTICS_ metadata beside them carries 14
        // significant digits (measured: "mean":30.341 against STATISTICS_MEAN
        // 30.341263614231 for the sample terrain). The metadata first, the
        // rounded keys when a driver gives only those.
        let statistics = statistics_of(each);
        let statistic = |key: &str, metadata_key: &str| {
            if let Some(exact) = statistics
                .and_then(|statistics| statistics.find(metadata_key))
                .and_then(Json::as_str)
                && let Some(value) = parse_finite_double(exact)
            {
                return Some(value);
            }
            each.find(key).and_then(number)
        };
        band.min = statistic("minimum", "STATISTICS_MINIMUM");
        band.max = statistic("maximum", "STATISTICS_MAXIMUM");
        band.mean = statistic("mean", "STATISTICS_MEAN");
        band.std_dev = statistic("stdDev", "STATISTICS_STDDEV");
        for overview in members(each, "overviews")? {
            if let Some(size) = overview.find("size")
                && size.is_array()
                && size.size() == 2
            {
                band.overviews
                    .push((size[0].get::<i32>()?, size[1].get::<i32>()?));
            }
        }
        raster.bands.push(band);
    }
    raster.band_count = raster.bands.len() as i32;
    raster.no_data_value = raster.bands.first().and_then(|band| band.no_data);
    // A container's datasets, SUBDATASET_<n>_NAME and _DESC in the
    // SUBDATASETS metadata domain, in n's order.
    if let Some(metadata) = json.find("metadata")
        && metadata.is_object()
        && let Some(domain) = metadata.find("SUBDATASETS")
        && domain.is_object()
    {
        for n in 1.. {
            let key = format!("SUBDATASET_{n}_");
            let name = text(domain, &format!("{key}NAME"));
            if name.is_empty() {
                break;
            }
            description.subdatasets.push(SubdatasetDescription {
                name,
                description: text(domain, &format!("{key}DESC")),
            });
        }
    }
    description.projection_wkt = wkt_of(json);
    description.raster = Some(raster);
    Ok(())
}

/// `vector info`'s JSON into the description.
fn read_vector(json: &Json, description: &mut SourceDescription) -> JsonResult<()> {
    if description.driver.is_empty() {
        description.driver = text(json, "driverShortName");
    }
    let mut wkts: Vec<String> = Vec::new();
    for each in members(json, "layers")? {
        let mut layer = VectorLayerDescription {
            name: text(each, "name"),
            ..VectorLayerDescription::default()
        };
        if let Some(count) = each.find("featureCount")
            && count.is_number_integer()
            && count.get::<i64>()? >= 0
        {
            layer.feature_count = count.get::<u64>()?;
        }
        let mut wkt = String::new();
        let geometries = members(each, "geometryFields")?;
        if let Some(geometry) = geometries.first() {
            layer.geometry_type = text(geometry, "type");
            wkt = wkt_of(geometry);
            if let Some(extent) = geometry.find("extent")
                && extent.is_array()
                && extent.size() >= 4
            {
                layer.extent.expand(Point2::new(
                    extent[0].get::<f64>()?,
                    extent[1].get::<f64>()?,
                ));
                layer.extent.expand(Point2::new(
                    extent[2].get::<f64>()?,
                    extent[3].get::<f64>()?,
                ));
            }
        }
        layer.crs = describe_crs(&wkt);
        wkts.push(wkt);
        for field in members(each, "fields")? {
            layer.fields.push(FieldDescription {
                name: text(field, "name"),
                r#type: text(field, "type"),
                subtype: text(field, "subType"),
                width: field.value::<i32>("width", 0)?,
                precision: field.value::<i32>("precision", 0)?,
            });
        }
        description.vector_layers.push(layer);
    }
    // A vector dataset has no CRS of its own; its layers each have one. When
    // a raster did not already give one, the file's CRS is the one every
    // layer that declares a CRS agrees on. Layers that disagree leave it
    // empty, and format_description says they differ rather than that there
    // is none.
    if description.projection_wkt.is_empty() {
        let mut common = String::new();
        let mut agree = true;
        for wkt in &wkts {
            if wkt.is_empty() {
                continue;
            }
            if common.is_empty() {
                common.clone_from(wkt);
            } else if *wkt != common {
                agree = false;
            }
        }
        if agree {
            description.projection_wkt = common;
        }
    }
    Ok(())
}

/// The multidimensional formats GDAL has here (`gdalinfo --formats`, the
/// "multidimensional raster" ones that are files): `mdim info` is asked of
/// these only, since it fails for every classic raster.
fn multidimensional(driver: &str) -> bool {
    [
        "netCDF", "HDF4", "HDF5", "GRIB", "BAG", "S102", "S104", "S111", "Zarr", "CPHD",
    ]
    .contains(&driver)
}

/// Both readings into the description; nlohmann's exception, which the C++
/// catches around the two, as the Err.
fn read_descriptions(
    raster: Option<String>,
    vector: Option<String>,
    description: &mut SourceDescription,
) -> JsonResult<()> {
    if let Some(text) = raster {
        read_raster(&Json::parse(&text)?, description)?;
        description.raster_json = text;
    }
    if let Some(text) = vector {
        read_vector(&Json::parse(&text)?, description)?;
        description.vector_json = text;
    }
    Ok(())
}

/// A GDAL dataset described from GDAL's own info JSON: `raster info` and
/// `vector info` are both asked, since one file can hold both (a GeoPackage),
/// and what neither can read is refused with the reader's error its kind
/// suggests.
fn describe_gdal_source(
    path: &Path,
    mut description: SourceDescription,
    options: &DescribeOptions,
) -> Result<SourceDescription> {
    let name = gdal_name(path);
    let mut raster_tokens = vec![String::from("--format=json")];
    if options.statistics {
        raster_tokens.push(String::from("--stats"));
    }
    let raster = gdal_info(&["raster", "info"], &name, raster_tokens, "");
    let vector = gdal_info(
        &["vector", "info"],
        &name,
        vec![String::from("--format=json")],
        &options.layer,
    );
    let (raster, vector) = match (raster, vector) {
        (Err(raster), Err(vector)) => {
            // A layer asked for that is not there: the vector reader's words.
            let vector_first = !options.layer.is_empty() || description.kind == SourceKind::Vector;
            return Err(if vector_first { vector } else { raster });
        }
        (raster, vector) => (raster.ok(), vector.ok()),
    };
    if let Err(error) = read_descriptions(raster, vector, &mut description) {
        return Err(make_error(
            ErrorCode::CommandRejected,
            format!("GDAL's description could not be read: {}", error.what),
            name,
        ));
    }
    if options.multidim && multidimensional(&description.driver) {
        // A format that can be multidimensional need not be (a netCDF of one
        // plain grid opens as either): no multidimensional reading is not a
        // failure.
        if let Ok(multidim) = gdal_info(&["mdim", "info"], &name, Vec::new(), "") {
            description.multidim_json = multidim;
        }
    }
    description.crs = describe_crs(&description.projection_wkt);
    Ok(description)
}

fn describe_point_cloud(
    path: &Path,
    mut description: SourceDescription,
) -> Result<SourceDescription> {
    let engine = PointCloudEngine;
    let header = engine.read_header(path)?;
    let copc = engine.is_copc(path)?;
    description.point_cloud = Some(PointCloudDescription {
        point_count: header.point_count,
        bounds: header.bounds,
        has_color: header.has_color,
        copc,
    });
    description.driver = if copc {
        String::from("readers.copc")
    } else {
        pdal_reader_for(path)
    };
    description.crs = describe_crs(&header.projection_wkt);
    description.projection_wkt = header.projection_wkt;
    Ok(description)
}

// ---- text ---------------------------------------------------------------------------------------

/// 1234567 -> "1,234,567". By hand rather than through a locale: the
/// grouping character must not depend on where the program happens to run,
/// any more than the decimal point may (katana_core::text).
fn grouped(value: u64) -> String {
    let digits = format!("{value}");
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// Three decimals, a millimetre in metres: the C++'s std::to_chars fixed 3,
/// whose 64-character buffer only a magnitude beyond 10^60 overflows, which
/// no coordinate is - then the exact text instead.
fn millimetres(value: f64) -> String {
    if !value.is_finite() {
        // std::to_chars's "inf", "-inf", "nan".
        return katana_core::text::format_fixed(value, 3);
    }
    let text = format!("{value:.3}");
    if text.len() > 64 {
        return format_exact_real(value);
    }
    text
}

fn coordinate_pair(x: f64, y: f64) -> String {
    format!("({}, {})", millimetres(x), millimetres(y))
}

fn plural(count: u64, one: &str, many: &str) -> String {
    format!("{} {}", grouped(count), if count == 1 { one } else { many })
}

/// Whether `path` names something GDAL opens rather than a file on this
/// disk: a /vsi path (/vsizip/, /vsicurl/ ...) or a URL.
pub fn is_virtual_path(path: &Path) -> bool {
    let name = gdal_name(path);
    name.starts_with("/vsi") || name.contains("://")
}

/// NotFound for a missing file (a /vsi path or a URL is GDAL's to find);
/// Unsupported for an extension no importer claims (unless
/// DescribeOptions::any_format), and for an archive, which is described by
/// importing it (its header says nothing its contents do not);
/// FileImportFailure when GDAL or PDAL cannot read it.
pub fn describe_source(path: &Path, options: &DescribeOptions) -> Result<SourceDescription> {
    // A /vsi path or a URL is GDAL's to find: refused here, /vsizip/a.zip/b
    // was "file does not exist" before GDAL was asked.
    if !is_virtual_path(path) && !path_exists(path).unwrap_or(false) {
        return Err(make_error(
            ErrorCode::NotFound,
            "file does not exist",
            gdal_name(path),
        ));
    }

    let description = SourceDescription {
        path: path.to_path_buf(),
        kind: kind_for_path(path),
        ..SourceDescription::default()
    };
    match description.kind {
        SourceKind::PointCloud => return describe_point_cloud(path, description),
        SourceKind::Archive12d => {
            // A refusal that is shown and never stored nor read back, so it
            // says "an archive" without the vendor's name (CLAUDE.md section
            // 9); the C++ line changes with it.
            return Err(make_error(
                ErrorCode::Unsupported,
                "an archive is described by importing it: its header says nothing its contents \
                 do not",
                gdal_name(path),
            ));
        }
        // A /vsi path or a URL is named by what GDAL opens, not by an
        // extension (a URL's may end in a query): GDAL is asked, and so it is
        // when the caller wants whatever GDAL reads. Otherwise a file whose
        // extension no importer claims is refused, as it always was.
        SourceKind::Unknown if !is_virtual_path(path) && !options.any_format => {
            return Err(make_error(
                ErrorCode::Unsupported,
                format!(
                    "no importer reads files named '{}'",
                    extension_with_dot(path)
                ),
                gdal_name(path),
            ));
        }
        SourceKind::Vector | SourceKind::Raster | SourceKind::Unknown => {}
    }
    describe_gdal_source(path, description, options)
}

/// A multi-line summary for a person, in a fixed order: kind and driver, the
/// CRS (or that there is none), then the raster, the layers or the cloud.
/// Coordinates are printed to 3 decimals - the millimetre, the resolution
/// survey coordinates are quoted to - and counts with thousands separators.
pub fn format_description(description: &SourceDescription) -> String {
    let mut text = String::new();
    let mut line = |content: &str| {
        text.push_str(content);
        text.push('\n');
    };

    line(
        &description
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
    );
    line(&format!(
        "Kind: {}{}",
        to_string(description.kind),
        if description.driver.is_empty() {
            String::new()
        } else {
            format!(" ({})", description.driver)
        }
    ));

    if !description.crs.is_empty() {
        line(&format!("Coordinate system: {}", description.crs));
    } else if description
        .vector_layers
        .iter()
        .any(|layer| !layer.crs.is_empty())
    {
        line("Coordinate system: differs between layers (see each layer)");
    } else {
        line("Coordinate system: no coordinate system declared");
    }

    if let Some(raster) = &description.raster {
        line(&format!(
            "Raster: {} x {} pixels, {}",
            grouped(raster.width.max(0) as u64),
            grouped(raster.height.max(0) as u64),
            plural(raster.band_count.max(0) as u64, "band", "bands")
        ));
        if raster.georeferenced {
            // Exact rather than to the millimetre: a geographic raster's
            // pixel is a few ten-thousandths of a degree, which three
            // decimals would print as nothing.
            line(&format!(
                "Pixel size: {} x {}",
                format_exact_real(raster.pixel_width),
                format_exact_real(raster.pixel_height)
            ));
            if !raster.bounds.empty() {
                line(&format!(
                    "Bounds: {} to {}",
                    coordinate_pair(raster.bounds.min.x, raster.bounds.min.y),
                    coordinate_pair(raster.bounds.max.x, raster.bounds.max.y)
                ));
            }
        } else {
            line("Georeferencing: not georeferenced - its pixels have no ground position");
        }
        // Exact: a no-data value is a sentinel compared for equality, so a
        // rounded one would be a different value.
        line(&format!(
            "No-data value: {}",
            match raster.no_data_value {
                Some(value) => format_exact_real(value),
                None => String::from("none declared"),
            }
        ));
    }

    if !description.vector_layers.is_empty() {
        line(&format!(
            "Vector layers: {}",
            grouped(description.vector_layers.len() as u64)
        ));
        for layer in &description.vector_layers {
            line(&format!(
                "  {}: {}, {}, {}",
                layer.name,
                plural(layer.feature_count, "feature", "features"),
                if layer.geometry_type.is_empty() {
                    "unknown geometry"
                } else {
                    layer.geometry_type.as_str()
                },
                if layer.crs.is_empty() {
                    "no coordinate system declared"
                } else {
                    layer.crs.as_str()
                }
            ));
        }
    }

    if let Some(cloud) = &description.point_cloud {
        line(&format!(
            "Point cloud: {}",
            plural(cloud.point_count, "point", "points")
        ));
        if !cloud.bounds.empty() {
            line(&format!(
                "Bounds: x {} to {}, y {} to {}, z {} to {}",
                millimetres(cloud.bounds.min_x),
                millimetres(cloud.bounds.max_x),
                millimetres(cloud.bounds.min_y),
                millimetres(cloud.bounds.max_y),
                millimetres(cloud.bounds.min_z),
                millimetres(cloud.bounds.max_z)
            ));
        } else {
            line("Bounds: none declared");
        }
        line(&format!(
            "Colour: {}",
            if cloud.has_color { "yes" } else { "no" }
        ));
        line(&format!(
            "COPC: {}",
            if cloud.copc {
                "yes - can be read by resolution"
            } else {
                "no - read by decimation"
            }
        ));
    }
    text
}
