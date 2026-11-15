//! Import: external files to the internal domain model
//! (include/katana/interop/import.hpp, src/katana_interop/import.cpp).
//!
//! Nothing here writes to a Model. Vector import produces plain Entity
//! values and the caller wraps them in commands::create_entities, so an
//! import is one validated, atomic, undoable command like every other change.
//! Raster and point cloud import produce reference data, which is not
//! undoable by design - see reference_data.rs for why.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use katana_core::error::make_error;
use katana_core::path_text::path_exists;
use katana_core::text::{format_exact_real, format_fixed, parse_finite_double, trimmed};
use katana_core::{ErrorCode, Result};
use katana_entity::entity::{
    ELEVATION_PROPERTY, ELEVATIONS_PROPERTY, property_to_string, set_heights,
};
use katana_entity::entity_geometry::bounding_box;
use katana_entity::{Entity, EnumName, PropertyMap, PropertyValue};
use katana_geometry::{Box2, Vec2};
use katana_gis::formats::{
    DataKind, OptionList, check_options, formats, identify_content, is_remote_path,
    is_virtual_path, readable_extensions,
};
use katana_gis::gdal_adapter::{
    GdalDataset, VectorLayerInfo, VectorReadOptions, VectorReadReport, describe_crs, same_crs,
};
use katana_gis::processing::{
    ArgValue, DatasetValue, Feature, FeatureSet, FeatureTable, FieldValue, OutputTo, RunRequest,
    run,
};
use katana_gis::reproject::{CrsBox, crs_to_wkt, transform_box};
use katana_math::cpp;
use katana_pointcloud::{PointCloudBounds, PointCloudEngine, PointCloudReadOptions};

/// The C++ `toString` overload set: `to_string(SourceKind::Vector)`.
pub use katana_entity::entity::to_string;

use crate::archive12d::archive12d_extensions;
use crate::dataset_info::{DescribeOptions, SubdatasetDescription, describe_source};
use crate::geo::drawing_dataset::{
    FeaturePieceCounts, FeaturePieceOptions, IMPORT_TYPE_PREFIX, SHIFT_KEY, date_type_tag,
    feature_pieces,
};
use crate::reference_data::{PointCloudLayer, RasterOverlay};
use crate::table_reprojection::reproject_table;

/// Archive12d is an archive file (.12da or .12daz): not a vector file, since
/// one can carry surfaces, alignments and point clouds too. See
/// archive12d.rs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SourceKind {
    #[default]
    Unknown,
    Vector,
    Raster,
    PointCloud,
    Archive12d,
}

/// The C++ `toString(SourceKind)`. A word that is shown (dataset_info's
/// format_description) and never stored nor read back, so the archive's is
/// "archive" without the vendor's name (CLAUDE.md section 9); the C++ line
/// changes with it.
impl EnumName for SourceKind {
    fn name(self) -> &'static str {
        match self {
            SourceKind::Vector => "vector",
            SourceKind::Raster => "raster",
            SourceKind::PointCloud => "point cloud",
            SourceKind::Archive12d => "archive",
            SourceKind::Unknown => "unknown",
        }
    }
}

