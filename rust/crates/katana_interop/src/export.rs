//! Export: the internal domain model to external files
//! (include/katana/interop/export.hpp, src/katana_interop/export.cpp).
//!
//! The inverse of import.rs. The model is read, never mutated.

use std::path::Path;

use katana_core::error::make_error;
use katana_core::text::format_exact_real;
use katana_core::{ErrorCode, Result, Status};
use katana_entity::{EntityId, Geometry, Model};
use katana_geometry::Vec2;
use katana_gis::formats::vector_save_choices;
use katana_gis::gdal_adapter::{
    GdalDataset, GeoPoint, GeometryKind, VectorExportOptions as GdalExportOptions, VectorGeometry,
    driver_assumes_wgs84, driver_has_fixed_fields, driver_holds_no_areas,
    driver_holds_one_geometry_type, driver_holds_only_lon_lat,
};
use katana_gis::processing::{Feature, FeatureSet, FeatureTable, FieldDef, FieldType, FieldValue};
use katana_gis::reproject::crs_to_wkt;
use katana_pointcloud::{PointCloud, PointCloudEngine};

use crate::geo::drawing_dataset::{DrawingDatasetOptions, DrawingDatasetStats, drawing_dataset};
use crate::reference_data::PointCloudLayer;
use crate::table_reprojection::reproject_table;

#[derive(Clone, Debug, PartialEq)]
pub struct VectorExportOptions {
    /// Empty exports the whole model; otherwise only these entities.
    pub entities: Vec<EntityId>,
    /// Empty exports every layer; otherwise only entities on these layers.
    pub layers: Vec<String>,

    pub layer_name: String,
    /// Empty: inferred from the path extension.
    pub driver: String,
    /// The drawing's coordinate system (anything GDAL reads: "EPSG:7856",
    /// WKT), written into the file. A format that holds only longitude and
    /// latitude (KML, KMZ, GPX) is written in them, converted from this;
    /// without it such an export is refused with InvalidCRS, since GDAL would
    /// write its placemarks without their geometry and report success.
    pub projection_wkt: String,

    /// Added back to every coordinate, undoing an import's origin_shift.
    pub origin_shift: Option<Vec2>,

    /// Write entity properties as feature attributes.
    pub properties_as_attributes: bool,
    /// Curves have no exact representation in most vector formats, so arcs
    /// and circles are written as polylines. This is the sagitta of the
    /// chords used, in model units: the largest distance the polyline may
    /// deviate from the true curve. Stated explicitly rather than hidden,
    /// because it is a lossy conversion and the user is entitled to choose
    /// its accuracy.
    pub curve_tolerance: f64,

    // ---- EXPORT's options (docs/interop.md, "Export options") ----
    /// The coordinate system the file is written in (anything crs_to_wkt
    /// reads): the coordinates, in projection_wkt's system, are moved into it
    /// on the way out. Empty writes them as they are. InvalidCRS when it is
    /// set and projection_wkt is empty: there is nothing to move from.
    pub target_crs: String,
    /// One file layer per drawing layer, named after it, in the order the
    /// layers are first met (split=layer). Refused for a format of points and
    /// lines only (GPX), whose layers are its own.
    pub split_by_layer: bool,
    /// The layers added to the file when it exists (a GeoPackage), not the
    /// file replaced (the adapter's VectorExportOptions::append).
    pub append: bool,
    /// The driver's own options, KEY=VALUE (co=, lco=). The caller checks
    /// them against the driver's lists (check_options).
    pub creation_options: Vec<String>,
    pub layer_creation_options: Vec<String>,
    /// A text entity written as a point at its insertion point, with fields
    /// text, text_height (model units) and text_rotation (degrees,
    /// anticlockwise) and an OGR_STYLE LABEL that KML, DXF and MapInfo draw
    /// (text=points). Off, text is left out and counted, as it always was.
    pub text_as_points: bool,
}

