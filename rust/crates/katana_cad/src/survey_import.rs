//! Survey field data into the drawing (PLAN.MD 45.3;
//! include/katana/cad/survey_import.hpp).
//!
//! This is the ONLY place a survey value becomes a CAD entity, and it lives in
//! cad rather than in surveyio deliberately: cad may not see surveyio
//! (tools/check_layering.cmake), so an instrument maker's type cannot reach
//! the drawing even by accident. Everything that arrives here has already been
//! normalised into metres, radians and northing/easting by a parser.
//!
//! The whole import is ONE command. One Ctrl+Z undoes an import of ten
//! thousand points, not the last one of them - which is what PLAN.MD 45 asks
//! for and what applying survey codes already does for the same reason.

use std::collections::{BTreeMap, BTreeSet};

use katana_commands::{CommandPtr, Transaction, create_entities, create_layer};
use katana_core::error::make_error;
use katana_core::{ErrorCode, Result};
use katana_entity::entity::set_heights;
use katana_entity::layer_path::{LAYER_SEPARATOR, join_layer_path, validate_layer_path};
use katana_entity::{Entity, Geometry, Layer, PointGeometry, PropertyValue};
use katana_geometry::Point2;
use katana_survey::data_model::{
    CoordinateSource, SurveyPoint, SurveyProject, to_string, validate_project,
};

use crate::document::Document;

// Keys this import writes that are NOT configurable, because they describe the
// import rather than the survey. They are the brief's source-tracking fields:
// "Source Manufacturer / Source Format / Source File / Source Record", so that
// "where did point 102 come from" is answerable from the drawing alone.
//
// The provenance ones go in Entity::metadata, which the entity reserves for
// exactly this; the coordinate source is a property because it is a fact
// about the survey that a report is entitled to show.
pub const COORDINATE_SOURCE_PROPERTY: &str = "survey.coordinate_source";
pub const SOURCE_MANUFACTURER_META: &str = "survey.manufacturer";
pub const SOURCE_FORMAT_META: &str = "survey.format";
pub const SOURCE_FORMAT_VERSION_META: &str = "survey.format_version";
pub const SOURCE_FILE_META: &str = "survey.file";
pub const SOURCE_RECORD_META: &str = "survey.record";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurveyImportOptions {
    /// Where the points go: a '/'-separated layer path (entity's
    /// layer_path.rs).
    pub layer: String,
    /// A layer per field code beneath `layer`, so that "EP" and "BM" can be
    /// switched on and off independently. That is how a surveyor reads a
    /// drawing and it costs one layer per distinct code; off, everything lands
    /// on `layer`.
    pub layer_per_code: bool,
    /// A layer the drawing does not have is created as part of the same
    /// transaction. Off, points on a missing layer are refused rather than
    /// silently landing on "0" - a point on the wrong layer is worse than an
    /// error, because nothing about it looks wrong later.
    pub create_layers: bool,

    /// Where the point's own fields are written. The defaults are exactly what
    /// the survey coding's code property candidates look for, so applying
    /// survey codes finds the codes afterwards without being told where they
    /// are - importing field data and then applying a survey code file to it
    /// is one workflow, not two.
    pub code_property: String,
    pub point_number_property: String,
    pub description_property: String,

    /// Provenance into Entity::metadata, which the entity documents as the
    /// place for it: the manufacturer, the format and its parser version, the
    /// source file NAME and the record within it. This is what makes "where
    /// did point 102 come from" answerable a year later.
    pub record_source: bool,
}