fn lower_extension(path: &Path) -> String {
    // ASCII only, whatever the locale.
    path.extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// `path.string()`, for an error's context.
fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// `path.filename().string()`.
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `path.stem().string()`.
fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The extensions GDAL reads data of one kind from, less those another
/// importer takes by its extension first (kind_for_path): PDAL's point clouds
/// - GDAL claims .e57 for the images inside one - and the .12da archives.
fn gdal_extensions(kind: DataKind) -> Vec<String> {
    let clouds = point_cloud_extensions();
    let archives = archive12d_extensions();
    readable_extensions(kind)
        .into_iter()
        .filter(|extension| !clouds.contains(extension) && !archives.contains(extension))
        .collect()
}

/// Where a name alone must decide - a URL, a file not written yet - an
/// extension only raster readers claim is a raster, one only vector readers
/// claim is vector data. One claimed by both is vector data (a GeoPackage's
/// features are drawing data, its tiles a backdrop), except for the formats
/// that are chiefly imagery or grids.
fn kind_by_extension(extension: &str) -> SourceKind {
    if extension.is_empty() {
        return SourceKind::Unknown;
    }
    const CHIEFLY_RASTER: [&str; 8] = ["mbtiles", "pdf", "jp2", "j2k", "nc", "pix", "fits", "bag"];
    let mut raster = false;
    let mut vector = false;
    for format in formats() {
        if format
            .extensions
            .iter()
            .any(|each| each.as_str() == extension)
        {
            raster = raster || format.read_raster;
            vector = vector || format.read_vector;
        }
    }
    if raster && vector {
        return if CHIEFLY_RASTER.contains(&extension) {
            SourceKind::Raster
        } else {
            SourceKind::Vector
        };
    }
    if raster {
        return SourceKind::Raster;
    }
    if vector {
        SourceKind::Vector
    } else {
        SourceKind::Unknown
    }
}

/// Converts a source layer name into something usable as a Katana layer name.
/// Keeps it recognisable rather than mangling it: only characters that would
/// be confusing in a layer list are replaced.
fn sanitize_layer_name(name: String) -> String {
    // The ASCII control characters, tab and newlines among them, and DEL.
    // Tested by value rather than by a class test, whose answer for a byte
    // above 0x7F depends on the process locale in the C++. Each is one byte
    // of UTF-8, so a character is a byte here.
    let name: String = name
        .chars()
        .map(|character| {
            let value = u32::from(character);
            if value < 0x20 || value == 0x7F {
                ' '
            } else {
                character
            }
        })
        .collect();
    let trimmed = name.trim_matches(' ');
    if trimmed.is_empty() {
        return String::from("IMPORT");
    }
    String::from(trimmed)
}

/// A field's value as a property: absent is no property, not an empty one.
fn property_of(value: &FieldValue) -> Option<PropertyValue> {
    match value {
        FieldValue::Null => None,
        FieldValue::Boolean(flag) => Some(PropertyValue::Boolean(*flag)),
        FieldValue::Integer64(integer) => Some(PropertyValue::Integer(*integer)),
        FieldValue::Real(real) => Some(PropertyValue::Real(*real)),
        FieldValue::Text(text) => Some(PropertyValue::Text(text.clone())),
    }
}

/// The box around a feature's points, in the file's own coordinates: what
/// the online import's area is tested against.
fn box_of(feature: &Feature) -> CrsBox {
    let mut found = CrsBox {
        min_x: f64::INFINITY,
        min_y: f64::INFINITY,
        max_x: f64::NEG_INFINITY,
        max_y: f64::NEG_INFINITY,
    };
    for part in &feature.parts {
        for ring in &part.parts {
            for point in ring {
                found.min_x = cpp::min(found.min_x, point.x);
                found.min_y = cpp::min(found.min_y, point.y);
                found.max_x = cpp::max(found.max_x, point.x);
                found.max_y = cpp::max(found.max_y, point.y);
            }
        }
    }
    found
}

/// "1 TIN", "3 polyhedral surfaces": what a read left out, for a warning.
fn skipped_geometry(what: &str, count: u64) -> String {
    if what == "tin" {
        return format!("{count}{}", if count == 1 { " TIN" } else { " TINs" });
    }
    format!(
        "{count} {what}{}",
        if count == 1 { "" } else { " geometries" }
    )
}

fn listed_ignoring_case(values: &[String], value: &str) -> bool {
    values.iter().any(|each| each.eq_ignore_ascii_case(value))
}

/// std::map::emplace: the value goes in only when the key is not there.
fn emplace(map: &mut PropertyMap, key: impl Into<String>, value: PropertyValue) {
    map.entry(key.into()).or_insert(value);
}

/// The scope's box, given in the drawing's coordinates, in a layer's own:
/// the same box when the import keeps the file's coordinates, else moved
/// from the target CRS into the layer's (densified, transform_box), so the
/// box the driver filters by encloses the one asked for.
fn box_in_layer(area: &CrsBox, options: &VectorImportOptions, layer_crs: &str) -> Result<CrsBox> {
    if options.target_crs.is_empty() {
        return Ok(*area);
    }
    let from = if layer_crs.is_empty() {
        options.assumed_source_crs.as_str()
    } else {
        layer_crs
    };
    if from.is_empty() {
        // Refused below, where the move of the features fails the same way.
        return Ok(*area);
    }
    transform_box(area, &options.target_crs, from)
}

/// A table cut by the scope (VectorImportOptions::clip_to_area,
/// clip_shapes), by GDAL's `vector clip` through the one bridge
/// (katana_gis::processing): --bbox for a box, --like for the shapes, which
/// clips to their geometry and not merely their bounds. The shapes are in the
/// drawing's coordinates, which are the table's by now, so they are given its
/// CRS. GDAL's warnings join the import's.
fn clip_table(
    table: FeatureTable,
    options: &VectorImportOptions,
    warnings: &mut Vec<String>,
) -> Result<FeatureTable> {
    if table.features.is_empty() {
        return Ok(table);
    }
    let name = table.name.clone();
    let crs = table.crs_wkt.clone();
    let fields = table.fields.clone();
    let mut values = vec![(
        String::from("input"),
        ArgValue::Dataset(DatasetValue::Features(FeatureSet {
            tables: vec![table],
        })),
    )];
    let mut tokens = Vec::new();
    if let Some(shapes) = &options.clip_shapes {
        let mut like = shapes.clone();
        for each in &mut like.tables {
            each.crs_wkt.clone_from(&crs);
        }
        values.push((
            String::from("like"),
            ArgValue::Dataset(DatasetValue::Features(like)),
        ));
    } else if let Some(area) = &options.area {
        tokens.push(format!(
            "--bbox={},{},{},{}",
            format_exact_real(area.min_x),
            format_exact_real(area.min_y),
            format_exact_real(area.max_x),
            format_exact_real(area.max_y)
        ));
    }
    let request = RunRequest {
        path: vec![String::from("vector"), String::from("clip")],
        values,
        tokens,
        output_to: OutputTo::Memory,
        ..RunRequest::default()
    };
    let outputs = run(&request, None, None)?;
    for diagnostic in &outputs.diagnostics {
        if !diagnostic.failure {
            warnings.push(diagnostic.message.clone());
        }
    }
    let mut clipped = match outputs
        .features
        .and_then(|set| set.tables.into_iter().next())
    {
        Some(first) => first,
        // Nothing inside: the fields are still the layer's.
        None => FeatureTable {
            fields,
            ..FeatureTable::default()
        },
    };
    clipped.name = name;
    clipped.crs_wkt = crs;
    Ok(clipped)
}

/// A source's Z becomes the heights every other part of Katana reads
/// (katana_entity::entity). A feature without one gets no height property at
/// all.
fn apply_heights(entity: &mut Entity, heights: &[Option<f64>], replaced: &mut usize) {
    if !heights.iter().any(Option::is_some) {
        // A 2D feature's "elevation" attribute that is a number IS its
        // height: it is how export_vector writes the heights a 2D layer
        // cannot hold in its geometry (a shapefile mixing entities with and
        // without them), so a round trip keeps them. A typed field arrives as
        // a number already; a format without types (a CSV without its .csvt)
        // gives text, read here. The list form, "elevations", is text anyway.
        let parsed = match entity.properties.get(ELEVATION_PROPERTY) {
            Some(PropertyValue::Text(text)) => parse_finite_double(trimmed(text.as_bytes())),
            _ => None,
        };
        if let Some(z) = parsed {
            entity
                .properties
                .insert(String::from(ELEVATION_PROPERTY), PropertyValue::Real(z));
        }
        return;
    }
    // An ATTRIBUTE named "elevation" is taken by a height here: those two
    // names are what the rest of the application reads a height from.
    if entity.properties.contains_key(ELEVATION_PROPERTY)
        || entity.properties.contains_key(ELEVATIONS_PROPERTY)
    {
        *replaced += 1;
    }
    set_heights(&mut entity.properties, heights);
}

/// The layer's declared CRS replaced by srs=, said when they differ.
fn override_declared(layer: &mut VectorLayerInfo, source_wkt: &str, warnings: &mut Vec<String>) {
    if source_wkt.is_empty() {
        return;
    }
    if same_crs(&layer.projection_wkt, source_wkt) == Some(false) {
        warnings.push(format!(
            "layer '{}' declares {}; srs= says it is in {}, which is what it is read as",
            layer.name,
            describe_crs(&layer.projection_wkt),
            describe_crs(source_wkt)
        ));
    }
    layer.projection_wkt = String::from(source_wkt);
}

/// Which importer a path goes to (docs/interop.md, "Formats"). An archive
/// and a point cloud are known by their extensions (.12da, .las ...).
/// Anything else is routed by what it HOLDS, found by GDAL (identify_content)
/// when the path can be looked at - a local file or folder, a /vsi path: a
/// GeoPackage of raster tiles is a Raster, a .zip of a shapefile Vector, a
/// file with an unknown extension whatever GDAL finds in it. A path that
/// cannot be looked at (not written yet, or a URL, whose look costs a round
/// trip) is routed by its name, from GDAL's registry of extensions.
pub fn kind_for_path(path: &Path) -> SourceKind {
    let extension = lower_extension(path);
    if archive12d_extensions().contains(&extension) {
        return SourceKind::Archive12d;
    }
    // Point cloud first: PDAL and GDAL both claim .e57 (GDAL, the images in
    // one), and a .ply in a survey context is a point cloud far more often
    // than it is anything else.
    if point_cloud_extensions().contains(&extension) {
        return SourceKind::PointCloud;
    }
    // What the file holds, where GDAL can look at it: a GeoPackage of raster
    // tiles is a raster, a .zip of a shapefile vector data, a file whose
    // extension no reader claims whatever GDAL finds in it. A web address is
    // looked at only when its name says nothing: a look costs a round trip.
    let remote = is_remote_path(path);
    let named = kind_by_extension(&extension);
    let local = !remote && (is_virtual_path(path) || path_exists(path).unwrap_or(false));
    if (local || (remote && named == SourceKind::Unknown))
        && let Ok(content) = identify_content(path)
    {
        if content.raster != content.vector {
            return if content.raster {
                SourceKind::Raster
            } else {
                SourceKind::Vector
            };
        }
        if content.raster {
            // Both: the name's leaning, vector data where it has none.
            return if named == SourceKind::Raster {
                SourceKind::Raster
            } else {
                SourceKind::Vector
            };
        }
    }
    // A file not there yet, or one GDAL cannot open: by its name, so the
    // import that follows fails with GDAL's own reason, not "no importer".
    named
}

/// Extensions offered in the open dialog, without the leading dot. The
/// vector and raster ones are those of GDAL's readers (readable_extensions),
/// so they are what this build of GDAL opens; the archives are those whose
/// inside is opened (.zip, .tar, .tgz, .gz).
pub fn vector_extensions() -> Vec<String> {
    gdal_extensions(DataKind::Vector)
}

pub fn raster_extensions() -> Vec<String> {
    gdal_extensions(DataKind::Raster)
}

pub fn archive_extensions() -> Vec<String> {
    ["zip", "tar", "tgz", "gz"].map(String::from).to_vec()
}

pub fn point_cloud_extensions() -> Vec<String> {
    ["las", "laz", "copc", "bpf", "ply", "pcd", "e57"]
        .map(String::from)
        .to_vec()
}

// ---- vector -------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct VectorImportOptions {
    /// Katana layer the imported entities are placed on. Empty means "one
    /// layer per source layer, named after it", which is what a multi-layer
    /// GeoPackage should do.
    pub target_layer: String,
    /// A feature attribute that names the layer each entity belongs on,
    /// matched case-insensitively; used only when `target_layer` is empty.
    ///
    /// A DXF is ONE source layer to GDAL ("entities"), and the CAD layer of
    /// each entity arrives as its `Layer` attribute - so without this a
    /// 50 000-entity drawing lands on a single layer called "entities" and
    /// the layer tree is useless. Katana's own exports write the same
    /// information as `layer`, which the case-insensitive match also picks
    /// up, so a round trip through GeoJSON or GeoPackage keeps its layers
    /// too. A feature without the attribute, or with a blank one, falls back
    /// to the source layer's name. Empty disables the lookup.
    pub layer_attribute: String,
    /// -1 imports every layer in the dataset.
    pub source_layer_index: i32,
    /// 0 imports them all; otherwise at most this many of the file's
    /// features.
    pub max_features: u64,
    /// Copy feature attributes onto the entities as properties, typed as the
    /// file types them: integers, reals, booleans, text, and dates as ISO
    /// 8601 text.
    pub attributes_as_properties: bool,
    /// A curve that is one arc or one circle becomes an Arc or a Circle; any
    /// other curve (a line of several arcs) is chords, none further than this
    /// from the curve, in the file's units - the 1 mm sagitta EXPORT uses.
    pub curve_tolerance: f64,
    /// Subtracted from every coordinate. Survey data often sits at
    /// coordinates where a double has only ~0.1 mm of resolution left;
    /// shifting to a local origin restores precision for downstream editing.
    /// Recorded in the result so the same shift can be undone on export.
    pub origin_shift: Option<Vec2>,
    /// Moves every feature into this CRS (anything crs_to_wkt reads:
    /// "EPSG:7856", a WKT) before it becomes an entity. Empty, the default,
    /// leaves coordinates as the file has them - a file import never
    /// reprojects (docs/interop.md, "No reprojection"). The online import
    /// sets it, because data a web service chose the CRS of is useless until
    /// it is in the project's (docs/gis_online.md).
    pub target_crs: String,
    /// The CRS of a layer that declares none, for target_crs; empty refuses
    /// such a layer rather than guess.
    pub assumed_source_crs: String,
    /// The CRS the file's coordinates are in, whatever its layers declare
    /// (IMPORT's srs=, as ogr2ogr's -s_srs): GDAL declares a GeoJSON without
    /// a crs member WGS 84 (RFC 7946), so a fallback for layers that declare
    /// none never reached one, and MGA coordinates were moved as degrees. A
    /// layer that declares another system is said in a warning.
    pub source_crs: String,
    /// Keeps only features whose bounding box meets this box, given in the
    /// source's own CRS (so before any reprojection): the online import's
    /// area applied to a file that covers the world (Natural Earth).
    pub source_filter: Option<CrsBox>,

    // ---- IMPORT's options (docs/interop.md, "Import options") ----
    /// Only these of the file's layers, by name, ignoring case; empty reads
    /// every layer (or source_layer_index's). NotFound, naming the layers the
    /// file has, for a name it does not. layers=.
    pub layer_names: Vec<String>,
    /// An OGR SQL WHERE clause on every layer read ("kind = 'lot'"), which
    /// the driver applies (VectorReadOptions::attribute_filter). where=.
    pub attribute_filter: String,
    /// A SELECT run on the dataset, whose rows are imported in place of its
    /// layers, as one layer named after the file; `sql_dialect` is ogrsql,
    /// sqlite or empty for the driver's own. Refused with layer_names. sql=.
    pub sql: String,
    pub sql_dialect: String,
    /// Only the features whose box meets this box, given in the DRAWING's
    /// coordinates - target_crs's when it is set, the file's own otherwise -
    /// and handed to the driver as its spatial filter, moved into each
    /// layer's CRS first when the import reprojects. IMPORT's scope.
    pub area: Option<CrsBox>,
    /// Features cut at the edges of `area` (clip with an AREA or a VIEW), or
    /// by the areas of `clip_shapes` (clip with the closed shapes a scope
    /// takes), in the drawing's coordinates, by GDAL's vector clip. A feature
    /// wholly outside goes; one across the edge keeps the part inside.
    pub clip_to_area: bool,
    pub clip_shapes: Option<FeatureSet>,
    /// Only these attributes become properties, matched ignoring case; empty
    /// keeps every one. InvalidArgument naming a field no layer read has.
    /// fields=.
    pub fields: Vec<String>,
    /// The driver's open options, KEY=VALUE, each checked against the list
    /// the driver declares (check_options) before the file is opened with
    /// them. oo=.
    pub open_options: Vec<String>,
}