impl Default for VectorExportOptions {
    fn default() -> Self {
        Self {
            entities: Vec::new(),
            layers: Vec::new(),
            layer_name: String::from("katana"),
            driver: String::new(),
            projection_wkt: String::new(),
            origin_shift: None,
            properties_as_attributes: true,
            curve_tolerance: 0.001,
            target_crs: String::new(),
            split_by_layer: false,
            append: false,
            creation_options: Vec::new(),
            layer_creation_options: Vec::new(),
            text_as_points: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VectorExportResult {
    pub features_written: u64,
    pub entities_skipped: u64,
    /// Text entities written as points (text_as_points).
    pub texts_written: u64,
    /// The file's layers this export wrote, in order.
    pub layers: Vec<String>,
    pub driver: String,
    pub warnings: Vec<String>,
    /// The coordinate system the file's coordinates are in: the project's,
    /// crs=<code>'s, WGS 84 for KML, KMZ and GPX, which hold nothing else, or
    /// none (empty) for entities IMPORT moved from their file's coordinates.
    pub projection_wkt: String,
}

/// Formats offered in the save dialog, as (description, extension) pairs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormatChoice {
    pub description: String,
    /// Without the dot.
    pub extension: String,
}

/// The index of the field `name` in `table`, added as `type` when it has
/// none: every feature already there gets no value for it.
fn field_of(table: &mut FeatureTable, name: &str, r#type: FieldType) -> usize {
    if let Some(found) = table.fields.iter().position(|field| field.name == name) {
        return found;
    }
    table.fields.push(FieldDef {
        name: String::from(name),
        r#type,
    });
    let count = table.fields.len();
    for feature in &mut table.features {
        feature.values.resize(count, FieldValue::Null);
    }
    count - 1
}

/// An OGR style string's text parameter: double-quoted, a quote or a
/// backslash in it escaped (GDAL's OGR Feature Style specification).
fn style_text(text: &str) -> String {
    let mut quoted = String::from("\"");
    for character in text.chars() {
        if character == '"' || character == '\\' {
            quoted.push('\\');
        }
        quoted.push(if character == '\n' { ' ' } else { character });
    }
    quoted.push('"');
    quoted
}

/// The text entities of `ids` as points in `set` (VectorExportOptions::
/// text_as_points): in its one table, or its points table, made when there
/// is none. Each carries its text, height and rotation as fields and as the
/// LABEL of its OGR style. The count written.
fn add_texts(
    model: &Model,
    ids: &[EntityId],
    name: &str,
    one_table: bool,
    crs: &str,
    set: &mut FeatureSet,
) -> usize {
    let mut written = 0;
    for &id in ids {
        let Some(entity) = model.entities.find(id) else {
            continue;
        };
        let Geometry::Text(text) = &entity.geometry else {
            continue;
        };
        let index = match set
            .tables
            .iter()
            .position(|each| one_table || each.kind == GeometryKind::Point)
        {
            Some(index) => index,
            None => {
                set.tables.push(FeatureTable {
                    name: if one_table {
                        String::from(name)
                    } else {
                        String::from("points")
                    },
                    kind: GeometryKind::Point,
                    crs_wkt: String::from(crs),
                    fields: vec![
                        FieldDef {
                            name: String::from("katana_id"),
                            r#type: FieldType::Integer64,
                        },
                        FieldDef {
                            name: String::from("layer"),
                            r#type: FieldType::String,
                        },
                    ],
                    ..FeatureTable::default()
                });
                set.tables.len() - 1
            }
        };
        let table = &mut set.tables[index];
        let had_others = table.features.iter().any(|feature| {
            feature
                .parts
                .first()
                .is_some_and(|part| part.kind != GeometryKind::Point)
        });
        table.kind =
            if had_others || (table.kind != GeometryKind::Point && !table.features.is_empty()) {
                GeometryKind::Unknown
            } else {
                GeometryKind::Point
            };
        let text_field = field_of(table, "text", FieldType::String);
        let height_field = field_of(table, "text_height", FieldType::Real);
        let rotation_field = field_of(table, "text_rotation", FieldType::Real);
        let style_field = field_of(table, "OGR_STYLE", FieldType::String);
        let mut feature = Feature::default();
        feature.parts.push(VectorGeometry {
            kind: GeometryKind::Point,
            parts: vec![vec![GeoPoint {
                x: text.position.x,
                y: text.position.y,
                z: 0.0,
            }]],
            ..VectorGeometry::default()
        });
        feature.values = table
            .fields
            .iter()
            .map(|field| match field.name.as_str() {
                "katana_id" => FieldValue::Integer64(entity.id as i64),
                "layer" => FieldValue::Text(entity.layer.clone()),
                _ => FieldValue::Null,
            })
            .collect();
        // Degrees anticlockwise, as OGR's LABEL angle and every GIS reads
        // one; the entity keeps radians.
        let degrees = text.rotation * 180.0 / std::f64::consts::PI;
        feature.values[text_field] = FieldValue::Text(text.text.clone());
        feature.values[height_field] = FieldValue::Real(text.height);
        feature.values[rotation_field] = FieldValue::Real(degrees);
        feature.values[style_field] = FieldValue::Text(format!(
            "LABEL(t:{},s:{}g,a:{})",
            style_text(&text.text),
            format_exact_real(text.height),
            format_exact_real(degrees)
        ));
        table.features.push(feature);
        written += 1;
    }
    written
}

/// Every vector format this GDAL writes, from its registry, the common ones
/// first; each extension is written by the driver EXPORT picks for it
/// (vector_save_choices, docs/interop.md "Formats").
pub fn vector_export_formats() -> Vec<FormatChoice> {
    // Every writer of vector layers this GDAL has, the common ones first
    // (vector_save_choices): six hand-picked entries used to hide FlatGeobuf,
    // GeoParquet, KMZ, GPX and CSV, which EXPORT could write.
    vector_save_choices()
        .into_iter()
        .map(|choice| FormatChoice {
            description: choice.description,
            extension: choice.extension,
        })
        .collect()
}

/// What a group of entities goes to: one file layer.
struct Group {
    name: String,
    ids: Vec<EntityId>,
}

/// Entity types with no vector-format counterpart (text, dimensions) are
/// skipped and counted, never silently dropped. The entities go through the
/// one conversion to features (geo::drawing_dataset): properties keep their
/// types, a ring IMPORT tagged as a hole is written as its area's hole, and
/// the file is written by GdalDataset::write_tables, which meets each
/// format's needs (docs/interop.md, "Fidelity").
pub fn export_vector(
    model: &Model,
    path: &Path,
    options: &VectorExportOptions,
) -> Result<VectorExportResult> {
    // Fail on an unsupported extension BEFORE walking the model, so a
    // mistyped filename costs nothing and the error names the extension.
    let driver = if options.driver.is_empty() {
        GdalDataset::vector_driver_for_path(path)?
    } else {
        options.driver.clone()
    };

    let mut result = VectorExportResult {
        driver: driver.clone(),
        ..VectorExportResult::default()
    };

    // The entities named, or the whole drawing, on the layers named.
    let filter_by_layer = !options.layers.is_empty();
    let on_layer =
        |layer: &str| !filter_by_layer || options.layers.iter().any(|each| each.as_str() == layer);
    let mut ids: Vec<EntityId> = Vec::new();
    if !options.entities.is_empty() {
        for &id in &options.entities {
            // One the drawing does not hold is counted missing by the
            // conversion.
            match model.entities.find(id) {
                Some(entity) if !on_layer(&entity.layer) => {}
                _ => ids.push(id),
            }
        }
    } else {
        model.entities.for_each(|entity| {
            if on_layer(&entity.layer) {
                ids.push(entity.id);
            }
        });
    }

    // GPX holds a layer of points and one of lines, and no areas: a closed
    // shape goes to it as the closed line around it.
    let no_areas = driver_holds_no_areas(&driver);
    if options.split_by_layer && no_areas {
        return Err(make_error(
            ErrorCode::Unsupported,
            format!(
                "{driver} has layers of its own (waypoints, routes, tracks), not one per drawing \
                 layer"
            ),
            path.to_string_lossy(),
        ));
    }
    if !options.target_crs.is_empty() && options.projection_wkt.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidCrs,
            format!(
                "the drawing is in no coordinate system Katana was told of, so it cannot be moved \
                 into {}: set the project's (CRS SET <code>) first",
                options.target_crs
            ),
            path.to_string_lossy(),
        ));
    }

    // What goes to one file layer: every entity, or those of one drawing
    // layer each (split=layer), in the order the layers are first met.
    let mut groups: Vec<Group> = Vec::new();
    if options.split_by_layer {
        for &id in &ids {
            let layer = model
                .entities
                .find(id)
                .map_or_else(String::new, |entity| entity.layer.clone());
            let at = match groups.iter().position(|group| group.name == layer) {
                Some(at) => at,
                None => {
                    groups.push(Group {
                        name: layer,
                        ids: Vec::new(),
                    });
                    groups.len() - 1
                }
            };
            groups[at].ids.push(id);
        }
    } else {
        groups.push(Group {
            name: options.layer_name.clone(),
            ids,
        });
    }

    // The ONE conversion of entities to features (drawing_dataset): the one
    // EXPORT and every algorithm read the drawing through, so a lot with a
    // hole is written as one polygon with a hole - 9600 m2, not a lot of
    // 10000 and a separate "hole" of 400 - and a property keeps its type.
    let mut conversion = DrawingDatasetOptions {
        curve_tolerance: options.curve_tolerance,
        crs_wkt: options.projection_wkt.clone(),
        style_fields: false,
        properties: options.properties_as_attributes,
        closed_as_polygons: !no_areas,
        one_table: !no_areas,
        ..DrawingDatasetOptions::default()
    };
    let mut stats = DrawingDatasetStats::default();
    let mut set = FeatureSet::default();
    for group in &groups {
        conversion.table_name.clone_from(&group.name);
        let mut dataset = drawing_dataset(model, &group.ids, &conversion)?;
        if options.text_as_points {
            let texts = add_texts(
                model,
                &group.ids,
                &group.name,
                conversion.one_table,
                &conversion.crs_wkt,
                &mut dataset.set,
            );
            result.texts_written += texts as u64;
            let skipped_text = dataset
                .stats
                .skipped
                .entry(String::from("text"))
                .or_insert(0);
            *skipped_text = skipped_text.saturating_sub(texts);
            if *skipped_text == 0 {
                dataset.stats.skipped.remove("text");
            }
        }
        let one = &dataset.stats;
        stats.matched += one.matched;
        stats.used += one.used;
        stats.points += one.points;
        stats.lines += one.lines;
        stats.polygons += one.polygons;
        for (reason, count) in &one.skipped {
            *stats.skipped.entry(reason.clone()).or_insert(0) += count;
        }
        stats.warnings.extend(one.warnings.iter().cloned());
        stats.shifted += one.shifted;
        set.tables.append(&mut dataset.set.tables);
    }
    let skipped_as = |reason: &str| stats.skipped.get(reason).copied().unwrap_or(0);
    let text_skipped = skipped_as("text");
    let dimension_skipped = skipped_as("dimension");
    let annotation_skipped = skipped_as("label") + skipped_as("leader");
    for count in stats.skipped.values() {
        result.entities_skipped += *count as u64;
    }

    let mut feature_count: usize = 0;
    let mut points: usize = 0;
    let mut lines: usize = 0;
    let mut polygons: usize = 0;
    for table in &set.tables {
        for feature in &table.features {
            feature_count += 1;
            let kind = feature
                .parts
                .first()
                .map_or(GeometryKind::Unknown, |part| part.kind);
            points += usize::from(kind == GeometryKind::Point);
            lines += usize::from(kind == GeometryKind::LineString);
            polygons += usize::from(kind == GeometryKind::Polygon);
        }
    }
    if feature_count == 0 {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "nothing to export: no entity matched, or none has a geometry this format can \
             represent",
            format!("{} skipped", result.entities_skipped),
        ));
    }

    // Refuse a mixed collection for a single-type format BEFORE writing
    // anything. GDAL would otherwise accept the first feature, fix the
    // layer's shape type from it, and fail on the first feature of another
    // kind - after a partial file exists.
    if driver_holds_one_geometry_type(&driver)
        && usize::from(points != 0) + usize::from(lines != 0) + usize::from(polygons != 0) > 1
    {
        return Err(make_error(
            ErrorCode::Unsupported,
            format!("{driver} stores one geometry type per file, but this selection mixes them"),
            format!(
                "{points} points, {lines} lines, {polygons} polygons. Export to GeoPackage or \
                 GeoJSON, which hold all three, or select one kind at a time."
            ),
        ));
    }

    if let Some(shift) = options.origin_shift {
        // Added back: the file gets the coordinates the import shifted.
        for table in &mut set.tables {
            for feature in &mut table.features {
                for part in &mut feature.parts {
                    for ring in &mut part.parts {
                        for point in ring {
                            point.x += shift.x;
                            point.y += shift.y;
                        }
                    }
                }
            }
        }
    }

    // Heights written into a geometry are dropped from its attributes, where
    // they would only repeat it. The exception is a format whose layer is
    // either 2D or 3D (a shapefile): a 3D layer has no way to say "no
    // height", so the drawing's heightless entities would be written at 0.
    // When some entities have heights and some do not, such a layer is
    // written in plan and every height stays an attribute - reported, not
    // decided silently.
    let mut with_heights: usize = 0;
    for table in &set.tables {
        for feature in &table.features {
            with_heights += usize::from(feature.parts.first().is_some_and(|part| part.has_z));
        }
    }
    let in_plan =
        with_heights > 0 && with_heights < feature_count && driver_holds_one_geometry_type(&driver);
    for table in &mut set.tables {
        if in_plan {
            table.has_z = false;
        }
        let height_fields: Vec<usize> = table
            .fields
            .iter()
            .enumerate()
            .filter(|(_, field)| {
                field.name == katana_entity::entity::ELEVATION_PROPERTY
                    || field.name == katana_entity::entity::ELEVATIONS_PROPERTY
            })
            .map(|(f, _)| f)
            .collect();
        for feature in &mut table.features {
            if in_plan {
                for part in &mut feature.parts {
                    part.has_z = false;
                }
            }
            let heighted = feature.parts.first().is_some_and(|part| part.has_z);
            for &f in &height_fields {
                if heighted && f < feature.values.len() {
                    feature.values[f] = FieldValue::Null;
                }
            }
        }
        // A height field no feature needs any more is not written at all.
        for &f in height_fields.iter().rev() {
            let used = table.features.iter().any(|feature| {
                f < feature.values.len() && !matches!(feature.values[f], FieldValue::Null)
            });
            if used {
                continue;
            }
            table.fields.remove(f);
            for feature in &mut table.features {
                if f < feature.values.len() {
                    feature.values.remove(f);
                }
            }
        }
    }

    // crs=<code>: moved on the way out, and the file says the system it is
    // now in. Entities IMPORT moved from their file's coordinates are in no
    // known system: the file says none, and cannot be moved into one.
    let mut written_crs = options.projection_wkt.clone();
    if stats.shifted != 0 {
        if !options.target_crs.is_empty() || driver_holds_only_lon_lat(&driver) {
            return Err(make_error(
                ErrorCode::InvalidCrs,
                format!(
                    "{} entities were imported moved from their file's coordinates (LOCAL, \
                     ALONGSIDE or OFFSET=), so they are in no known coordinate system and cannot \
                     be converted to another",
                    stats.shifted
                ),
                path.to_string_lossy(),
            ));
        }
        written_crs.clear();
    }
    if !options.target_crs.is_empty() {
        let target = crs_to_wkt(&options.target_crs)?;
        for table in &mut set.tables {
            let from = if table.crs_wkt.is_empty() {
                options.projection_wkt.clone()
            } else {
                table.crs_wkt.clone()
            };
            reproject_table(table, &from, &options.target_crs)?;
            table.crs_wkt.clone_from(&target);
        }
        written_crs = target;
    }

    let gdal_options = GdalExportOptions {
        driver: driver.clone(),
        // One layer per drawing layer is named after it, whatever layer_name
        // says.
        layer_name: if options.split_by_layer {
            String::new()
        } else {
            options.layer_name.clone()
        },
        projection_wkt: written_crs.clone(),
        creation_options: options.creation_options.clone(),
        layer_creation_options: options.layer_creation_options.clone(),
        append: options.append,
    };
    for table in &set.tables {
        result.layers.push(
            if set.tables.len() == 1 && !gdal_options.layer_name.is_empty() {
                gdal_options.layer_name.clone()
            } else {
                table.name.clone()
            },
        );
    }

    let written = GdalDataset::write_tables(path, &set, &gdal_options)?;

    result.features_written = written.features_written;
    // What the file's coordinates are in: KML, KMZ and GPX are converted to
    // longitude and latitude by the writer, whatever the drawing is in.
    result.projection_wkt.clone_from(&written_crs);
    if !written_crs.is_empty() && driver_holds_only_lon_lat(&driver) {
        result.projection_wkt = crs_to_wkt("EPSG:4326").unwrap_or(written_crs);
    }
    result.entities_skipped += written.features_skipped;
    if driver_has_fixed_fields(&driver) {
        result.warnings.push(format!(
            "{driver} has a fixed set of fields: every entity keeps its geometry and its layer, \
             but entity ids and properties were not written. Use GeoPackage to keep them."
        ));
    }
    if options.projection_wkt.is_empty() && driver_assumes_wgs84(&driver) {
        // RFC 7946 defines GeoJSON coordinates as WGS 84 longitude/latitude,
        // so readers will label these as lat/lon whatever they really are.
        // Saying so is the difference between a known limitation and a
        // silent lie.
        result.warnings.push(String::from(
            "GeoJSON always declares WGS 84 longitude/latitude; these coordinates were written \
             unchanged and will be read back as degrees. Use GeoPackage to keep a projected \
             coordinate system.",
        ));
    }
    if text_skipped > 0 {
        result.warnings.push(format!(
            "{text_skipped} text entities were skipped: this format has no text geometry"
        ));
    }
    if dimension_skipped > 0 {
        result.warnings.push(format!(
            "{dimension_skipped} dimension entities were skipped: this format has no dimension \
             geometry"
        ));
    }
    if annotation_skipped > 0 {
        result.warnings.push(format!(
            "{annotation_skipped} labels and leaders were skipped: this format has no annotation \
             geometry"
        ));
    }
    if in_plan {
        result.warnings.push(format!(
            "{with_heights} of {feature_count} entities have heights, but a {driver} layer is \
             either all 2D or all 3D and a 3D one cannot say \"no height\": the layer was written \
             in plan and the heights are in the elevation attributes. Export to GeoPackage or \
             GeoJSON to keep them in the geometry."
        ));
    }
    // What the conversion could not carry (a height at only some vertices, a
    // property of several types) and what the writer said (GDAL's warnings,
    // a conversion to longitude and latitude).
    result.warnings.extend(stats.warnings);
    result.warnings.extend(written.warnings);
    Ok(result)
}

