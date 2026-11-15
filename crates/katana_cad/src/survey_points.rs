//! The drawing's survey points as a list, and what the import wizard, the
//! point export, the Point Manager and the Point Report need from it (PLAN.MD
//! 45 slices 10, 11 and 13; include/katana/cad/survey_points.hpp,
//! src/katana_cad/customisation/survey_points.cpp).
//!
//! A SURVEY POINT here is a point entity carrying the point-number property
//! the survey import writes (SurveyImportOptions::point_number_property,
//! "point" by default) - the same test the survey tools use to name a point. A
//! point entity without one is a CAD point, not a survey mark: it has no id to
//! list, export or match against an import, so it is left out and, where a
//! caller asked for it by id, counted as left out rather than dropped
//! silently.
//!
//! Everything here reads the drawing through the survey import's own property
//! and metadata keys (survey_import.rs) and heights through the one reader of
//! heights (entity's heights_of), so a point written by the import reads back
//! as it went in. Drawing coordinates are taken as METRES, because that is
//! what import_survey_project writes into them: the survey model is metres and
//! the bridge puts its eastings and northings into the drawing unchanged.
//!
//! Nothing here sees surveyio (cad may not - tools/check_layering.cmake): the
//! wizard hands a parsed SurveyProject in and gets a command back, and the
//! export hands SurveyPoints to the delimited writer itself.
//!
//! The report columns pad by characters, where the C++'s std::format pads by
//! display width: an id holding wide or combining characters lines up
//! otherwise (docs/rust.md, "Where the Rust necessarily differs").

use std::collections::{BTreeMap, BTreeSet};

use katana_commands::{CommandPtr, Transaction, delete_entities};
use katana_core::error::make_error;
use katana_core::text::format_fixed;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::entity::{INVALID_ENTITY_ID, heights_of, property_to_string};
use katana_entity::{Entity, EntityId, EnumName, Geometry, Model, PropertyMap};
use katana_geodesy::coordinate_reference_system::to_string as kind_name;
use katana_geodesy::{
    Coordinate, CoordinateReferenceSystem, CoordinateTransformer, TransformerOptions,
};
use katana_survey::data_model::{DeclaredCoordinateSystem, SurveyPoint, SurveyProject};

use crate::document::Document;
use crate::survey_import::{
    SOURCE_FILE_META, SurveyImportOptions, SurveyImportReport, import_survey_project_to,
};

/// One survey point as the drawing holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawingSurveyPoint {
    pub entity: EntityId,
    /// The point number.
    pub id: String,
    pub code: String,
    pub description: String,
    /// Drawing x.
    pub easting: f64,
    /// Drawing y.
    pub northing: f64,
    /// Absent is not zero.
    pub elevation: Option<f64>,
    pub layer: String,
    /// The file NAME the import recorded; empty when none.
    pub source_file: String,
}

impl Default for DrawingSurveyPoint {
    fn default() -> Self {
        Self {
            entity: INVALID_ENTITY_ID,
            id: String::new(),
            code: String::new(),
            description: String::new(),
            easting: 0.0,
            northing: 0.0,
            elevation: None,
            layer: String::new(),
            source_file: String::new(),
        }
    }
}

// How many ids an error or a warning names before it says "and N more": enough
// to recognise the file, few enough that a whole drawing's worth does not
// become one line of thousands in the log (the area report's limit).
const NAMED_IDS: usize = 12;

fn text_of(map: &PropertyMap, key: &str) -> String {
    if key.is_empty() {
        return String::new();
    }
    map.get(key).map(property_to_string).unwrap_or_default()
}

// A survey point, or None for anything that is not one (see the top).
fn survey_point_of(entity: &Entity, options: &SurveyImportOptions) -> Option<DrawingSurveyPoint> {
    let Geometry::Point(point) = &entity.geometry else {
        return None;
    };
    if options.point_number_property.is_empty()
        || !entity
            .properties
            .contains_key(&options.point_number_property)
    {
        return None;
    }
    Some(DrawingSurveyPoint {
        entity: entity.id,
        id: text_of(&entity.properties, &options.point_number_property),
        code: text_of(&entity.properties, &options.code_property),
        description: text_of(&entity.properties, &options.description_property),
        easting: point.position.x,
        northing: point.position.y,
        // The one reader of heights: a point with no height property has none.
        elevation: heights_of(&entity.properties, 1).first().copied().flatten(),
        layer: entity.layer.clone(),
        source_file: text_of(&entity.metadata, SOURCE_FILE_META),
    })
}