impl Default for VectorImportOptions {
    fn default() -> Self {
        Self {
            target_layer: String::new(),
            layer_attribute: String::from("layer"),
            source_layer_index: -1,
            max_features: 0,
            attributes_as_properties: true,
            curve_tolerance: 0.001,
            origin_shift: None,
            target_crs: String::new(),
            assumed_source_crs: String::new(),
            source_crs: String::new(),
            source_filter: None,
            layer_names: Vec::new(),
            attribute_filter: String::new(),
            sql: String::new(),
            sql_dialect: String::new(),
            area: None,
            clip_to_area: false,
            clip_shapes: None,
            fields: Vec::new(),
            open_options: Vec::new(),
        }
    }
}

/// Whether imported data sits so far from the drawing it is joining that the
/// two cannot usefully be seen together.
///
/// This is the survey case, not a corner case. A DXF or GeoPackage in a
/// projected CRS carries coordinates like (255440, 7410850); a drawing
/// started from scratch sits near the origin. Merging them silently produces
/// a drawing whose extents span seven million metres, in which the original
/// content is a dot smaller than a pixel - and nothing says so.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlacementAdvice {
    /// True when one of the two would be invisible at a zoom that shows both.
    pub far_apart: bool,
    /// Subtract from every imported coordinate to bring it alongside the
    /// drawing. Zero when they already share a neighbourhood.
    pub suggested_shift: Vec2,
    /// Distance between the two bounding boxes, 0 when they overlap.
    pub separation: f64,
    /// Ready to show. Empty when there is nothing worth saying.
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorImportResult {
    pub entities: Vec<Entity>,
    /// Katana layers the entities reference, in first-use order. The caller
    /// must create any that do not exist yet, BEFORE executing the create
    /// command, or the model will reject entities on an unknown layer.
    pub layers_needed: Vec<String>,

    pub bounds: Box2,
    /// The file's features read (and in the area).
    pub features_read: u64,
    /// The features the layers read hold, before any filter: the M of IMPORT
    /// PREVIEW's "matched N of M" (N is features_read). For sql=, every
    /// layer's.
    pub features_in_file: u64,
    /// Whether a filter, a scope or a statement chose what was read: a
    /// result with no entities is then an answer ("matched nothing"), not a
    /// failure to read the file.
    pub filtered: bool,
    /// Of those, the ones no entity came of.
    pub features_skipped: u64,
    /// Geometries left out, by what they were: "tin", "polyhedral surface",
    /// "unsupported <GDAL's type>".
    pub skipped: BTreeMap<String, u64>,
    pub projection_wkt: String,
    /// Non-fatal problems: unsupported geometry types, empty geometries,
    /// attributes that could not be represented, and GDAL's own warnings.
    /// Never silently dropped.
    pub warnings: Vec<String>,
}