impl Default for SurveyImportOptions {
    fn default() -> Self {
        Self {
            layer: String::from("survey/points"),
            layer_per_code: false,
            create_layers: true,
            code_property: String::from("code"),
            point_number_property: String::from("point"),
            description_property: String::from("description"),
            record_source: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SurveyImportReport {
    pub points: usize,
    /// Points the source gave no height for. They are imported, WITHOUT the
    /// elevation property, so a surface built from the drawing leaves them out
    /// rather than pulling itself down to the datum under each one; a warning
    /// says how many.
    pub points_without_elevation: usize,
    /// Points the source names with no coordinates at all (the targets of raw
    /// observations, the survey's UnpositionedPoint). Not drawn - they have
    /// nowhere to be drawn - and a warning says how many and why.
    pub points_without_position: usize,
    /// In name order.
    pub layers_created: Vec<String>,
    /// Things worth telling the user that are not failures - a metadata field
    /// that could not be written because the import already uses that
    /// property, for instance. There is deliberately no "skipped" count: every
    /// point in a project that passed the survey's validate_project is
    /// importable, so a point that vanished would be a bug and not a
    /// statistic.
    pub warnings: Vec<String>,
}

/// A field code is user data; a layer name is a PATH. So a code carrying a
/// separator would silently build a subtree - "KERB/TOP" would put its points
/// on a layer "TOP" underneath "KERB", which nobody asked for and which is
/// invisible until someone wonders where their points went. Every character a
/// layer path gives meaning to is replaced with '_'.
///
/// A code made of nothing but those characters becomes "unnamed" rather than
/// an empty segment, because an empty segment is not a valid layer path at
/// all.
fn sanitized_code(code: &str) -> String {
    let replaced: String = code
        .chars()
        .map(|c| if c == LAYER_SEPARATOR { '_' } else { c })
        .collect();
    // Leading and trailing spaces would make two codes that read the same
    // produce two layers.
    let trimmed = replaced.trim_matches([' ', '\t']);
    if trimmed.is_empty() {
        String::from("unnamed")
    } else {
        String::from(trimmed)
    }
}

/// The layer a point would land on under these options, exposed so that the
/// import wizard can show the layers an import is about to create before it
/// runs. Field codes are sanitised: a code is user data that can contain
/// anything, and a '/' in it would silently create a nested layer subtree,
/// because a layer name IS a path.
pub fn layer_for_point(point: &SurveyPoint, options: &SurveyImportOptions) -> String {
    if !options.layer_per_code || point.code.is_empty() {
        return options.layer.clone();
    }
    join_layer_path(&options.layer, &sanitized_code(&point.code))
}

/// The project as one command: the layers it needs, then its points.
///
/// Fails with the error the survey's validate_project gives when the project
/// is not internally consistent - it is called rather than re-checked here, so
/// that an importer and this bridge cannot disagree about what a valid project
/// is. Fails with NotFound when a layer is missing and `create_layers` is off.
/// Returns None with NO error when the project has no points: "nothing to do"
/// and "something went wrong" must be distinguishable, the same contract
/// applying survey codes follows.
pub fn import_survey_project(
    document: &Document,
    project: &SurveyProject,
    options: &SurveyImportOptions,
    report: Option<&mut SurveyImportReport>,
) -> Result<Option<CommandPtr>> {
    // Asked of the survey layer rather than re-checked here, so that a parser
    // and this bridge cannot disagree about what a valid project is. It also
    // guarantees what follows: ids are non-empty and unique and every
    // coordinate is finite, so there is no point below that has to be skipped.
    validate_project(project)?;
    validate_layer_path(&options.layer)?;

    let mut discarded = SurveyImportReport::default();
    let out = match report {
        Some(report) => report,
        None => &mut discarded,
    };
    *out = SurveyImportReport::default();

    // Said before the empty check, because a raw observation file whose every
    // target is unreduced imports NOTHING, and "nothing" needs its reason.
    out.points_without_position = project.unpositioned_points.len();
    if out.points_without_position != 0 {
        out.warnings.push(format!(
            "{} point(s) are named in the source with no coordinates - the targets of raw \
             observations - and are not drawn; reducing the observations is what gives them a \
             position",
            out.points_without_position
        ));
    }

    // Nothing to do, and NOT an error: a caller has to be able to tell those
    // apart.
    if project.points.is_empty() {
        return Ok(None);
    }

    let model = document.model();
    // Ordered: the layers are created in name order.
    let mut layers_needed: BTreeSet<String> = BTreeSet::new();
    // Counted per key rather than warned per point: a metadata key that
    // clashes clashes on every one of ten thousand points, and ten thousand
    // identical warnings is the same as none.
    let mut metadata_clashes: BTreeMap<String, usize> = BTreeMap::new();
    let mut without_elevation = 0_usize;

    let mut entities = Vec::with_capacity(project.points.len());

    for point in &project.points {
        let layer = layer_for_point(point, options);
        if let Err(status) = validate_layer_path(&layer) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the field code of a point makes an invalid layer name; import without a layer \
                 per code, or correct the code",
                format!(
                    "point={} code={} layer={} {}",
                    point.id, point.code, layer, status.context
                ),
            ));
        }
        if model.layers.find(&layer).is_none() {
            if !options.create_layers {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "the drawing has no layer for these points and this import was told not to \
                     create one",
                    format!("point={} layer={}", point.id, layer),
                ));
            }
            layers_needed.insert(layer.clone());
        }

        // THE transposition point of the whole module, and the reason this
        // line has a comment on it. The survey model is (northing, easting) -
        // north first, as a field book and LandXML write it - and a CAD Point2
        // is (x, y), which is EASTING first. Getting this backwards produces a
        // drawing that is internally consistent, plots beautifully, and is in
        // the wrong place. ImportedPointsAreNotTransposed asserts it with
        // coordinates whose magnitudes cannot be confused.
        let mut entity = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: Point2::new(point.easting, point.northing),
            }),
            layer,
            ..Entity::default()
        };

        let properties = &mut entity.properties;
        if !options.point_number_property.is_empty() {
            properties.insert(
                options.point_number_property.clone(),
                PropertyValue::from(point.id.as_str()),
            );
        }
        if !options.code_property.is_empty() && !point.code.is_empty() {
            properties.insert(
                options.code_property.clone(),
                PropertyValue::from(point.code.as_str()),
            );
        }
        if !options.description_property.is_empty() && !point.description.is_empty() {
            properties.insert(
                options.description_property.clone(),
                PropertyValue::from(point.description.as_str()),
            );
        }
        // A 2D drawing has no Z, so the height is a property - the same one
        // the archive import writes and the surface builder reads. A point the
        // source gave no height for gets NO property rather than 0.0: the
        // surface builder leaves a point without one out, where a zero would
        // be a real height at the datum and pull the surface down to it
        // (PLAN.MD 45.3b).
        if point.elevation.is_some() {
            set_heights(properties, &[point.elevation]);
        } else {
            without_elevation += 1;
        }
        if point.coordinate_source != CoordinateSource::Unknown {
            properties.insert(
                String::from(COORDINATE_SOURCE_PROPERTY),
                PropertyValue::from(to_string(point.coordinate_source)),
            );
        }
        // Whatever else the file carried. The brief is explicit that metadata
        // is preserved rather than thrown away on import - but never on top of
        // a field this import has just written, because that would let a
        // column in a CSV overwrite the code the same row's code column set.
        for (key, value) in &point.metadata {
            if properties.contains_key(key) {
                *metadata_clashes.entry(key.clone()).or_insert(0) += 1;
                continue;
            }
            properties.insert(key.clone(), PropertyValue::from(value.as_str()));
        }

        if options.record_source && point.source.known() {
            let metadata = &mut entity.metadata;
            let source = &point.source;
            if !source.manufacturer.is_empty() {
                metadata.insert(
                    String::from(SOURCE_MANUFACTURER_META),
                    PropertyValue::from(source.manufacturer.as_str()),
                );
            }
            if !source.format.is_empty() {
                metadata.insert(
                    String::from(SOURCE_FORMAT_META),
                    PropertyValue::from(source.format.as_str()),
                );
            }
            if !source.format_version.is_empty() {
                metadata.insert(
                    String::from(SOURCE_FORMAT_VERSION_META),
                    PropertyValue::from(source.format_version.as_str()),
                );
            }
            if !source.file_name.is_empty() {
                metadata.insert(
                    String::from(SOURCE_FILE_META),
                    PropertyValue::from(source.file_name.as_str()),
                );
            }
            if source.record_number != 0 {
                metadata.insert(
                    String::from(SOURCE_RECORD_META),
                    PropertyValue::from(source.record_number as i64),
                );
            }
        }

        entities.push(entity);
    }

    out.points = entities.len();
    out.points_without_elevation = without_elevation;
    if without_elevation != 0 {
        out.warnings.push(format!(
            "{without_elevation} point(s) have no height in the source; they are drawn without an \
             elevation and a surface will leave them out"
        ));
    }
    for (key, count) in &metadata_clashes {
        out.warnings.push(format!(
            "metadata field \"{key}\" was not imported on {count} point(s): this import already \
             writes that property"
        ));
    }

    // One command, so one undo. The layers come first and the points second,
    // and a Transaction validates only its first step up front precisely
    // because a later step may depend on an earlier one's effect - here, the
    // points need the layers to exist.
    let mut transaction = Transaction::new("IMPORT_SURVEY_POINTS");
    for name in layers_needed {
        let layer = Layer {
            name: name.clone(),
            ..Layer::default()
        };
        transaction.add(create_layer(layer));
        out.layers_created.push(name);
    }
    transaction.add(create_entities(entities));
    Ok(Some(Box::new(transaction)))
}