fn named_ids(ids: &[String]) -> String {
    let mut text = String::new();
    for (i, id) in ids.iter().take(NAMED_IDS).enumerate() {
        if i != 0 {
            text.push_str(", ");
        }
        text.push_str(id);
    }
    if ids.len() > NAMED_IDS {
        text.push_str(&format!(" and {} more", ids.len() - NAMED_IDS));
    }
    text
}

// Fixed notation without the locale, and no "-0.000" for a value that rounds
// to zero: core's format_fixed is exactly that.
fn fixed(value: f64, decimals: i32) -> String {
    format_fixed(value, decimals)
}

fn clamped_decimals(decimals: i32) -> i32 {
    decimals.clamp(0, 12)
}

fn csv_field(field: &str) -> String {
    let quote = field.contains([',', '"', '\r', '\n'])
        || (!field.is_empty()
            && (field.starts_with(' ')
                || field.ends_with(' ')
                || field.starts_with('\t')
                || field.ends_with('\t')));
    if !quote {
        return String::from(field);
    }
    let mut out = String::from("\"");
    for c in field.chars() {
        if c == '"' {
            out.push_str("\"\"");
        } else {
            out.push(c);
        }
    }
    out.push('"');
    out
}

// A sentence: a full stop at the end, and a capital first letter when it is a
// lower-case ASCII one.
fn sentence(mut text: String) -> String {
    if !text.is_empty() && !text.ends_with('.') {
        text.push('.');
    }
    if text.as_bytes().first().is_some_and(u8::is_ascii_lowercase) {
        text[..1].make_ascii_uppercase();
    }
    text
}

/// Every survey point in the drawing, in entity id order (the order they were
/// added), whatever layer they are on - a hidden layer's points are still the
/// survey's points. The C++ default `options` is
/// `SurveyImportOptions::default()`.
pub fn drawing_survey_points(
    document: &Document,
    options: &SurveyImportOptions,
) -> Vec<DrawingSurveyPoint> {
    drawing_survey_points_in(document.model(), options)
}

/// drawing_survey_points over the model it reads, for a step of the
/// document's own (a survey job's import, which plans while the document's
/// state is lent to it).
pub(crate) fn drawing_survey_points_in(
    model: &Model,
    options: &SurveyImportOptions,
) -> Vec<DrawingSurveyPoint> {
    let mut points = Vec::new();
    model.entities.for_each(|entity| {
        if let Some(point) = survey_point_of(entity, options) {
            points.push(point);
        }
    });
    // Ids are handed out rising, so this is the order the points were added
    // in, whatever order the database visits them in.
    points.sort_by_key(|point| point.entity);
    points
}

/// The survey points among `ids`, in the order given, and a count of the ids
/// that are not one (another kind of entity, a point with no number, or an id
/// the drawing does not have).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurveyPointPick {
    pub points: Vec<DrawingSurveyPoint>,
    pub not_survey_points: usize,
}

pub fn survey_points_among(
    document: &Document,
    ids: &[EntityId],
    options: &SurveyImportOptions,
) -> SurveyPointPick {
    let mut pick = SurveyPointPick::default();
    for &id in ids {
        let point = document
            .model()
            .entities
            .find(id)
            .and_then(|entity| survey_point_of(entity, options));
        match point {
            Some(point) => pick.points.push(point),
            None => pick.not_survey_points += 1,
        }
    }
    pick
}

/// The survey model's point for one in the drawing - id, northing, easting,
/// elevation (still absent when absent), code and description - for a writer.
/// Where it came from is not carried over: an exported file is a new source.
pub fn to_survey_point(point: &DrawingSurveyPoint) -> SurveyPoint {
    SurveyPoint {
        id: point.id.clone(),
        // The drawing is (x, y) = (easting, northing); the model is north first.
        northing: point.northing,
        easting: point.easting,
        elevation: point.elevation,
        code: point.code.clone(),
        description: point.description.clone(),
        ..SurveyPoint::default()
    }
}

// ---- importing into a drawing that already has points --------------------------------------

/// What to do with an imported point whose id a survey point in the drawing
/// already has. There is no silent default: the wizard shows the choice, and
/// each one's consequence is said in the report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ExistingPointPolicy {
    /// Nothing is imported; the error names the ids. For a file that should be
    /// new to this drawing, a clash means it is the wrong file or a repeat.
    #[default]
    Refuse,
    /// The drawing's point stays; the file's point of that id is not imported.
    /// For topping up a drawing with the new points of a longer file.
    Skip,
    /// The drawing's point is deleted and the file's is imported, in the same
    /// command (one undo puts the old one back). For a re-observed control
    /// file.
    Replace,
    /// Both stay: the drawing then has two points of that id, which the report
    /// says. For a file whose numbering restarts and is known to.
    KeepBoth,
}