pub fn import_vector(path: &Path, options: &VectorImportOptions) -> Result<VectorImportResult> {
    if !(options.curve_tolerance > 0.0) || !options.curve_tolerance.is_finite() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the curve tolerance must be positive",
            format_exact_real(options.curve_tolerance),
        ));
    }
    if !options.sql.is_empty()
        && (!options.layer_names.is_empty() || options.source_layer_index >= 0)
    {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "sql= reads the rows its statement gives, not layers: give it without layers=",
            options.sql.clone(),
        ));
    }
    let mut dataset = GdalDataset::open(path)?;
    // The driver's open options, checked against its own list before the
    // file is opened with them: GDAL only warns of one it does not know and
    // reads on without it, so a misspelt option looked as if it had worked.
    let driver = dataset.driver_name();
    check_options(&driver, OptionList::Open, &options.open_options)?;
    let mut open_with = options.open_options.clone();
    // A CSV's geometry columns (WKT, or X, Y and Z) are its geometry, not
    // fields as well: kept, each entity would carry its own coordinates as a
    // property that goes stale the moment it is moved. Z is a height when
    // the file says X and Y are coordinates (its .csvt, as EXPORT writes
    // one). After the caller's own, so an oo= of the same key wins: GDAL
    // reads the first of a key.
    if driver == "CSV" {
        open_with.push(String::from("KEEP_GEOM_COLUMNS=NO"));
        open_with.push(String::from("Z_POSSIBLE_NAMES=Z"));
    }
    if !open_with.is_empty() {
        dataset = GdalDataset::open_with_options(path, &open_with)?;
    }
    if !dataset.has_vector() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "file holds no vector layers",
            path_string(path),
        ));
    }

    let layers = dataset.vector_layers()?;
    if layers.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "file holds no vector layers",
            path_string(path),
        ));
    }

    let layer_count = layers.len() as i32;
    if options.source_layer_index >= layer_count {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "vector layer index is out of range",
            format!("requested {} of {layer_count}", options.source_layer_index),
        ));
    }

    let first_layer = if options.source_layer_index < 0 {
        0
    } else {
        options.source_layer_index
    };
    let last_layer = if options.source_layer_index < 0 {
        layer_count - 1
    } else {
        options.source_layer_index
    };

    // The layers read, in the file's order: named (layers=), one by index,
    // or all of them; -1 is the rows of sql=.
    let mut indices: Vec<i32> = Vec::new();
    let whole_file =
        options.source_layer_index < 0 && options.layer_names.is_empty() && options.sql.is_empty();
    if !options.sql.is_empty() {
        indices.push(-1);
    } else if !options.layer_names.is_empty() {
        for name in &options.layer_names {
            let Some(found) = layers
                .iter()
                .position(|info| info.name.eq_ignore_ascii_case(name))
            else {
                let mut names = String::new();
                for info in &layers {
                    if !names.is_empty() {
                        names.push_str(", ");
                    }
                    names.push_str(&info.name);
                }
                return Err(make_error(
                    ErrorCode::NotFound,
                    format!("the file has no layer '{name}'; it has {names}"),
                    path_string(path),
                ));
            };
            let index = found as i32;
            if !indices.contains(&index) {
                indices.push(index);
            }
        }
        indices.sort_unstable();
    } else {
        indices.extend(first_layer..=last_layer);
    }

    let mut result = VectorImportResult {
        filtered: !options.attribute_filter.is_empty()
            || !options.sql.is_empty()
            || options.area.is_some()
            || options.clip_shapes.is_some(),
        ..VectorImportResult::default()
    };
    for &index in &indices {
        if index >= 0 {
            result.features_in_file += layers[index as usize].feature_count;
        } else {
            for info in &layers {
                result.features_in_file += info.feature_count;
            }
        }
    }
    // srs=: what every layer is in, whatever it declares.
    let mut source_wkt = String::new();
    if !options.source_crs.is_empty() {
        source_wkt = crs_to_wkt(&options.source_crs)?;
    }
    // The statement's rows are in whatever CRS its layers are; the first
    // layer's is the one a scope's box is moved into.
    result.projection_wkt = if !source_wkt.is_empty() {
        source_wkt.clone()
    } else {
        let first = if indices[0] < 0 {
            0
        } else {
            indices[0] as usize
        };
        layers[first].projection_wkt.clone()
    };
    if !options.target_crs.is_empty() {
        // Every coordinate below is in the target, so that is what the
        // result declares.
        result.projection_wkt = crs_to_wkt(&options.target_crs)?;
    }

    let source_name = file_name(path);
    let mut remaining = options.max_features;
    let mut height_attributes_replaced: usize = 0;
    let mut counts = FeaturePieceCounts::default();
    let mut skipped_geometries: BTreeMap<String, u64> = BTreeMap::new();
    let piece_options = FeaturePieceOptions {
        origin_shift: options.origin_shift,
        curve_tolerance: options.curve_tolerance,
    };

    // GDAL reads a GPX as five layers, two of which - route_points and
    // track_points - are the vertices of its routes and tracks again, one
    // point each: imported with the rest, every line came with a point on
    // each of its vertices. They are left out of a whole-file import, and
    // said; named by index, either is imported as asked.
    let gpx = driver == "GPX";
    let mut restated: Vec<String> = Vec::new();
    // fields= names each seen in some layer read; one never seen is refused.
    let mut fields_seen: Vec<String> = Vec::new();

    for (k, &index) in indices.iter().enumerate() {
        let mut info = if index >= 0 {
            layers[index as usize].clone()
        } else {
            VectorLayerInfo {
                name: stem(path),
                feature_count: 0,
                projection_wkt: layers[0].projection_wkt.clone(),
                geometry_type: String::new(),
            }
        };
        if gpx && whole_file && (info.name == "route_points" || info.name == "track_points") {
            if info.feature_count > 0 {
                restated.push(info.name.clone());
            }
            continue;
        }

        let target_layer = if options.target_layer.is_empty() {
            sanitize_layer_name(if info.name.is_empty() {
                stem(path)
            } else {
                info.name.clone()
            })
        } else {
            options.target_layer.clone()
        };
        if index >= 0 {
            override_declared(&mut info, &source_wkt, &mut result.warnings);
        }
        if index >= 0
            && options.target_crs.is_empty()
            && !info.projection_wkt.is_empty()
            && !result.projection_wkt.is_empty()
            && info.projection_wkt != result.projection_wkt
        {
            result.warnings.push(format!(
                "layer '{}' declares a different coordinate reference system from the first \
                 layer; coordinates are imported unchanged",
                info.name
            ));
        }

        // The one read (gdal_adapter): typed fields, and curves kept as arcs
        // for feature_pieces to make Katana's arcs and circles of. The
        // filters go to the driver, which may use its index for them.
        let mut read_options = VectorReadOptions {
            max_features: remaining,
            keep_arcs: true,
            attribute_filter: options.attribute_filter.clone(),
            spatial_filter: None,
        };
        if let Some(area) = &options.area {
            let found = box_in_layer(area, options, &info.projection_wkt)?;
            read_options.spatial_filter =
                Some([found.min_x, found.min_y, found.max_x, found.max_y]);
        }
        let mut report = VectorReadReport::default();
        let mut table = if index >= 0 {
            dataset.read_table(index, &read_options, Some(&mut report))?
        } else {
            dataset.read_sql(
                &options.sql,
                &options.sql_dialect,
                &read_options,
                Some(&mut report),
            )?
        };
        if index < 0 {
            // The statement's rows carry the CRS of what it read.
            info.projection_wkt.clone_from(&table.crs_wkt);
            override_declared(&mut info, &source_wkt, &mut result.warnings);
            if options.target_crs.is_empty() {
                result.projection_wkt.clone_from(&info.projection_wkt);
            }
        }
        if !source_wkt.is_empty() {
            table.crs_wkt.clone_from(&source_wkt);
        }
        for (what, count) in &report.skipped {
            if what != "no geometry" {
                *skipped_geometries.entry(what.clone()).or_insert(0) += count;
            }
        }
        result.warnings.append(&mut report.warnings);
        if let Some(area) = &options.source_filter {
            table.features.retain(|feature| {
                let found = box_of(feature);
                !(found.max_x < area.min_x
                    || found.min_x > area.max_x
                    || found.max_y < area.min_y
                    || found.min_y > area.max_y)
            });
        }
        if !options.target_crs.is_empty() && !table.features.is_empty() {
            let source = if info.projection_wkt.is_empty() {
                options.assumed_source_crs.clone()
            } else {
                info.projection_wkt.clone()
            };
            if source.is_empty() {
                return Err(make_error(
                    ErrorCode::InvalidCrs,
                    "the layer declares no coordinate system, so it cannot be moved into the \
                     project's",
                    info.name.clone(),
                ));
            }
            reproject_table(&mut table, &source, &options.target_crs)?;
            table.crs_wkt.clone_from(&result.projection_wkt);
        }
        // Cut at the scope's edges, in the drawing's coordinates - which is
        // why after the move into them.
        if (options.clip_to_area && options.area.is_some()) || options.clip_shapes.is_some() {
            table = clip_table(table, options, &mut result.warnings)?;
        }
        for field in &table.fields {
            if listed_ignoring_case(&options.fields, &field.name)
                && !listed_ignoring_case(&fields_seen, &field.name)
            {
                fields_seen.push(field.name.clone());
            }
        }

        // The field that names each entity's layer: `layer` by default,
        // which is what Katana's own exports write and, matched
        // case-insensitively, DXF's `Layer` too.
        let mut layer_field: Option<usize> = None;
        if options.target_layer.is_empty() && !options.layer_attribute.is_empty() {
            layer_field = table
                .fields
                .iter()
                .position(|field| field.name.eq_ignore_ascii_case(&options.layer_attribute));
        }

        // Numbered from 1, as the C++'s counter bumped at the top of the loop.
        for (number, feature) in (1_u64..).zip(&table.features) {
            result.features_read += 1;

            // The layer THIS feature belongs on: its own layer attribute when
            // it has one and the caller has not said "put everything here".
            let mut entity_layer = target_layer.clone();
            if let Some(field) = layer_field
                && let Some(value) = feature.values.get(field).and_then(property_of)
            {
                let named = property_to_string(&value);
                // sanitize_layer_name turns a blank into "IMPORT"; a blank
                // attribute means "no layer given", not that.
                if named
                    .bytes()
                    .any(|byte| !matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
                {
                    entity_layer = sanitize_layer_name(named);
                }
            }

            let mut properties = PropertyMap::new();
            if options.attributes_as_properties {
                for (field, value) in table.fields.iter().zip(&feature.values) {
                    if !options.fields.is_empty()
                        && !listed_ignoring_case(&options.fields, &field.name)
                    {
                        continue;
                    }
                    if let Some(value) = property_of(value) {
                        emplace(&mut properties, field.name.clone(), value);
                    }
                }
            }

            // Which feature a ring came from, so a hole joins its own
            // exterior when the drawing is read as features again
            // (drawing_dataset): the file, the layer, the feature and the
            // part.
            let feature_key = format!("{source_name}|{}#{number}", info.name);
            let mut wrote_any = false;
            for (p, part) in feature.parts.iter().enumerate() {
                let pieces = feature_pieces(part, &piece_options, &mut counts);
                for piece in pieces {
                    let mut entity = Entity {
                        geometry: piece.geometry,
                        layer: entity_layer.clone(),
                        properties: properties.clone(),
                        ..Entity::default()
                    };
                    apply_heights(&mut entity, &piece.heights, &mut height_attributes_replaced);
                    emplace(
                        &mut entity.metadata,
                        "source.file",
                        PropertyValue::Text(source_name.clone()),
                    );
                    emplace(
                        &mut entity.metadata,
                        "source.layer",
                        PropertyValue::Text(info.name.clone()),
                    );
                    if let Some(shift) = options.origin_shift {
                        // Moved from the file's coordinates: no longer in its
                        // coordinate system nor the project's, which EXPORT
                        // and the algorithms must not claim for it
                        // (drawing_dataset).
                        emplace(
                            &mut entity.metadata,
                            SHIFT_KEY,
                            PropertyValue::Text(format!(
                                "{},{}",
                                format_exact_real(shift.x),
                                format_exact_real(shift.y)
                            )),
                        );
                    }
                    for field in &table.fields {
                        if let Some(tag) = date_type_tag(field.r#type)
                            && entity.properties.contains_key(&field.name)
                        {
                            emplace(
                                &mut entity.metadata,
                                format!("{IMPORT_TYPE_PREFIX}{}", field.name),
                                PropertyValue::Text(tag),
                            );
                        }
                    }
                    if let Some(ring) = piece.ring {
                        // The entity model has no polygon-with-holes type, so
                        // each ring is its own closed polyline, and its role
                        // is recorded so the area can be put together again.
                        emplace(
                            &mut entity.metadata,
                            "source.ring",
                            PropertyValue::Text(ring),
                        );
                        emplace(
                            &mut entity.metadata,
                            "source.part",
                            PropertyValue::Text(format!("{feature_key}.{}", p + 1)),
                        );
                    }
                    result.bounds.expand_box(bounding_box(&entity.geometry));
                    if !result.layers_needed.contains(&entity_layer) {
                        result.layers_needed.push(entity_layer.clone());
                    }
                    result.entities.push(entity);
                    wrote_any = true;
                }
            }
            if !wrote_any {
                result.features_skipped += 1;
            }
        }

        if options.max_features != 0 {
            if report.features_read >= remaining {
                if k + 1 < indices.len() {
                    result.warnings.push(String::from(
                        "the feature limit was reached; later layers were not read",
                    ));
                }
                break;
            }
            remaining -= report.features_read;
        }
    }

    for (what, count) in &skipped_geometries {
        *result.skipped.entry(what.clone()).or_insert(0) += count;
    }
    // srs= of a file that declares no CRS, kept where it is: what its
    // coordinates are said to be in (docs/interop.md, "Import options").
    if options.target_crs.is_empty()
        && result.projection_wkt.is_empty()
        && !options.assumed_source_crs.is_empty()
    {
        result.projection_wkt = crs_to_wkt(&options.assumed_source_crs)?;
    }
    for name in &options.fields {
        if !listed_ignoring_case(&fields_seen, name) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("no layer read has the field '{name}' that fields= names"),
                path_string(path),
            ));
        }
    }
    if result.entities.is_empty() && result.filtered {
        // What the filters chose is nothing: an answer, not a failure.
        result.warnings.push(format!(
            "the filters took none of the {} features of the layers read; nothing was imported",
            result.features_in_file
        ));
        return Ok(result);
    }
    if result.entities.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "no geometry in this file could be imported",
            format!(
                "{} features read, {} skipped",
                result.features_read, result.features_skipped
            ),
        ));
    }
    if result.features_skipped > 0 {
        result.warnings.push(format!(
            "{} features were skipped: empty or degenerate geometry, or only geometry Katana has \
             no entity for",
            result.features_skipped
        ));
    }
    for (what, count) in &skipped_geometries {
        result.warnings.push(format!(
            "{} left out: {}",
            skipped_geometry(what, *count),
            if what == "tin" || what == "polyhedral surface" {
                "Katana has no entity that is a surface of faces"
            } else {
                "Katana has no entity for it"
            }
        ));
    }
    if counts.curves_made_chords > 0 {
        result.warnings.push(format!(
            "{} arcs within curves that are not one arc or circle were made chords within {} \
             units: Katana has no entity for a line of several arcs",
            counts.curves_made_chords,
            format_exact_real(options.curve_tolerance)
        ));
    }
    if counts.heights_lost > 0 {
        result.warnings.push(format!(
            "{} vertices were dropped for standing on the vertex before them in plan, and their \
             different heights went with them: a vertical step cannot be drawn in plan",
            counts.heights_lost
        ));
    }
    if height_attributes_replaced > 0 {
        result.warnings.push(format!(
            "{height_attributes_replaced} features had an attribute named \"elevation\" or \
             \"elevations\", which is where Katana keeps a height; the geometry's own Z replaced \
             it"
        ));
    }
    for name in &restated {
        result.warnings.push(format!(
            "GPX layer '{name}' left out: its points are the vertices of the file's {} again, one \
             point each; import that layer by its index for them",
            if name == "route_points" {
                "routes"
            } else {
                "tracks"
            }
        ));
    }
    Ok(result)
}