pub fn export_point_cloud(cloud: &PointCloudLayer, path: &Path) -> Status {
    if cloud.points.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "point cloud layer holds no points",
            cloud.name.clone(),
        ));
    }

    let out = PointCloud {
        points: cloud.points.clone(),
        bounds: cloud.bounds,
        projection_wkt: cloud.projection_wkt.clone(),
        source_point_count: cloud.source_point_count,
    };

    PointCloudEngine.write(path, &out)
}

/// What export_surface_raster (terrain_io) writes: GeoTIFF, Esri ASCII grid,
/// Erdas Imagine - exactly the extensions GdalDataset::raster_driver_for_path
/// maps, so nothing offered here can then be refused by name.
pub fn raster_export_formats() -> Vec<FormatChoice> {
    [
        ("GeoTIFF", "tif"),
        ("Esri ASCII grid", "asc"),
        ("Erdas Imagine", "img"),
    ]
    .into_iter()
    .map(|(description, extension)| FormatChoice {
        description: String::from(description),
        extension: String::from(extension),
    })
    .collect()
}

/// What export_point_cloud writes: LAS and LAZ. COPC is a conversion of a
/// FILE, not an export of the sample held in memory - see
/// PointCloudEngine::convert_to_copc - so it is not offered here.
pub fn point_cloud_export_formats() -> Vec<FormatChoice> {
    [("LAS", "las"), ("LAZ (compressed LAS)", "laz")]
        .into_iter()
        .map(|(description, extension)| FormatChoice {
            description: String::from(description),
            extension: String::from(extension),
        })
        .collect()
}