/// The C++ `toString(ExistingPointPolicy)`.
impl EnumName for ExistingPointPolicy {
    fn name(self) -> &'static str {
        match self {
            Self::Refuse => "refuse the import",
            Self::Skip => "keep the drawing's point",
            Self::Replace => "replace the drawing's point",
            Self::KeepBoth => "keep both",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SurveyPointImportReport {
    /// What import_survey_project reported.
    pub import: SurveyImportReport,
    /// Ids of the project that a survey point in the drawing already has, in
    /// the project's order, each once.
    pub existing_ids: Vec<String>,
    /// Points not imported under Skip.
    pub skipped: usize,
    /// Drawing entities deleted under Replace.
    pub replaced: usize,
    /// Every warning of the import, and one sentence for what the policy did.
    pub warnings: Vec<String>,
}

/// `project` into the drawing as ONE command: under Replace, the deletion of
/// the points it replaces and then import_survey_project's layers and points.
/// AlreadyExists, naming the ids (the first dozen, then how many more), under
/// Refuse when any id is already in the drawing; the errors of
/// import_survey_project otherwise. None with no error when nothing is left to
/// import - an empty file, or every point skipped - which the report says.
///
/// The command is built, not executed: the wizard calls this to show what an
/// import WILL do on its last page and again to do it, so the preview and the
/// import cannot disagree.
pub fn import_survey_points(
    document: &Document,
    project: &SurveyProject,
    options: &SurveyImportOptions,
    policy: ExistingPointPolicy,
    report: Option<&mut SurveyPointImportReport>,
) -> Result<Option<CommandPtr>> {
    import_survey_points_to(document.model(), project, options, policy, report)
}

/// import_survey_points over the model it reads: what a survey job command
/// plans with while the document's state is lent to its step.
pub(crate) fn import_survey_points_to(
    model: &Model,
    project: &SurveyProject,
    options: &SurveyImportOptions,
    policy: ExistingPointPolicy,
    report: Option<&mut SurveyPointImportReport>,
) -> Result<Option<CommandPtr>> {
    let mut discarded = SurveyPointImportReport::default();
    let out = match report {
        Some(report) => report,
        None => &mut discarded,
    };
    *out = SurveyPointImportReport::default();

    // Every drawing entity of each id: under Replace all of them go, since a
    // drawing that already had two of an id would otherwise keep one.
    let mut in_drawing: BTreeMap<String, Vec<EntityId>> = BTreeMap::new();
    for point in drawing_survey_points_in(model, options) {
        in_drawing.entry(point.id).or_default().push(point.entity);
    }
    let mut clashing: BTreeSet<String> = BTreeSet::new();
    for point in &project.points {
        if in_drawing.contains_key(&point.id) && clashing.insert(point.id.clone()) {
            out.existing_ids.push(point.id.clone());
        }
    }

    let mut kept: Option<SurveyProject> = None;
    let mut replaced: Vec<EntityId> = Vec::new();
    if !out.existing_ids.is_empty() {
        let count = out.existing_ids.len();
        match policy {
            ExistingPointPolicy::Refuse => {
                return Err(make_error(
                    ErrorCode::AlreadyExists,
                    format!(
                        "{count} point id(s) of the file are already in the drawing; choose to \
                         skip, replace or keep both"
                    ),
                    named_ids(&out.existing_ids),
                ));
            }
            ExistingPointPolicy::Skip => {
                let mut copy = project.clone();
                copy.points.retain(|point| !clashing.contains(&point.id));
                out.skipped = project.points.len() - copy.points.len();
                kept = Some(copy);
                out.warnings.push(sentence(format!(
                    "{} point(s) of the file are not imported because the drawing already has \
                     their ids; the drawing's points are kept: {}",
                    out.skipped,
                    named_ids(&out.existing_ids)
                )));
            }
            ExistingPointPolicy::Replace => {
                for id in &out.existing_ids {
                    if let Some(entities) = in_drawing.get(id) {
                        replaced.extend_from_slice(entities);
                    }
                }
                out.replaced = replaced.len();
                out.warnings.push(sentence(format!(
                    "{} point(s) in the drawing are replaced by the file's points of the same \
                     id: {}",
                    out.replaced,
                    named_ids(&out.existing_ids)
                )));
            }
            ExistingPointPolicy::KeepBoth => {
                out.warnings.push(sentence(format!(
                    "{count} point id(s) will be in the drawing twice, the drawing's point and \
                     the file's: {}",
                    named_ids(&out.existing_ids)
                )));
            }
        }
    }

    let to_import = kept.as_ref().unwrap_or(project);
    let command = import_survey_project_to(model, to_import, options, Some(&mut out.import))?;
    for warning in &out.import.warnings {
        out.warnings.push(sentence(warning.clone()));
    }
    let Some(command) = command else {
        return Ok(None);
    };
    if replaced.is_empty() {
        return Ok(Some(command));
    }
    // One command, so one undo puts the replaced points back with the new ones
    // gone. The deletion goes first: Transaction validates its first step up
    // front and the rest as they run.
    let mut transaction = Transaction::new("IMPORT_SURVEY_POINTS");
    transaction.add(delete_entities(replaced));
    transaction.add(command);
    Ok(Some(Box::new(transaction)))
}

// ---- coordinate systems ---------------------------------------------------------------------

/// The system a person says a file is in, checked against the EPSG database so
/// that a mistyped code is refused rather than recorded. InvalidCRS / NotFound
/// from geodesy for a code it does not know.
pub fn declared_system_from_epsg(code: i32) -> Result<DeclaredCoordinateSystem> {
    if code <= 0 {
        return Err(make_error(
            ErrorCode::InvalidCrs,
            "an EPSG code is a positive whole number",
            format!("{code}"),
        ));
    }
    let crs = CoordinateReferenceSystem::from_epsg(code)?;
    Ok(DeclaredCoordinateSystem::epsg(code, crs.name()))
}

/// Every point of `project` from projected system `source_epsg` to projected
/// system `target_epsg` through katana_geodesy's transformer (never a formula
/// of this module's own). The parser never transforms (PLAN.MD 45.2); this is
/// the separate, stated step a person asks for by naming BOTH codes.
///
/// Horizontal only: eastings and northings are transformed, heights are
/// carried through unchanged - a height datum is not a projection, and no
/// vertical transformation is made or implied. The model's metres are
/// converted into each system's own linear unit and back. Unsupported for a
/// geographic, geocentric or compound system (a delimited point file holds
/// grid coordinates; the reader refuses latitude and longitude); the
/// transformer's error, naming the point, for a point it cannot transform -
/// all or nothing. On success the project declares the target system, and its
/// metadata records "transformed from" and "transformation" (the operation
/// PROJ used).
pub fn transform_survey_project(
    mut project: SurveyProject,
    source_epsg: i32,
    target_epsg: i32,
) -> Result<SurveyProject> {
    let source = CoordinateReferenceSystem::from_epsg(source_epsg)?;
    let target = CoordinateReferenceSystem::from_epsg(target_epsg)?;
    for crs in [&source, &target] {
        if !crs.is_projected() {
            return Err(make_error(
                ErrorCode::Unsupported,
                format!(
                    "survey points are grid coordinates: both systems must be projected, and \
                     this one is {}",
                    kind_name(crs.kind())
                ),
                format!("EPSG:{} {}", crs.code(), crs.name()),
            ));
        }
    }
    let mut transformer =
        CoordinateTransformer::create(&source, &target, &TransformerOptions::default())?;
    // The model is metres; each system's coordinates are in its own unit
    // (geodesy's coordinate.rs: not silently converted), so a US survey foot
    // grid gets feet in and gives feet out.
    let source_unit = source.horizontal_unit_to_si();
    let target_unit = target.horizontal_unit_to_si();
    for point in &mut project.points {
        // Traditional GIS order for a projected system: x easting, y northing.
        let input = Coordinate {
            x: point.easting / source_unit,
            y: point.northing / source_unit,
            ..Coordinate::default()
        };
        let moved = match transformer.forward(input) {
            Ok(moved) => moved,
            Err(error) => {
                return Err(make_error(
                    error.code,
                    format!(
                        "point {} cannot be transformed: {}",
                        point.id, error.message
                    ),
                    error.context,
                ));
            }
        };
        point.easting = moved.x * target_unit;
        point.northing = moved.y * target_unit;
    }
    project.coordinate_system = DeclaredCoordinateSystem::epsg(target_epsg, target.name());
    project.metadata.insert(
        String::from("transformed from"),
        format!("EPSG:{source_epsg} {}", source.name()),
    );
    project.metadata.insert(
        String::from("transformation"),
        transformer.operation().name.clone(),
    );
    Ok(project)
}

// ---- reports --------------------------------------------------------------------------------

/// The Point Report: a heading, then one row per point - id, easting,
/// northing, elevation (blank when absent), code, description, source file -
/// with coordinates to `decimals` places (0 to 12), then a count. The C++
/// default `decimals` is 3.
pub fn format_point_report(points: &[DrawingSurveyPoint], decimals: i32) -> String {
    let decimals = clamped_decimals(decimals);
    let mut text = format!(
        "{:<12} {:>16} {:>16} {:>12}  {:<10} {:<24} {}\n",
        "Point", "Easting", "Northing", "Elevation", "Code", "Description", "Source file"
    );
    let mut without_elevation = 0_usize;
    for point in points {
        if point.elevation.is_none() {
            without_elevation += 1;
        }
        let elevation = point
            .elevation
            .map(|z| fixed(z, decimals))
            .unwrap_or_default();
        text.push_str(&format!(
            "{:<12} {:>16} {:>16} {:>12}  {:<10} {:<24} {}\n",
            point.id,
            fixed(point.easting, decimals),
            fixed(point.northing, decimals),
            elevation,
            point.code,
            point.description,
            point.source_file
        ));
    }
    text.push_str(&format!("{} point(s)", points.len()));
    if without_elevation != 0 {
        text.push_str(&format!(", {without_elevation} without an elevation"));
    }
    text.push('\n');
    text
}

/// The same rows as CSV (RFC 4180: a field holding a comma, a quote, a line
/// break or blanks at either end is quoted, a quote doubled), with a header
/// line, CRLF line ends and an absent elevation as an empty field. The C++
/// default `decimals` is 3.
pub fn point_report_csv(points: &[DrawingSurveyPoint], decimals: i32) -> String {
    let decimals = clamped_decimals(decimals);
    let mut text =
        String::from("Point,Easting,Northing,Elevation,Code,Description,Source file\r\n");
    for point in points {
        let elevation = point
            .elevation
            .map(|z| fixed(z, decimals))
            .unwrap_or_default();
        text.push_str(&format!(
            "{},{},{},{},{},{},{}\r\n",
            csv_field(&point.id),
            fixed(point.easting, decimals),
            fixed(point.northing, decimals),
            elevation,
            csv_field(&point.code),
            csv_field(&point.description),
            csv_field(&point.source_file)
        ));
    }
    text
}

/// What the import wizard's last page shows and the log records. The parser's
/// part (file, format, records read, its warnings) is given as text because
/// cad does not see surveyio; the drawing's part is this module's report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SurveyImportSummary {
    pub file_name: String,
    /// "Delimited points 1.0".
    pub format: String,
    /// The layout template, when the format has one.
    pub layout: String,
    pub records_read: usize,
    pub reader_warnings: Vec<String>,
    /// "unknown" or what the person declared.
    pub coordinate_system: String,
    /// Empty when none was asked for.
    pub transformation: String,
    /// What the file's numbers were declared in.
    pub units: String,
    pub policy: ExistingPointPolicy,
}

/// Lines of a report: what was read, every warning as a sentence, and either
/// the error blocking the import or the points and layers it will create.
/// `blocking` is that error, when there is one (the C++ default null).
pub fn format_survey_import_report(
    summary: &SurveyImportSummary,
    report: &SurveyPointImportReport,
    blocking: Option<&Error>,
) -> String {
    let mut text = format!("File: {}\n", summary.file_name);
    text.push_str(&format!("Format: {}\n", summary.format));
    if !summary.layout.is_empty() {
        text.push_str(&format!("Layout: {}\n", summary.layout));
    }
    text.push_str(&format!("Records read: {}\n", summary.records_read));
    text.push_str(&format!("Units: {}\n", summary.units));
    text.push_str(&format!(
        "Coordinate system: {}\n",
        summary.coordinate_system
    ));
    if !summary.transformation.is_empty() {
        text.push_str(&format!("Transformation: {}\n", summary.transformation));
    }
    text.push_str(&format!(
        "Points already in the drawing: {}\n",
        summary.policy.name()
    ));
    let warnings = summary.reader_warnings.len() + report.warnings.len();
    if warnings != 0 {
        text.push_str(&format!("Warnings ({warnings}):\n"));
        for warning in &summary.reader_warnings {
            text.push_str(&format!("  - {}\n", sentence(warning.clone())));
        }
        for warning in &report.warnings {
            text.push_str(&format!("  - {warning}\n"));
        }
    }
    if let Some(blocking) = blocking {
        text.push_str(&format!(
            "Import blocked: {}\n",
            sentence(blocking.describe())
        ));
        return text;
    }
    text.push_str(&format!("Points to create: {}\n", report.import.points));
    if !report.import.layers_created.is_empty() {
        text.push_str("Layers to create: ");
        text.push_str(&report.import.layers_created.join(", "));
        text.push('\n');
    }
    text
}