// ---- raster -------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RasterImportOptions {
    /// Neither dimension of the decimated display copy exceeds this. 4096
    /// keeps a full-screen overlay sharp while bounding memory at 64 MB of
    /// RGBA.
    pub max_pixels: i32,
    /// Empty: the file stem.
    pub name: String,
    /// One band alone, 1-based, as grey (or through its colour table); 0
    /// shows the bands as the file says they make a picture. band=.
    pub band: i32,
    /// A dataset inside a container (a netCDF variable, a GeoPackage raster
    /// table): its number, 1-based, in the order INFO lists them, or its name -
    /// GDAL's whole name, or the part after its last ':'. Empty reads the
    /// file itself. InvalidArgument listing what the file holds for one it
    /// does not. subdataset=.
    pub subdataset: String,
    /// The coordinate system of a raster whose file declares none, recorded
    /// on the reference layer; empty leaves it without. srs=.
    pub assumed_crs: String,
}

impl Default for RasterImportOptions {
    fn default() -> Self {
        Self {
            max_pixels: 4096,
            name: String::new(),
            band: 0,
            subdataset: String::new(),
            assumed_crs: String::new(),
        }
    }
}

pub fn import_raster(path: &Path, options: &RasterImportOptions) -> Result<RasterOverlay> {
    // A dataset inside a container is opened by the name GDAL gives it
    // (SUBDATASET_<n>_NAME), found by its number or its name.
    let mut source = path.to_path_buf();
    if !options.subdataset.is_empty() {
        let describe = DescribeOptions {
            any_format: true,
            ..DescribeOptions::default()
        };
        let description = describe_source(path, &describe)?;
        let held = &description.subdatasets;
        if held.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the file holds no subdatasets; import it without subdataset=",
                path_string(path),
            ));
        }
        let wanted = &options.subdataset;
        let mut found: Option<&SubdatasetDescription> = None;
        if let Some(number) = parse_finite_double(wanted)
            && (1.0..=held.len() as f64).contains(&number)
            && number.floor() == number
        {
            found = Some(&held[number as usize - 1]);
        }
        for each in held {
            if found.is_some() {
                break;
            }
            let name = &each.name;
            let last = match name.rfind(':') {
                Some(colon) => &name[colon + 1..],
                None => name.as_str(),
            };
            if name == wanted || last.eq_ignore_ascii_case(wanted) {
                found = Some(each);
            }
        }
        let Some(found) = found else {
            let mut names = String::new();
            for (i, each) in held.iter().enumerate() {
                if !names.is_empty() {
                    names.push_str(", ");
                }
                names.push_str(&format!("{} {}", i + 1, each.name));
            }
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("the file has no subdataset '{wanted}'; it holds {names}"),
                path_string(path),
            ));
        };
        source = PathBuf::from(&found.name);
    }
    let dataset = GdalDataset::open(&source)?;
    if !dataset.has_raster() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "file holds no raster bands",
            path_string(path),
        ));
    }

    let image = dataset.read_image_band(options.max_pixels, options.band)?;

    let mut overlay = RasterOverlay {
        name: if options.name.is_empty() {
            stem(path)
        } else {
            options.name.clone()
        },
        source,
        width: image.width,
        height: image.height,
        rgba: image.rgba,
        geotransform: image.geotransform,
        has_geotransform: image.has_geotransform,
        projection_wkt: image.projection_wkt,
        ..RasterOverlay::default()
    };
    if overlay.projection_wkt.is_empty() && !options.assumed_crs.is_empty() {
        overlay.projection_wkt = crs_to_wkt(&options.assumed_crs)?;
    }
    Ok(overlay)
}

// ---- point cloud --------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct PointCloudImportOptions {
    /// Points to keep. The importer reads the header first and picks a
    /// decimation step that meets this budget, so opening a billion-point
    /// file costs the same as opening a small one.
    pub budget: u64,
    pub classification: Option<u8>,
    pub clip: Option<PointCloudBounds>,
    /// COPC only: read the octree levels no finer than this point spacing,
    /// in the cloud's units, instead of decimating. The budget still caps
    /// what is kept. Refused with InvalidArgument for a file that is not
    /// COPC, as PointCloudReadOptions::resolution is.
    pub resolution: Option<f64>,
    /// Empty: the file stem.
    pub name: String,
}

impl Default for PointCloudImportOptions {
    fn default() -> Self {
        Self {
            budget: 2_000_000,
            classification: None,
            clip: None,
            resolution: None,
            name: String::new(),
        }
    }
}

pub fn import_point_cloud(
    path: &Path,
    options: &PointCloudImportOptions,
) -> Result<PointCloudLayer> {
    let engine = PointCloudEngine;

    // Read the header first so the decimation step is chosen from the real
    // point count. Opening a 400-million-point file then costs the same as
    // opening a small one.
    let header = engine.read_header(path)?;

    let mut read_options = PointCloudReadOptions {
        classification: options.classification,
        clip: options.clip,
        ..PointCloudReadOptions::default()
    };
    if options.resolution.is_some() {
        // The COPC octree is the level of detail: decimating its answer as
        // well would thin a sample that is already even, by a step sized for
        // the WHOLE file. A plain LAS is refused by the engine, not quietly
        // read whole (PointCloudReadOptions::resolution).
        read_options.resolution = options.resolution;
        read_options.decimation_step = 1;
    } else {
        read_options.decimation_step =
            PointCloudEngine::decimation_for_budget(header.point_count, options.budget);
    }
    // A ceiling as well as a step: a file whose header understates its count
    // (or a classification filter that changes the density, or a resolution
    // finer than the budget allows) must still not blow the budget.
    read_options.max_points = options.budget;

    let mut cloud = engine.read(path, &read_options)?;
    // The step was sized to every point, but the class filter comes before
    // the decimation, so a class that is a fraction of the file came back as
    // that fraction of the budget (738 of 1000 for class 2 of the sample
    // scan). No header counts points by class, and this read measured it:
    // the filter passed at most kept x step points, so a step sized to that
    // keeps within the budget. One more read, only when it would bring back
    // more than the first.
    if options.classification.is_some()
        && options.resolution.is_none()
        && read_options.decimation_step > 1
    {
        // The C++'s unsigned product, which wraps rather than overflows.
        let measured =
            (cloud.points.len() as u64).wrapping_mul(u64::from(read_options.decimation_step));
        let step = PointCloudEngine::decimation_for_budget(measured, options.budget);
        if step < read_options.decimation_step {
            read_options.decimation_step = step;
            cloud = engine.read(path, &read_options)?;
        }
    }
    if cloud.points.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "no points survived the import filters",
            path_string(path),
        ));
    }

    let kept = cloud.points.len() as u64;
    Ok(PointCloudLayer {
        name: if options.name.is_empty() {
            stem(path)
        } else {
            options.name.clone()
        },
        source: path.to_path_buf(),
        points: cloud.points,
        bounds: cloud.bounds,
        projection_wkt: cloud.projection_wkt,
        source_point_count: header.point_count.max(kept),
        decimation_step: read_options.decimation_step,
        ..PointCloudLayer::default()
    })
}

/// `existing` may be empty, which is the common case of importing into a
/// new drawing; the advice is then never "far apart", because there is
/// nothing for the data to be far FROM.
pub fn advise_placement(existing: &Box2, incoming: &Box2) -> PlacementAdvice {
    let mut advice = PlacementAdvice::default();
    if existing.empty() || incoming.empty() {
        return advice; // nothing to be far from
    }

    let diagonal = |found: &Box2| found.width().hypot(found.height());
    let mut combined = *existing;
    combined.expand_box(*incoming);

    let together = diagonal(&combined);
    if !(together > 0.0) || !together.is_finite() {
        return advice;
    }

    // Gap between the boxes, zero where they overlap.
    let gap_x = cpp::max3(
        existing.min.x - incoming.max.x,
        incoming.min.x - existing.max.x,
        0.0,
    );
    let gap_y = cpp::max3(
        existing.min.y - incoming.max.y,
        incoming.min.y - existing.max.y,
        0.0,
    );
    advice.separation = gap_x.hypot(gap_y);

    // The criterion is the symptom, not an arbitrary distance: a part is lost
    // when showing both at once shrinks it below roughly one percent of the
    // view, which is the point at which it stops being visible at all. A
    // distance threshold in metres would be wrong for a site plan and wrong
    // again for a national grid.
    //
    // But the loss must be the PLACEMENT's doing. This used to report any
    // part under 1% of the combined view, which correctly placed data that
    // is merely small always is - a single control point has no extent at
    // all, a 10 m detail inside a 2 km site is 0.5% of it - and the default
    // button then moved it to the drawing's corner (audit IO-02). So a part
    // counts only if, with the gap closed, it would plainly be visible beside
    // the other: its diagonal over VISIBLE_BESIDE of the other's, twice the
    // invisibility line. Being lost at 1% of the combined view then needs a
    // combined view more than twice the larger part's, which only a real gap
    // produces. With the margin at 1x, a part barely over the line at the
    // other's own zoom was "lost" once it overhung it by a hair: a 20 m shed
    // 80 m off the corner of a 2 km site was reported far apart (the corner
    // case in the tests).
    //
    // The larger part always clears the margin, so a drawing hidden by a
    // distant import is reported however small the import is, one far-away
    // point included. A part with no extent is drawn as a marker at any zoom
    // and is never lost by its size.
    const INVISIBLE_FRACTION: f64 = 0.01;
    const VISIBLE_BESIDE: f64 = 2.0 * INVISIBLE_FRACTION;
    let existing_size = diagonal(existing);
    let incoming_size = diagonal(incoming);
    let lost_by_placement = |size: f64, other_size: f64| {
        size > 0.0 && size <= together * INVISIBLE_FRACTION && size > other_size * VISIBLE_BESIDE
    };
    if !lost_by_placement(existing_size, incoming_size)
        && !lost_by_placement(incoming_size, existing_size)
    {
        return advice;
    }

    advice.far_apart = true;
    // Bring the incoming data's lower-left corner to the drawing's, which
    // keeps the imported geometry's own shape and internal coordinates
    // exactly as they were and moves it as one piece.
    advice.suggested_shift = Vec2::new(
        incoming.min.x - existing.min.x,
        incoming.min.y - existing.min.y,
    );

    advice.message = format!(
        "the imported data sits about {} units from the existing drawing, so at a zoom that \
         shows both, one of them is smaller than a pixel",
        format_fixed(advice.separation, 1)
    );
    advice
}
