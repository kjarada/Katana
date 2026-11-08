//! Survey jobs on the drawing: import a field file as a job, and go back to it
//! later to change the reduction and adjustment (include/katana/cad/survey_job.hpp,
//! src/katana_cad/survey_job.cpp and survey_job_context.cpp).
//!
//! ```text
//! wizard:   surveyio's read_survey -> raw SurveyProject
//!           ImportSurveyJobCommand   (reduce + adjust + draw + keep the job)
//! later:    ReadjustSurveyJobCommand (re-read the stored bytes, reduce and
//!           adjust with new settings, MOVE the job's points, new report)
//! ```
//!
//! Both are ONE undoable command each, like import_survey_project: one Ctrl+Z
//! takes the drawing and the job list back together.
//!
//! cad may not see surveyio (tools/check_layering.cmake), so everything that
//! needs a reader is INJECTED: the import is handed the project already read,
//! and the re-adjustment is handed a SurveyJobReader, which the application
//! binds to surveyio's reader over the job's files in memory. The reduction is
//! injected as well - reduce_for_drawing being what the C++ defaults to, which
//! is survey's reduce_and_adjust with the drawing's control checked - so the
//! commands can be tested with a fake that returns a known outcome.
//!
//! The drawing side is import_survey_points' - the same layers, codes,
//! properties, provenance and existing-point rule as any survey import - so a
//! job's points are ordinary survey points and there is one bridge from the
//! survey model to the drawing, not two. What this module adds is the job:
//! the raw bytes, the settings and the report kept beside the points, and the
//! record of which entity holds which point at which coordinates
//! (SurveyJob::placed_points), which is how a re-adjustment moves the job's
//! own points and nothing else.
//!
//! # How the commands reach the Document in Rust
//!
//! The C++ commands hold `Document&` and change its job list through the
//! SurveyJobAccess friend. A Rust command sees only the model, so each holds
//! the link to the Document's state (document.rs, StateLink) and reaches the
//! job list, the survey map and the next job number through it while the
//! Document lends its state to the step; run anywhere else, the link is empty
//! and the command refuses with InvalidState - the C++'s requireDocumentModel,
//! made structural. A `with` is never held across a call that takes one of
//! its own (the finish's validate, the linework's styling): StateLink::with
//! does not nest. SurveyJobAccess remains the door for a caller holding the
//! Document between steps.
//!
//! A command is moved into the Document's history when it is executed, so
//! what the C++ reads back through the pointer it kept (jobId(), report(),
//! changes(), finishReport()) is read through the command's `results()`, a
//! shared handle the command fills when it runs.
//!
//! The reduction's context holds functions, and survey's ReductionContext
//! holds them as `Box<dyn Fn>` that need not be Send, where a Command must be.
//! The requests therefore carry a SurveyJobContext - the same members, its
//! functions shared and Send - which the commands turn into the survey's
//! ReductionContext when they call the reduction, and reduction_context_for
//! returns one.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, Mutex};

use katana_commands::{
    ChangeSet, ChangeSetCommand, Command, CommandContext, CommandPtr, Transaction,
};
use katana_core::civil_date::now_utc;
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::entity::{heights_of, property_to_string, set_heights};
use katana_entity::{Entity, EntityId, EnumName, Geometry, Model, PointGeometry, PropertyValue};
use katana_geodesy::{
    CoordinateReferenceSystem, CrsKind, GridFactorCalculator, ProjectedCoordinate,
};
use katana_geometry::Point2;
use katana_math::numerics::tolerance;
use katana_storage::ProjectStore;
use katana_survey::data_model::to_string as survey_text;
use katana_survey::data_model::{
    CoordinateSource, GeocentricCoordinate, GeodeticCoordinate, SurveyPoint, SurveyProject,
};
use katana_survey::reduction::{
    ComputedPoint, GeocentricToGridFn, GeodeticToGridFn, GeoidSeparationFn, GridPosition,
    GridScaleFactorFn, ReductionContext, ReductionOutcome, reduce_and_adjust,
};
use katana_survey::reduction_report::{
    ReductionReport, ReportInput, ReportMessage, render_html, render_text,
};
use katana_survey::reduction_settings::{ControlOrigin, ReductionSettings};

use crate::document::{Document, StateLink};
use crate::survey_finish::{
    SharedFinishReport, SurveyFinishEarlier, SurveyFinishOptions, SurveyFinishReport,
    finish_warnings, lock, name_survey_strings, with_survey_finish_to,
};
use crate::survey_import::{COORDINATE_SOURCE_PROPERTY, SurveyImportOptions};
use crate::survey_points::{
    ExistingPointPolicy, SurveyPointImportReport, drawing_survey_points, import_survey_points_to,
    to_survey_point,
};
use crate::survey_tools::c_fixed;

/// The job value is storage's (it has to be saved, and storage may not see
/// cad); these are the names cad code uses for it.
pub use katana_storage::{SurveyJob, SurveyJobFile, SurveyJobPoint};

/// The reduction, as the commands call it: the C++ `std::function`, which an
/// empty `Option` stands for when none is given.
pub type ReductionFunction = Box<
    dyn Fn(&SurveyProject, &ReductionSettings, &ReductionContext) -> Result<ReductionOutcome>
        + Send,
>;

/// The C++ default reduction of the job commands, `&reduceForDrawing`.
pub fn default_reduction() -> ReductionFunction {
    Box::new(reduce_for_drawing)
}

/// Re-reads a job's stored bytes (SurveyJob::source_bytes and sibling_files)
/// into the raw project, with the reader SurveyJob::format_id names. The
/// application supplies it; see the top of this module for why cad cannot.
pub type SurveyJobReader = Box<dyn Fn(&SurveyJob) -> Result<SurveyProject> + Send>;

/// survey's GridScaleFactorFn, shared and Send.
pub type SharedGridScaleFactor = Arc<dyn Fn(f64, f64, f64) -> Option<f64> + Send + Sync>;
/// survey's GeoidSeparationFn, shared and Send.
pub type SharedGeoidSeparation = Arc<dyn Fn(f64, f64) -> Option<f64> + Send + Sync>;
/// survey's GeocentricToGridFn, shared and Send.
pub type SharedGeocentricToGrid =
    Arc<dyn Fn(&GeocentricCoordinate) -> Option<GridPosition> + Send + Sync>;
/// survey's GeodeticToGridFn, shared and Send.
pub type SharedGeodeticToGrid =
    Arc<dyn Fn(&GeodeticCoordinate) -> Option<GridPosition> + Send + Sync>;

/// What the drawing contributes to a reduction - the C++'s
/// `survey::ReductionContext` as a request carries it (the top of this module
/// says why it is a type of its own). Every member is the survey type's, of
/// the same name.
#[derive(Clone, Default)]
pub struct SurveyJobContext {
    pub drawing_points: Vec<SurveyPoint>,
    pub grid_scale_factor: Option<SharedGridScaleFactor>,
    pub geoid_separation: Option<SharedGeoidSeparation>,
    pub geocentric_to_grid: Option<SharedGeocentricToGrid>,
    pub geodetic_to_grid: Option<SharedGeodeticToGrid>,
    pub input: ReportInput,
    pub previous: Vec<ComputedPoint>,
    pub created_utc: String,
}

impl SurveyJobContext {
    /// The survey's ReductionContext of this one, its functions calling these.
    pub fn to_reduction_context(&self) -> ReductionContext {
        ReductionContext {
            drawing_points: self.drawing_points.clone(),
            grid_scale_factor: self
                .grid_scale_factor
                .clone()
                .map(|f| -> GridScaleFactorFn {
                    Box::new(move |northing: f64, easting: f64, height: f64| {
                        f(northing, easting, height)
                    })
                }),
            geoid_separation: self.geoid_separation.clone().map(|f| -> GeoidSeparationFn {
                Box::new(move |northing: f64, easting: f64| f(northing, easting))
            }),
            geocentric_to_grid: self
                .geocentric_to_grid
                .clone()
                .map(|f| -> GeocentricToGridFn {
                    Box::new(move |position: &GeocentricCoordinate| f(position))
                }),
            geodetic_to_grid: self.geodetic_to_grid.clone().map(|f| -> GeodeticToGridFn {
                Box::new(move |position: &GeodeticCoordinate| f(position))
            }),
            input: self.input.clone(),
            previous: self.previous.clone(),
            created_utc: self.created_utc.clone(),
        }
    }
}

// A function is neither Debug nor comparable; the context says only which it has.
impl fmt::Debug for SurveyJobContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SurveyJobContext")
            .field("drawing_points", &self.drawing_points)
            .field("grid_scale_factor", &self.grid_scale_factor.is_some())
            .field("geoid_separation", &self.geoid_separation.is_some())
            .field("geocentric_to_grid", &self.geocentric_to_grid.is_some())
            .field("geodetic_to_grid", &self.geodetic_to_grid.is_some())
            .field("input", &self.input)
            .field("previous", &self.previous)
            .field("created_utc", &self.created_utc)
            .finish()
    }
}

// ---- Document ----------------------------------------------------------------------

impl Document {
    /// The job of `id`, or None.
    pub fn find_survey_job(&self, id: &str) -> Option<&SurveyJob> {
        self.state.survey_jobs.iter().find(|job| job.id == id)
    }
}

/// Document keeps its job list closed (there is no public setter, so nothing
/// but an undoable command changes it). This is the door it lets a caller
/// holding the Document through between steps; the survey job commands reach
/// the list through the Document's state link while they run.
pub struct SurveyJobAccess;

impl SurveyJobAccess {
    pub fn jobs(document: &mut Document) -> &mut Vec<SurveyJob> {
        &mut document.state.survey_jobs
    }

    /// Bumps Document::survey_jobs_generation(); call after every change.
    pub fn changed(document: &mut Document) {
        document.state.survey_jobs_generation += 1;
    }
}

// ---- shared ------------------------------------------------------------------------

// A job's commands change the Document's job list, which is not in the model,
// so they must be run through the Document they were made for: a job command
// run on some other model would draw there and list the job here. Elsewhere
// the link is empty.
fn require_document_model(link: &StateLink) -> Status {
    link.with(|_| ()).map_err(|_| {
        Error::new(
            ErrorCode::InvalidState,
            "a survey job command must be run on the drawing it was made for",
        )
    })
}

// "%.3f", and no "-0.000": core's format_fixed is exactly that.
fn metres(value: f64) -> String {
    katana_core::text::format_fixed(value, 3)
}

fn warn(report: &mut ReductionReport, text: String) {
    report.warnings.push(ReportMessage {
        text,
        ..ReportMessage::default()
    });
}

// The ids the settings hold as control FROM the drawing: those points are the
// drawing's, not the job's, and the job never draws, moves or deletes them.
fn drawing_control_ids(settings: &ReductionSettings) -> BTreeSet<String> {
    settings
        .control
        .iter()
        .filter(|selection| selection.origin == ControlOrigin::Drawing)
        .map(|selection| selection.point.point_id.clone())
        .collect()
}

// What import_survey_points is handed: the reduced project's points and
// nothing that refers to them. The observations, stations and features stay
// out because some of their points are left out here (the drawing's control),
// and a project whose observations name a missing point is not a valid one;
// they are the job's raw data, kept in its bytes, not drawing. The
// unpositioned points stay in: the import's warning about them is the
// person's answer to "why was point 205 not drawn". The drawing's control
// goes from that list too: a file with no coordinates for the point held
// there leaves it among the reduction's unpositioned points, since the
// drawing placed it and not the observations, and "no coordinates, reduce to
// place it" is not why it is not drawn.
fn drawable_project(reduced: &SurveyProject, leave_out: &BTreeSet<String>) -> SurveyProject {
    SurveyProject {
        name: reduced.name.clone(),
        coordinate_system: reduced.coordinate_system.clone(),
        units: reduced.units,
        metadata: reduced.metadata.clone(),
        source: reduced.source.clone(),
        points: reduced
            .points
            .iter()
            .filter(|point| !leave_out.contains(&point.id))
            .cloned()
            .collect(),
        unpositioned_points: reduced
            .unpositioned_points
            .iter()
            .filter(|point| !leave_out.contains(&point.id))
            .cloned()
            .collect(),
        ..SurveyProject::default()
    }
}

// The points import_survey_points will create, in the order it creates them:
// the project's order, less the ids it skips - as indices into `drawable`.
// That order is what pairs each created entity with its point; placed_points
// then confirms it.
fn points_to_be_drawn(
    drawable: &SurveyProject,
    report: &SurveyPointImportReport,
    policy: ExistingPointPolicy,
) -> Vec<usize> {
    let mut skipped: BTreeSet<&str> = BTreeSet::new();
    if policy == ExistingPointPolicy::Skip {
        skipped.extend(report.existing_ids.iter().map(String::as_str));
    }
    drawable
        .points
        .iter()
        .enumerate()
        .filter(|(_, point)| !skipped.contains(point.id.as_str()))
        .map(|(index, _)| index)
        .collect()
}

// Pairs the created point entities with the points they were made from, and
// checks each pairing by the point number the import wrote on the entity. A
// mismatch would make every later re-adjustment move the wrong points, so it
// is an error, never a guess.
fn placed_points(
    model: &Model,
    created: &[EntityId],
    points: &[SurveyPoint],
    drawn: &[usize],
    point_number_property: &str,
) -> Result<Vec<SurveyJobPoint>> {
    let mut placed = Vec::with_capacity(drawn.len());
    let mut next = 0_usize;
    for &id in created {
        let Some(entity) = model.entities.find(id) else {
            continue;
        };
        if !matches!(entity.geometry, Geometry::Point(_)) {
            continue; // not a point: a layer's entity or a line, not one of these
        }
        if next == drawn.len() {
            return Err(make_error(
                ErrorCode::Internal,
                "the survey job drew more points than it computed",
                format!("entity={id}"),
            ));
        }
        let point = &points[drawn[next]];
        next += 1;
        if !point_number_property.is_empty() {
            let numbered = entity
                .properties
                .get(point_number_property)
                .is_some_and(|value| property_to_string(value) == point.id);
            if !numbered {
                return Err(make_error(
                    ErrorCode::Internal,
                    "a survey job's point was drawn out of order",
                    format!("point={} entity={id}", point.id),
                ));
            }
        }
        placed.push(SurveyJobPoint {
            point_id: point.id.clone(),
            entity: id,
            northing: point.northing,
            easting: point.easting,
            elevation: point.elevation,
        });
    }
    if next != drawn.len() {
        return Err(make_error(
            ErrorCode::Internal,
            "the survey job drew fewer points than it computed",
            format!("{} missing", drawn.len() - next),
        ));
    }
    Ok(placed)
}

fn index_of_job(jobs: &[SurveyJob], id: &str) -> Option<usize> {
    jobs.iter().position(|job| job.id == id)
}

fn no_such_job(id: &str) -> Error {
    Error::new(
        ErrorCode::NotFound,
        format!("this drawing has no survey job {id}"),
    )
}

// A job number that is never handed out twice: taken from the entity id
// counter, which is saved with the project and only ever rises (entity ids
// are never reused - entity_database.rs). Consuming one id for it costs
// nothing and means no counter of the job list's own has to be saved, kept
// in step and got right on undo.
fn new_job_id(link: &StateLink, model: &mut Model) -> Result<String> {
    let mut number = model.entities.next_id();
    link.with(|state| {
        while state
            .survey_jobs
            .iter()
            .any(|job| job.id == format!("job-{number}"))
        {
            number += 1;
        }
    })?;
    model.entities.reserve_ids_below(number + 1);
    Ok(format!("job-{number}"))
}

// ---- The import's drawing options, kept on the job (SurveyJob::import_options)
//
// SurveyImportOptions less the layer (the job's own `layer`), as key=value
// lines under a versioned first line - the same shape as the reduction
// settings' text form, for the same reasons: a key a later build adds is
// skipped by this one, and text from a NEWER version is refused rather than
// half read, so a job is never drawn with options the person did not choose.

const IMPORT_OPTIONS_HEADER: &str = "katana-survey-import-options";
const IMPORT_OPTIONS_VERSION: i32 = 1;
const LAYER_PER_CODE_KEY: &str = "layer-per-code";
const CREATE_LAYERS_KEY: &str = "create-layers";
const CODE_PROPERTY_KEY: &str = "code-property";
const POINT_NUMBER_PROPERTY_KEY: &str = "point-number-property";
const DESCRIPTION_PROPERTY_KEY: &str = "description-property";
const RECORD_SOURCE_KEY: &str = "record-source";
// Whether the job's points were coded and strung when it was imported
// (SurveyJobImport::finish). Written only when on, and absent is OFF: a job
// from before these keys was neither, and its text must go on meaning that.
const APPLY_CODES_KEY: &str = "apply-codes";
const DRAW_LINEWORK_KEY: &str = "draw-linework";

// A property name is the person's own text: a line break or a '%' in one is
// percent-encoded, so that a line stays one line.
fn percent_encoded(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let code = c as u32;
        if code < 0x20 || code == 0x7F || c == '%' {
            out.push('%');
            out.push(char::from(HEX[(code >> 4) as usize]));
            out.push(char::from(HEX[(code & 0x0F) as usize]));
        } else {
            out.push(c);
        }
    }
    out
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'A'..=b'F' => Some(c - b'A' + 10),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

// The decoded name is a property name, which the model holds as UTF-8: bytes
// a hand-edited "%FF" makes that are not are read as the replacement
// character (docs/rust.md, "Step 2": the model holds validated UTF-8).
fn percent_decoded(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        if bytes.len() - i < 3 {
            return None; // a '%' needs two hex digits after it
        }
        let high = hex_digit(bytes[i + 1])?;
        let low = hex_digit(bytes[i + 2])?;
        out.push((high << 4) | low);
        i += 3;
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

// std::from_chars of an int over the whole of `text`: an optional '-' and
// decimal digits, nothing else, in range.
fn whole_int(text: &str) -> Option<i32> {
    if text.starts_with('+') {
        return None; // from_chars takes no plus sign
    }
    text.parse().ok()
}

/// What a job's stored option text (SurveyJob::import_options) says: how its
/// points were drawn, and whether they were coded and strung. Kept as text
/// with the job so that a re-adjustment in a later session draws a new point
/// where and as the others went, and treats it as they were treated.
///
/// The text is key=value lines under a versioned first line. A key a later
/// build adds is skipped by this one; text from a NEWER version is refused
/// rather than half read. `apply-codes` and `draw-linework` are written only
/// when they are on, and ABSENT IS OFF: a job imported before those keys
/// existed was not coded, and re-adjusting it must not start to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SurveyJobOptions {
    /// Less its layer, which is SurveyJob::layer.
    pub import: SurveyImportOptions,
    /// "apply-codes".
    pub apply_codes: bool,
    /// "draw-linework".
    pub draw_linework: bool,
}

pub fn write_survey_job_options(stored: &SurveyJobOptions) -> String {
    let options = &stored.import;
    let mut text = String::new();
    let mut line = |key: &str, value: &str| {
        text.push_str(key);
        text.push('=');
        text.push_str(value);
        text.push('\n');
    };
    let flag = |on: bool| if on { "true" } else { "false" };
    line(IMPORT_OPTIONS_HEADER, &format!("{IMPORT_OPTIONS_VERSION}"));
    line(LAYER_PER_CODE_KEY, flag(options.layer_per_code));
    line(CREATE_LAYERS_KEY, flag(options.create_layers));
    line(CODE_PROPERTY_KEY, &percent_encoded(&options.code_property));
    line(
        POINT_NUMBER_PROPERTY_KEY,
        &percent_encoded(&options.point_number_property),
    );
    line(
        DESCRIPTION_PROPERTY_KEY,
        &percent_encoded(&options.description_property),
    );
    line(RECORD_SOURCE_KEY, flag(options.record_source));
    // Only when on: an import that asked for neither writes the text it
    // always wrote.
    if stored.apply_codes {
        line(APPLY_CODES_KEY, "true");
    }
    if stored.draw_linework {
        line(DRAW_LINEWORK_KEY, "true");
    }
    text
}

/// Empty text is the defaults. The text comes from a project file, which may
/// have been damaged or edited by hand, so every malformed line is a
/// ParseFailure naming `job_id` and the line; Unsupported for a newer version.
pub fn read_survey_job_options(text: &str, job_id: &str) -> Result<SurveyJobOptions> {
    let unreadable = |line_number: usize, what: &str| {
        Error::new(
            ErrorCode::ParseFailure,
            format!(
                "the drawing options of survey job {job_id} cannot be read: line {line_number}: \
                 {what}"
            ),
        )
    };
    let must_begin = format!("it must begin with {IMPORT_OPTIONS_HEADER}=<version>");
    let mut stored = SurveyJobOptions::default();
    let mut saw_header = false;
    let mut line_number = 0_usize;
    let mut start = 0_usize;
    while start < text.len() {
        let cut = text[start..].find('\n').map(|at| start + at);
        let mut line = match cut {
            Some(cut) => &text[start..cut],
            None => &text[start..],
        };
        start = cut.map_or(text.len(), |cut| cut + 1);
        line_number += 1;
        if let Some(stripped) = line.strip_suffix('\r') {
            line = stripped;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(equals) = line.find('=') else {
            return Err(unreadable(line_number, "expected key=value"));
        };
        let key = &line[..equals];
        let value = &line[equals + 1..];
        if !saw_header {
            let version = match whole_int(value) {
                Some(version) if key == IMPORT_OPTIONS_HEADER && version > 0 => version,
                _ => return Err(unreadable(line_number, &must_begin)),
            };
            if version > IMPORT_OPTIONS_VERSION {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    format!(
                        "survey job {job_id} was drawn by a newer Katana (drawing options version \
                         {version}); this one reads up to version {IMPORT_OPTIONS_VERSION}"
                    ),
                ));
            }
            saw_header = true;
            continue;
        }
        let options = &mut stored.import;
        let flag = match key {
            LAYER_PER_CODE_KEY => Some(&mut options.layer_per_code),
            CREATE_LAYERS_KEY => Some(&mut options.create_layers),
            RECORD_SOURCE_KEY => Some(&mut options.record_source),
            APPLY_CODES_KEY => Some(&mut stored.apply_codes),
            DRAW_LINEWORK_KEY => Some(&mut stored.draw_linework),
            _ => None,
        };
        if let Some(flag) = flag {
            if value != "true" && value != "false" {
                return Err(unreadable(
                    line_number,
                    &format!("'{key}' must be true or false"),
                ));
            }
            *flag = value == "true";
            continue;
        }
        let options = &mut stored.import;
        let name = match key {
            CODE_PROPERTY_KEY => Some(&mut options.code_property),
            POINT_NUMBER_PROPERTY_KEY => Some(&mut options.point_number_property),
            DESCRIPTION_PROPERTY_KEY => Some(&mut options.description_property),
            _ => None,
        };
        if let Some(name) = name {
            let Some(decoded) = percent_decoded(value) else {
                return Err(unreadable(
                    line_number,
                    &format!("'{key}' has a '%' not followed by two hex digits"),
                ));
            };
            *name = decoded;
            continue;
        }
        // A key a later build added: skipped, as its versioning promises.
    }
    if !text.is_empty() && !saw_header {
        return Err(unreadable(line_number, &must_begin));
    }
    Ok(stored)
}

fn render_into(job: &mut SurveyJob, report: &ReductionReport, created_utc: String) {
    job.report_text = render_text(report);
    job.report_html = render_html(report);
    job.report_created_utc = created_utc;
}

// What the file strung, for the finish: the reduced project's points - all of
// them, the drawing's control too, because a line runs through where the
// reduction has a point whether or not the job drew it - with its
// unpositioned points and its features, and nothing else of it (a project
// whose observations are left out is still a valid one; one whose features
// name a point that is left out is not). Moved out, not copied: the plan has
// no further use for them once the drawable project is made.
fn strings_of(reduced: &mut SurveyProject) -> SurveyProject {
    SurveyProject {
        points: std::mem::take(&mut reduced.points),
        unpositioned_points: std::mem::take(&mut reduced.unpositioned_points),
        features: std::mem::take(&mut reduced.features),
        ..SurveyProject::default()
    }
}

// What of the finish a person may have to act on, said in the job's report:
// the report is where they later ask why a job has no lines, or lost some.
// Only that. What the finish DID is not a warning - a job coded and strung
// with nothing left over adds none - and is in the command's finish report.
fn report_finish(report: &mut ReductionReport, finish: &SurveyFinishReport) {
    for sentence in finish_warnings(finish) {
        warn(report, sentence);
    }
}

// Whether a job's points are given their string numbers as they are drawn
// (name_survey_strings): when the job is to be coded or strung AND the
// drawing has survey codes. Not for a job that asks for neither, which is
// drawn as it always was; and not with no codes loaded, where the finish does
// nothing and must then leave nothing behind either - not one property - so
// that asking for it by default changes no drawing that has no customisation.
fn names_strings(survey_codes_loaded: bool, finish: &SurveyFinishOptions) -> bool {
    finish.any() && survey_codes_loaded
}

// ---- The drawing's part of the reduction (survey_job_context.cpp) ------------------
//
// survey may not see geodesy (tools/check_layering.cmake), so what the
// drawing's coordinate system knows arrives as functions; this is where cad,
// which may, builds them.

// The projected system a grid scale factor is taken from: the system itself,
// or the horizontal part of a compound "EPSG:h+v" (the form a drawing records
// for grid plus height datum). A compound given any other way yields none,
// because this build has no way to split it; the reduction then refuses the
// grid scale "from the projection" by name rather than assume 1.0.
fn projected_part(
    crs: &CoordinateReferenceSystem,
    text: &str,
) -> Option<CoordinateReferenceSystem> {
    if crs.kind() == CrsKind::Projected {
        return Some(crs.clone());
    }
    if crs.kind() != CrsKind::Compound || !crs.is_projected() {
        return None;
    }
    let text = text.strip_prefix("EPSG:")?;
    // std::from_chars of an int: an optional '-' and digits, which must be
    // followed by the '+' of the compound.
    let bytes = text.as_bytes();
    let mut end = usize::from(bytes.first() == Some(&b'-'));
    let digits = end;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == digits || end == bytes.len() || bytes[end] != b'+' {
        return None;
    }
    let code: i32 = text[..end].parse().ok()?;
    let horizontal = CoordinateReferenceSystem::from_epsg(code).ok()?;
    if horizontal.kind() != CrsKind::Projected {
        return None;
    }
    Some(horizontal)
}

// "E 500000.0000 N 5000000.0000 Z 100.0000"; "(no height)" where the point
// has none, which is not a height of zero. To a tenth of a millimetre, the
// resolution of math's tolerance::COORDINATE by which same_mark tells two
// marks apart: millimetres, as a coordinate is usually written, could show
// two marks 0.3 mm apart as one place. std::format's "{:.4f}", a negative
// zero's sign included.
fn place_of(point: &SurveyPoint) -> String {
    let height = match point.elevation {
        Some(elevation) => format!(" Z {}", c_fixed(elevation, 4)),
        None => String::from(" (no height)"),
    };
    format!(
        "E {} N {}{height}",
        c_fixed(point.easting, 4),
        c_fixed(point.northing, 4)
    )
}

// One ground mark: closer than math's tolerance::COORDINATE across, and in
// height too - where one has a height and the other none they are not, as a
// height held from one would not be held from the other.
fn same_mark(a: &SurveyPoint, b: &SurveyPoint) -> bool {
    const SAME_MARK: f64 = tolerance::COORDINATE;
    if !((a.northing - b.northing).hypot(a.easting - b.easting) < SAME_MARK) {
        return false;
    }
    match (a.elevation, b.elevation) {
        (Some(first), Some(second)) => (first - second).abs() < SAME_MARK,
        (None, None) => true,
        _ => false,
    }
}

// "A", "A and B", "A, B and C".
fn listed(items: &[String]) -> String {
    let mut text = String::new();
    for (i, item) in items.iter().enumerate() {
        if i != 0 {
            text.push_str(if i + 1 == items.len() { " and " } else { ", " });
        }
        text.push_str(item);
    }
    text
}

/// survey's reduce_and_adjust as a job on a drawing runs it: the commands'
/// default reduction (default_reduction), and what the import wizard's and
/// the Survey Jobs dialog's previews run. First the control the settings hold
/// FROM the drawing is checked against the drawing's points
/// (context.drawing_points), where the reduction only takes the first point
/// of an id:
///
/// ```text
/// * an id the drawing has more than once is refused (InvalidArgument),
///   naming where each one is, since which is first is only the order they
///   were drawn in;
/// * an id the file never names changes nothing, and a warning says so;
/// * an id the file gives other coordinates is held where the drawing has
///   it, and a warning gives both.
/// ```
///
/// Those warnings open the report's list, before the reduction's own. An id
/// the drawing does not have is the reduction's to refuse, in its own words.
pub fn reduce_for_drawing(
    raw: &SurveyProject,
    settings: &ReductionSettings,
    context: &ReductionContext,
) -> Result<ReductionOutcome> {
    let mut notes: Vec<ReportMessage> = Vec::new();
    for selection in &settings.control {
        if selection.origin != ControlOrigin::Drawing {
            continue;
        }
        let id = &selection.point.point_id;
        let on_drawing: Vec<&SurveyPoint> = context
            .drawing_points
            .iter()
            .filter(|point| point.id == *id)
            .collect();
        let Some(&held) = on_drawing.first() else {
            continue; // the reduction refuses it, naming it
        };
        // The reduction holds the first of an id, and which is first is only
        // the order the points were drawn in: two marks of one id would put
        // the job on either with nothing to say which. Points that are one
        // mark give the same job whichever is taken.
        let mut places = Vec::with_capacity(on_drawing.len());
        let mut one_mark = true;
        for point in &on_drawing {
            places.push(place_of(point));
            one_mark = one_mark && same_mark(point, held);
        }
        if !one_mark {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "Control point {id} is on the drawing {} times, at {}, so which one to hold \
                     is not known; rename or delete all but one.",
                    on_drawing.len(),
                    listed(&places)
                ),
            ));
        }
        let in_file = raw.points.iter().find(|point| point.id == *id);
        let mut named = in_file.is_some();
        for point in &raw.unpositioned_points {
            named = named || point.id == *id;
        }
        // A file read by surveyio names every point an observation, a setup
        // or a feature refers to in one of these two lists (survey's
        // validate_project), so an id in neither is one nothing in the file
        // measures.
        if !named {
            notes.push(ReportMessage {
                text: format!(
                    "Control point {id} is held from the drawing, but the file names no point \
                     {id}, so holding it changes nothing."
                ),
                ..ReportMessage::default()
            });
        } else if let Some(in_file) = in_file
            && !same_mark(held, in_file)
        {
            notes.push(ReportMessage {
                text: format!(
                    "Control point {id} is held where the drawing has it, {}, not where the file \
                     gives it, {}.",
                    place_of(held),
                    place_of(in_file)
                ),
                source: in_file.source.clone(),
            });
        }
    }
    let mut outcome = reduce_and_adjust(raw, settings, context)?;
    let mut warnings = notes;
    warnings.append(&mut outcome.report.warnings);
    outcome.report.warnings = warnings;
    Ok(outcome)
}

/// What the drawing contributes to a reduction: its points, for control the
/// person takes from the drawing, and - from the drawing's coordinate system
/// through katana_geodesy - the grid scale factor, the geoid separation and
/// the GNSS-to-grid conversions. A function the drawing cannot provide (no
/// coordinate system set) is left EMPTY, so the reduction refuses the setting
/// that needs it instead of assuming 1.0. `input` and `created_utc` are the
/// caller's to fill.
pub fn reduction_context_for(document: &Document) -> Result<SurveyJobContext> {
    // Every survey point, by the import's own property names - the same list
    // the Point Manager shows, so "take CP1 from the drawing" means the CP1
    // the person can see. An id the drawing has twice stays in twice: the
    // reduction would hold the first, so reduce_for_drawing refuses to hold
    // an id whose points are not one mark, naming each - in the wizard and on
    // the SURVEY IMPORT line alike.
    let mut context = SurveyJobContext {
        drawing_points: drawing_survey_points(document, &SurveyImportOptions::default())
            .iter()
            .map(to_survey_point)
            .collect(),
        ..SurveyJobContext::default()
    };

    let system = &document.metadata().coordinate_system;
    if system.is_empty() {
        return Ok(context); // a local drawing: nothing a projection could answer
    }
    let crs = match CoordinateReferenceSystem::from_user_input(system) {
        Ok(crs) => crs,
        Err(error) => {
            // Not left empty quietly: a drawing that SAYS it is on a grid and
            // cannot be read would otherwise reduce as if it were local.
            return Err(make_error(
                error.code,
                format!(
                    "the drawing's coordinate system \"{system}\" cannot be read, so its grid \
                     scale factor is unknown: {}",
                    error.message
                ),
                error.context,
            ));
        }
    };

    if let Some(projected) = projected_part(&crs, system) {
        let calculator = GridFactorCalculator::create(&projected)?;
        // Shared behind a lock: the calculator (a PROJ context) answers one
        // call at a time and is not Sync, and the function must be shared.
        let shared = Arc::new(Mutex::new(calculator));
        // The model is metres; the projection's coordinates are in its own
        // unit (geodesy's coordinate.rs), so a feet grid is given feet.
        let unit = projected.horizontal_unit_to_si();
        let factor: SharedGridScaleFactor = Arc::new(
            move |northing: f64, easting: f64, _ellipsoidal_height: f64| -> Option<f64> {
                // The height does not enter: the point scale factor is the
                // projection's, at the point's place on the ellipsoid. Taking
                // a distance to the ellipsoid is the height reduction's job.
                let factors = lock(&shared)
                    .at(&ProjectedCoordinate::new(
                        easting / unit,
                        northing / unit,
                        0.0,
                    ))
                    .ok()?; // outside the projection: the reduction says so
                Some(factors.point_scale_factor())
            },
        );
        context.grid_scale_factor = Some(factor);
    }

    // Left EMPTY, on purpose, until katana_geodesy can answer them:
    //
    // geoid_separation - geodesy has no geoid model, and the PROJ installed
    //   here has no geoid grid, so there is nothing to ask. The reduction
    //   refuses a height reduction to the ellipsoid, naming it.
    // geocentric_to_grid / geodetic_to_grid - a GNSS value carries its own
    //   reference frame (GnssGlobalPositionObservation::reference_frame) and
    //   these functions are not told it. Converting as if it were the
    //   drawing's datum would move a WGS 84 position by as much as the frames
    //   differ (1.8 m between WGS 84 and GDA2020) with nothing to say so.
    Ok(context)
}

// ---- Import ------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct SurveyJobImport {
    /// Everything the job keeps except what the command fills: `id`
    /// (assigned, never reused), `created_entities`, `placed_points`,
    /// `import_options` (from import_options below, so a re-adjustment draws a
    /// new point as these were drawn) and the report fields. `settings` is
    /// what the reduction runs with; `layer` must equal import_options.layer.
    pub job: SurveyJob,
    /// The project as the reader returned it from job.source_bytes.
    pub raw: SurveyProject,
    pub context: SurveyJobContext,
    pub import_options: SurveyImportOptions,
    /// What to do with a computed point whose id a survey point in the drawing
    /// already has (import_survey_points' rule, and its report). A control
    /// point the settings take FROM the drawing (ControlOrigin::Drawing) is
    /// never drawn again, whatever this says: it is the drawing's point, not
    /// the job's.
    pub existing_points: ExistingPointPolicy,
    /// Whether the points are also coded and strung, in the same undo step
    /// (survey_finish.rs); both off by default. The lines come from the
    /// REDUCED project's features and coordinates, which exist only while the
    /// command runs - which is why the finish is part of this command and not
    /// a step a caller could add after it. The two choices are kept with the
    /// job (SurveyJobOptions), so a re-adjustment treats it as it was imported.
    pub finish: SurveyFinishOptions,
}

#[derive(Debug, Default)]
struct ImportOutcome {
    job_id: String,
    report: Option<ReductionReport>,
    finish: Option<SharedFinishReport>,
    created: Vec<EntityId>,
}

/// What an ImportSurveyJobCommand filled when it ran, read through a handle
/// taken before the command was handed to the Document (the C++ keeps a
/// pointer to the command for this).
#[derive(Clone, Debug, Default)]
pub struct ImportSurveyJobResults(Arc<Mutex<ImportOutcome>>);

impl ImportSurveyJobResults {
    /// After a successful execute(): the new job's id. Empty before.
    pub fn job_id(&self) -> String {
        lock(&self.0).job_id.clone()
    }

    /// After a successful execute(): the report of its reduction. None before.
    pub fn report(&self) -> Option<ReductionReport> {
        lock(&self.0).report.clone()
    }

    /// After a successful execute() of an import that asked for a finish:
    /// what the coding and the linework did, or why not. None before, and for
    /// an import that asked for neither.
    pub fn finish_report(&self) -> Option<SurveyFinishReport> {
        let finish = lock(&self.0).finish.clone()?;
        let report = lock(&finish).clone();
        Some(report)
    }

    /// The command's created_entities() as its last execute or redo left
    /// them: the points and then the lines a finish drew.
    pub fn created_entities(&self) -> Vec<EntityId> {
        lock(&self.0).created.clone()
    }
}

// What validate() worked out, handed to the execute() that follows it rather
// than worked out twice: the reduction is the expensive part of an import (a
// network adjustment), and nothing can change the drawing between the two
// calls.
struct ImportPlan {
    outcome: ReductionOutcome,
    drawable: SurveyProject,
    // Indices into `drawable.points`.
    drawn: Vec<usize>,
    // None: nothing to draw. With a finish asked for, the points command
    // inside the one that codes and strings what it creates.
    draw: Option<CommandPtr>,
    // What that finish does, filled when `draw` runs; None when the import
    // asked for none.
    finish: Option<SharedFinishReport>,
}

/// Reduces and adjusts `raw`, draws the result (as import_survey_project
/// does, provenance and all), and adds the job to the document with the
/// rendered report. Undo removes the points and the job; redo restores both
/// with the same entity ids and the same job id.
///
/// With SurveyJobImport::finish the points are coded and strung as part of
/// the same command. The job then owns the lines too: SurveyJob::
/// created_entities and created_entities() hold the points and then the lines
/// (placed_points stays the points alone, so that is the count of points),
/// and Remove Job deletes both. What the finish did is finish_report(); the
/// job's own report gains a warning only for what a person may have to act on
/// (finish_warnings: a step with nothing to go on, a code with no rule, a
/// string left out of every line), so a job coded and strung with nothing
/// left over reports exactly the reduction's warnings. With no survey codes
/// loaded the finish does nothing at all, and the drawing is the one the same
/// import makes without it.
///
/// validate() fails - and nothing is changed - when the reduction fails, with
/// the reduction's own error: an import whose adjustment cannot run is not
/// half-imported. A job too large for a project to keep (storage's
/// ProjectStore::check_survey_job_size) is refused with InvalidArgument
/// naming the file: by validate() for the file alone, before the reduction,
/// and by execute() once the report and point lists are counted too.
pub struct ImportSurveyJobCommand {
    link: StateLink,
    request: SurveyJobImport,
    reduce: Option<ReductionFunction>,
    pending: RefCell<Option<ImportPlan>>,
    // After execute().
    draw_command: Option<CommandPtr>,
    // While undone: the job as it was on the list.
    job: SurveyJob,
    index: usize,
    job_id: String,
    on_list: bool,
    results: ImportSurveyJobResults,
}

impl ImportSurveyJobCommand {
    /// The C++ default `reduce` is `Some(default_reduction())`; None is the
    /// empty function, which validate() refuses.
    pub fn new(
        document: &Document,
        request: SurveyJobImport,
        reduce: Option<ReductionFunction>,
    ) -> Self {
        Self {
            link: document.state_link(),
            request,
            reduce,
            pending: RefCell::new(None),
            draw_command: None,
            job: SurveyJob::default(),
            index: 0,
            job_id: String::new(),
            on_list: false,
            results: ImportSurveyJobResults::default(),
        }
    }

    /// The handle to what this command fills when it runs, to be taken
    /// before the command is executed.
    pub fn results(&self) -> ImportSurveyJobResults {
        self.results.clone()
    }

    pub fn job_id(&self) -> &str {
        &self.job_id
    }

    pub fn report(&self) -> Option<ReductionReport> {
        self.results.report()
    }

    pub fn finish_report(&self) -> Option<SurveyFinishReport> {
        self.results.finish_report()
    }

    fn build(&self, context: &CommandContext<'_>) -> Result<ImportPlan> {
        require_document_model(&self.link)?;
        let request = &self.request;
        let options = &request.import_options;
        if !request.job.layer.is_empty() && request.job.layer != options.layer {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the survey job's layer and the layer its points are imported to differ",
                format!(
                    "job layer={} import layer={}",
                    request.job.layer, options.layer
                ),
            ));
        }
        let Some(reduce) = &self.reduce else {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "no reduction was given for the survey job",
            ));
        };
        // A job the project could never save is refused before the reduction
        // and before anything is drawn: accepted, it would make every later
        // save of the whole drawing fail. The reader takes files the database
        // cannot hold (ProjectStore::MAX_SURVEY_JOB_BYTES). execute() checks
        // again once the report and point lists are added.
        ProjectStore::check_survey_job_size(&request.job, ProjectStore::MAX_SURVEY_JOB_BYTES)?;
        let mut outcome = reduce(
            &request.raw,
            &request.job.settings,
            &request.context.to_reduction_context(),
        )?;
        let mut drawable = drawable_project(
            &outcome.reduced,
            &drawing_control_ids(&request.job.settings),
        );
        let finishing = request.finish.any();
        let loaded = self.link.with(|state| !state.survey_map.empty())?;
        if names_strings(loaded, &request.finish) {
            // Before the import is planned, so the bridge writes each point's
            // string number with its other fields.
            name_survey_strings(&mut drawable.points, &outcome.reduced.features);
        }
        let mut drawing = SurveyPointImportReport::default();
        let mut draw = import_survey_points_to(
            &*context.model,
            &drawable,
            options,
            request.existing_points,
            Some(&mut drawing),
        )?;
        let mut finish = None;
        if finishing {
            // INSIDE this command, around the points command: the features
            // and the reduced coordinates the lines are drawn from are in this
            // plan and nowhere else, the job must own the lines, and a
            // Transaction around this command would validate it - reduce -
            // twice. A None `draw` stays None, and the report says no points
            // were drawn.
            let shared: SharedFinishReport = Arc::default();
            draw = with_survey_finish_to(
                self.link.clone(),
                draw,
                strings_of(&mut outcome.reduced),
                options,
                request.finish.clone(),
                Some(Arc::clone(&shared)),
                SurveyFinishEarlier::default(),
            );
            finish = Some(shared);
        }
        let mut drawn = Vec::new();
        if let Some(draw) = &draw {
            draw.validate(context)?;
            drawn = points_to_be_drawn(&drawable, &drawing, request.existing_points);
        }
        // What happened on the drawing belongs in the report of what happened
        // to the data: a point skipped because the drawing had it is as much
        // a result of this import as a rejected observation.
        for warning in drawing.warnings {
            warn(&mut outcome.report, warning);
        }
        Ok(ImportPlan {
            outcome,
            drawable,
            drawn,
            draw,
            finish,
        })
    }
}

impl Command for ImportSurveyJobCommand {
    fn name(&self) -> &str {
        "IMPORT_SURVEY_JOB"
    }

    fn validate(&self, context: &CommandContext<'_>) -> Status {
        let plan = self.build(context)?;
        *self.pending.borrow_mut() = Some(plan);
        Ok(())
    }

    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        let pending = self.pending.get_mut().take();
        let mut plan = match pending {
            Some(plan) => plan,
            None => self.build(context)?,
        };

        let id = new_job_id(&self.link, context.model)?;
        let mut created = Vec::new();
        let mut placed = Vec::new();
        if let Some(draw) = &mut plan.draw {
            draw.execute(context)?;
            created = draw.created_entities();
            match placed_points(
                &*context.model,
                &created,
                &plan.drawable.points,
                &plan.drawn,
                &self.request.import_options.point_number_property,
            ) {
                Ok(pairs) => placed = pairs,
                Err(error) => {
                    let _ = draw.undo(context); // all or nothing
                    return Err(error);
                }
            }
        }

        // Nothing below can fail, so the request's job - its raw bytes may be
        // tens of megabytes - is MOVED onto the list rather than copied, and
        // what only the reduction needed is let go: this command lives on in
        // the undo history, and a copy of the raw project and of every
        // drawing point there would be dead weight.
        let mut job = std::mem::take(&mut self.request.job);
        job.layer.clone_from(&self.request.import_options.layer);
        // The points and, after them, the lines a finish drew: the job owns
        // both (Remove Job deletes this list), and placed_points is the points
        // alone.
        job.created_entities = created;
        job.placed_points = placed;
        // What was ASKED for, whether or not a step then had anything to do:
        // a re-adjustment treats the job as the person chose to import it.
        let finish = &self.request.finish;
        job.import_options = write_survey_job_options(&SurveyJobOptions {
            import: self.request.import_options.clone(),
            apply_codes: finish.codes,
            draw_linework: finish.linework,
        });

        let stamp = if self.request.context.created_utc.is_empty() {
            now_utc()
        } else {
            self.request.context.created_utc.clone()
        };
        if job.imported_utc.is_empty() {
            job.imported_utc.clone_from(&stamp);
        }
        if let Some(finish) = &plan.finish {
            report_finish(&mut plan.outcome.report, &lock(finish));
        }
        render_into(&mut job, &plan.outcome.report, stamp);
        // The whole job now, report and point lists included: validate() saw
        // only the file. Checked before the id is given, so the sentence names
        // the file the person chose, not an id they never saw.
        if let Err(error) =
            ProjectStore::check_survey_job_size(&job, ProjectStore::MAX_SURVEY_JOB_BYTES)
        {
            if let Some(draw) = &mut plan.draw {
                let _ = draw.undo(context); // all or nothing
            }
            self.request.job = job; // the bytes back where they came from
            return Err(error);
        }
        job.id = id;

        self.job_id.clone_from(&job.id);
        self.index = self.link.with(|state| {
            let index = state.survey_jobs.len();
            state.survey_jobs.push(job);
            state.survey_jobs_generation += 1;
            index
        })?;
        self.on_list = true;
        self.draw_command = plan.draw;
        self.request.raw = SurveyProject::default();
        self.request.context = SurveyJobContext::default();
        let mut results = lock(&self.results.0);
        results.job_id.clone_from(&self.job_id);
        results.report = Some(plan.outcome.report);
        results.finish = plan.finish;
        results.created = self
            .draw_command
            .as_ref()
            .map(|draw| draw.created_entities())
            .unwrap_or_default();
        Ok(())
    }

    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        if !self.on_list {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "the survey job import has not been done",
            ));
        }
        let job_id = &self.job_id;
        let Some(index) = self
            .link
            .with(|state| index_of_job(&state.survey_jobs, job_id))?
        else {
            return Err(no_such_job(&self.job_id));
        };
        if let Some(draw) = &mut self.draw_command {
            draw.undo(context)?;
        }
        self.index = index;
        self.job = self.link.with(|state| {
            state.survey_jobs_generation += 1;
            state.survey_jobs.remove(index)
        })?;
        self.on_list = false;
        Ok(())
    }

    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        if self.on_list || self.job_id.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "the survey job import has not been undone",
            ));
        }
        if let Some(draw) = &mut self.draw_command {
            draw.redo(context)?;
        }
        let job = std::mem::take(&mut self.job);
        let index = self.index;
        self.link.with(|state| {
            let at = index.min(state.survey_jobs.len());
            state.survey_jobs.insert(at, job);
            state.survey_jobs_generation += 1;
        })?;
        self.on_list = true;
        lock(&self.results.0).created = self.created_entities();
        Ok(())
    }

    fn created_entities(&self) -> Vec<EntityId> {
        self.draw_command
            .as_ref()
            .map(|draw| draw.created_entities())
            .unwrap_or_default()
    }
}

// ---- Re-adjust ---------------------------------------------------------------------

/// A job's point the person has changed since the job last placed it - moved,
/// re-levelled or deleted - and what re-adjusting does about it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HandEditPolicy {
    /// The edit stands: the point stays where the person put it, and a
    /// deleted one stays deleted. The report names each one with the
    /// coordinates the new run computed for it.
    #[default]
    Keep,
    /// The new run's coordinates replace the edit, and a deleted point is
    /// drawn again. The report names each one.
    Overwrite,
}

/// The C++ `toString(HandEditPolicy)`.
impl EnumName for HandEditPolicy {
    fn name(self) -> &'static str {
        match self {
            Self::Keep => "keep the edits made by hand",
            Self::Overwrite => "overwrite the edits made by hand",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SurveyJobReadjustment {
    pub job_id: String,
    /// The new settings.
    pub settings: ReductionSettings,
    /// `previous` is filled by the command.
    pub context: SurveyJobContext,
    pub hand_edits: HandEditPolicy,
    /// The version of the reader the SurveyJobReader used, which becomes the
    /// job's parser_version; empty leaves the job's as it was.
    pub parser_version: String,
    /// HOW a job that was imported coded or strung is finished again: the
    /// colour lookup and the other coding options, the control codes, the
    /// order. WHETHER is the job's own, as it was imported (SurveyJobOptions),
    /// so `finish.codes` and `finish.linework` are not read here. Pass what
    /// the import was passed, or a new point's style differs from its
    /// neighbours' (a colour name with no lookup has no RGB).
    pub finish: SurveyFinishOptions,
}

/// What one re-adjustment did to the drawing, by point id, in the order of
/// the job's points and then of the new run's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SurveyJobChanges {
    /// Given the new run's coordinates.
    pub moved: Vec<String>,
    /// Produced now, not drawn by the job before.
    pub created: Vec<String>,
    /// No longer produced, so deleted.
    pub removed: Vec<String>,
    /// Changed by hand since the job last placed them, and deleted by hand.
    /// Under HandEditPolicy::Keep these are left as the person left them.
    pub edited_by_hand: Vec<String>,
    pub deleted_by_hand: Vec<String>,
    /// Produced now, but a survey point the job did not create already has
    /// the id: not drawn, because the job never touches another's point.
    pub not_drawn: Vec<String>,
}

// The fields of a job a re-adjustment replaces. Swapped in and out rather
// than keeping a copy of the whole job for undo, which would copy the raw
// field file - tens of megabytes - into every re-adjustment's undo step.
#[derive(Debug, Default)]
struct JobRevision {
    settings: ReductionSettings,
    parser_version: String,
    created_entities: Vec<EntityId>,
    placed_points: Vec<SurveyJobPoint>,
    report_text: String,
    report_html: String,
    report_created_utc: String,
}

impl JobRevision {
    fn swap_with(&mut self, job: &mut SurveyJob) {
        std::mem::swap(&mut self.settings, &mut job.settings);
        std::mem::swap(&mut self.parser_version, &mut job.parser_version);
        std::mem::swap(&mut self.created_entities, &mut job.created_entities);
        std::mem::swap(&mut self.placed_points, &mut job.placed_points);
        std::mem::swap(&mut self.report_text, &mut job.report_text);
        std::mem::swap(&mut self.report_html, &mut job.report_html);
        std::mem::swap(&mut self.report_created_utc, &mut job.report_created_utc);
    }
}

// How a point the job placed stands now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlacedState {
    AsPlaced,
    EditedByHand,
    DeletedByHand,
}

struct PlacedNow<'a> {
    state: PlacedState,
    entity: Option<&'a Entity>,
    position: Point2,
    elevation: Option<f64>,
}

fn state_of<'a>(model: &'a Model, placed: &SurveyJobPoint) -> PlacedNow<'a> {
    let mut now = PlacedNow {
        state: PlacedState::DeletedByHand,
        entity: None,
        position: Point2::default(),
        elevation: None,
    };
    let Some(entity) = model.entities.find(placed.entity) else {
        return now;
    };
    let Geometry::Point(point) = &entity.geometry else {
        return now;
    };
    now.entity = Some(entity);
    now.position = point.position;
    now.elevation = heights_of(&entity.properties, 1).first().copied().flatten();
    // Exact comparison on purpose: the job wrote these very doubles and the
    // project stores them bit for bit, so ANY difference is an edit - there
    // is no tolerance below which moving a mark by hand does not count.
    let moved = now.position.x != placed.easting || now.position.y != placed.northing;
    let relevelled = now.elevation != placed.elevation;
    now.state = if moved || relevelled {
        PlacedState::EditedByHand
    } else {
        PlacedState::AsPlaced
    };
    now
}

// The sentence's tail saying what the new run made of a point, for the report.
fn new_run_puts(point: Option<&SurveyPoint>) -> String {
    let Some(point) = point else {
        return String::from(" The new run does not compute it.");
    };
    let height = point
        .elevation
        .map(|elevation| format!(", H {}", metres(elevation)))
        .unwrap_or_default();
    format!(
        " The new run puts it at N {}, E {}{height}.",
        metres(point.northing),
        metres(point.easting)
    )
}

#[derive(Debug, Default)]
struct ReadjustOutcome {
    report: Option<ReductionReport>,
    changes: SurveyJobChanges,
    finish: Option<SharedFinishReport>,
    created: Vec<EntityId>,
}

/// What a ReadjustSurveyJobCommand filled when it ran, read through a handle
/// taken before the command was handed to the Document.
#[derive(Clone, Debug, Default)]
pub struct ReadjustSurveyJobResults(Arc<Mutex<ReadjustOutcome>>);

impl ReadjustSurveyJobResults {
    pub fn report(&self) -> Option<ReductionReport> {
        lock(&self.0).report.clone()
    }

    /// After a successful execute(): what it did to the drawing.
    pub fn changes(&self) -> SurveyJobChanges {
        lock(&self.0).changes.clone()
    }

    /// After a successful execute() that finished the job again: how many new
    /// points were coded, and the lines redrawn in place, created and
    /// deleted. None before, for a job imported without a finish, and for a
    /// run that changed nothing in the drawing (its lines then stand as they
    /// are).
    pub fn finish_report(&self) -> Option<SurveyFinishReport> {
        let finish = lock(&self.0).finish.clone()?;
        let report = lock(&finish).clone();
        Some(report)
    }

    /// The command's created_entities() as its last execute or redo left
    /// them.
    pub fn created_entities(&self) -> Vec<EntityId> {
        lock(&self.0).created.clone()
    }
}

#[derive(Default)]
struct ReadjustPlan {
    report: ReductionReport,
    // The new points only.
    fresh: SurveyProject,
    // Indices into `fresh.points`.
    drawn: Vec<usize>,
    // What the new points are numbered by.
    point_number_property: String,
    // None: the drawing is unchanged.
    entities: Option<CommandPtr>,
    // placed_points without the new ones yet.
    revision: JobRevision,
    deleted: Vec<EntityId>,
    changes: SurveyJobChanges,
    // Filled when `entities` runs, for a job that is finished again; None
    // otherwise.
    finish: Option<SharedFinishReport>,
}

// What a re-adjustment reads of its job, taken out while the Document's state
// is lent and given back before anything that reaches the state again: the
// stored options, the re-read project, and the job's own lists.
struct JobAsFound {
    stored: SurveyJobOptions,
    raw: SurveyProject,
    layer: String,
    placed_points: Vec<SurveyJobPoint>,
    created_entities: Vec<EntityId>,
    parser_version: String,
    survey_codes_loaded: bool,
}

/// Re-reads the job's stored bytes, reduces and adjusts them with the new
/// settings, and moves the job's existing points to the new coordinates (a
/// point the new run no longer produces is deleted, a new one is created on
/// the job's layer). The job's settings, parser version and report are
/// replaced; the report shows each point's shift from the previous run. Undo
/// puts every point, the settings and the report back.
///
/// Only the job's own points are touched (SurveyJob::placed_points). One the
/// person has moved, re-levelled or deleted since the job placed it is found
/// by comparing the two, named in the report and in changes(), and kept or
/// overwritten as SurveyJobReadjustment::hand_edits says. A new point whose
/// id a point the job did not create already has is not drawn, and said so. A
/// point of the job's that the new settings hold as control FROM THE DRAWING
/// is left where it stands, under either policy: the run was held to it there.
///
/// A job imported with a finish is finished again, in the same undo step, as
/// its stored options say (SurveyJobOptions): the points this run draws for
/// the first time are coded - the others keep what they have - and, when the
/// run changes the drawing at all, the lines the job owns are strung again
/// from the new reduction, the file's numbered strings first and then the
/// job's other points by their codes.
///
/// ```text
/// * A line this run still makes (the same code and string number) is
///   REDRAWN IN PLACE: the same entity, with its id, its layer and style and
///   so every label, leader and dimension on it, at the new vertices - as a
///   point of the job is moved and not replaced. A line none of whose points
///   moved is not touched.
/// * A line this run no longer makes is deleted, and what was attached to it
///   goes with it, as with any deleted entity; the report says how many.
/// * A line runs through each point where the point STANDS: one the person
///   moved and HandEditPolicy::Keep leaves there is still on its line. Where
///   the line itself runs is the job's: a line the person reshaped is
///   redrawn like any other, lines having no record of how the job left
///   them, as points have.
/// ```
///
/// When the linework step has nothing to go on (no survey codes loaded, no
/// rule for any code) the job's lines are left as the earlier run drew them,
/// and the report says so and how many. A job imported without a finish is
/// re-adjusted exactly as it always was.
///
/// NotFound for a job the document does not have; InvalidArgument when the
/// job with its new report and points would be too large for a project to
/// keep; the reader's or the reduction's own error otherwise - each with
/// nothing changed.
pub struct ReadjustSurveyJobCommand {
    link: StateLink,
    request: SurveyJobReadjustment,
    read: Option<SurveyJobReader>,
    reduce: Option<ReductionFunction>,
    pending: RefCell<Option<ReadjustPlan>>,
    // After execute().
    entities: Option<CommandPtr>,
    // The revision NOT on the job: the old one while applied.
    other: JobRevision,
    applied: bool,
    results: ReadjustSurveyJobResults,
}

impl ReadjustSurveyJobCommand {
    /// The C++ default `reduce` is `Some(default_reduction())`; None for
    /// `read` or `reduce` is the empty function, which validate() refuses.
    pub fn new(
        document: &Document,
        request: SurveyJobReadjustment,
        read: Option<SurveyJobReader>,
        reduce: Option<ReductionFunction>,
    ) -> Self {
        Self {
            link: document.state_link(),
            request,
            read,
            reduce,
            pending: RefCell::new(None),
            entities: None,
            other: JobRevision::default(),
            applied: false,
            results: ReadjustSurveyJobResults::default(),
        }
    }

    /// The handle to what this command fills when it runs, to be taken
    /// before the command is executed.
    pub fn results(&self) -> ReadjustSurveyJobResults {
        self.results.clone()
    }

    pub fn report(&self) -> Option<ReductionReport> {
        self.results.report()
    }

    pub fn changes(&self) -> SurveyJobChanges {
        self.results.changes()
    }

    pub fn finish_report(&self) -> Option<SurveyFinishReport> {
        self.results.finish_report()
    }

    // The job as it is on the list, read while the state is lent.
    fn find_job(&self) -> Result<JobAsFound> {
        let request = &self.request;
        self.link.with(|state| -> Result<JobAsFound> {
            let Some(job) = state
                .survey_jobs
                .iter()
                .find(|job| job.id == request.job_id)
            else {
                return Err(no_such_job(&request.job_id));
            };
            let (Some(read), Some(_)) = (&self.read, &self.reduce) else {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "no reader or no reduction was given for re-adjusting the survey job",
                ));
            };
            // A point this run draws for the first time goes where and as the
            // import drew the job's others - a layer per code, the same
            // property names - so the job's points stay together on the
            // drawing. Read before the reduction: options that cannot be read
            // end the run before its costly part.
            let stored = read_survey_job_options(&job.import_options, &job.id)?;
            let raw = read(job)?;
            Ok(JobAsFound {
                stored,
                raw,
                layer: job.layer.clone(),
                placed_points: job.placed_points.clone(),
                created_entities: job.created_entities.clone(),
                parser_version: job.parser_version.clone(),
                survey_codes_loaded: !state.survey_map.empty(),
            })
        })?
    }

    fn build(&self, context: &CommandContext<'_>) -> Result<ReadjustPlan> {
        require_document_model(&self.link)?;
        let found = self.find_job()?;
        let request = &self.request;
        let JobAsFound {
            stored,
            raw,
            layer,
            placed_points: job_placed,
            created_entities: job_created,
            parser_version,
            survey_codes_loaded,
        } = found;
        let mut options = stored.import;
        options.layer = if layer.is_empty() {
            SurveyImportOptions::default().layer
        } else {
            layer
        };
        // Whether the job is coded and strung is the JOB's, as it was
        // imported - absent from its text, as in every job from before the
        // finish existed, is off - and how is the caller's: a colour lookup is
        // not job data.
        let mut finish = request.finish.clone();
        finish.codes = stored.apply_codes;
        finish.linework = stored.draw_linework;

        // The previous run is what the job last PLACED, not where the points
        // are now: the report's "shift from the previous run" is about the
        // survey, and a hand edit is reported on its own.
        let mut reduction_context = request.context.to_reduction_context();
        reduction_context.previous = job_placed
            .iter()
            .map(|placed| ComputedPoint {
                id: placed.point_id.clone(),
                northing: placed.northing,
                easting: placed.easting,
                elevation: placed.elevation,
                ..ComputedPoint::default()
            })
            .collect();
        let Some(reduce) = &self.reduce else {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "no reader or no reduction was given for re-adjusting the survey job",
            ));
        };
        let mut outcome = reduce(&raw, &request.settings, &reduction_context)?;
        let mut plan = ReadjustPlan {
            report: std::mem::take(&mut outcome.report),
            ..ReadjustPlan::default()
        };
        let leave_out = drawing_control_ids(&request.settings);
        let mut computed: HashMap<&str, &SurveyPoint> =
            HashMap::with_capacity(outcome.reduced.points.len());
        for point in &outcome.reduced.points {
            if !leave_out.contains(&point.id) {
                computed.entry(point.id.as_str()).or_insert(point);
            }
        }

        let keep_edits = request.hand_edits == HandEditPolicy::Keep;
        let model: &Model = &*context.model;
        // Ids the job's own points answer.
        let mut accounted_for: HashSet<&str> = HashSet::new();
        let mut change = ChangeSet::default();
        let changes = &mut plan.changes;
        let mut kept: Vec<SurveyJobPoint> = Vec::with_capacity(job_placed.len());
        for placed in &job_placed {
            let id = placed.point_id.as_str();
            let now = computed.get(id).copied();
            let current = state_of(model, placed);
            if current.state == PlacedState::DeletedByHand {
                changes.deleted_by_hand.push(placed.point_id.clone());
                let fate = if keep_edits || now.is_none() {
                    "it stays deleted."
                } else {
                    "it is drawn again."
                };
                warn(
                    &mut plan.report,
                    format!(
                        "Point {id} was deleted from the drawing after the job placed it; \
                         {fate}{}",
                        new_run_puts(now)
                    ),
                );
                if keep_edits {
                    kept.push(placed.clone());
                    accounted_for.insert(id);
                }
                // Overwrite: not accounted for, so a point the new run
                // computes is drawn again below like any new one.
                continue;
            }
            if leave_out.contains(id) {
                // The new settings hold this point as control FROM THE
                // DRAWING: the run took it where the drawing has it, so there
                // is nothing to move it to - and it must not be deleted as "no
                // longer computed", which would take away the mark the
                // adjustment was just held to (and the next run would find no
                // such point). It stays the job's, recorded where the job put
                // it, so a hand edit remains visible as one to a later run that
                // computes it again.
                if current.state == PlacedState::EditedByHand {
                    changes.edited_by_hand.push(placed.point_id.clone());
                    warn(
                        &mut plan.report,
                        format!(
                            "Point {id} was moved or re-levelled by hand after the job placed \
                             it; the new run holds it as control from the drawing, where it now \
                             stands, so it is left there."
                        ),
                    );
                }
                kept.push(placed.clone());
                accounted_for.insert(id);
                continue;
            }
            if current.state == PlacedState::EditedByHand {
                changes.edited_by_hand.push(placed.point_id.clone());
                let fate = if keep_edits {
                    "the edit is kept."
                } else {
                    "the edit is overwritten."
                };
                warn(
                    &mut plan.report,
                    format!(
                        "Point {id} was moved or re-levelled by hand after the job placed it; \
                         {fate}{}",
                        new_run_puts(now)
                    ),
                );
                if keep_edits {
                    kept.push(placed.clone());
                    accounted_for.insert(id);
                    continue;
                }
            }
            accounted_for.insert(id);
            let Some(now) = now else {
                change.remove.push(placed.entity);
                plan.deleted.push(placed.entity);
                changes.removed.push(placed.point_id.clone());
                continue;
            };
            let moved = SurveyJobPoint {
                point_id: placed.point_id.clone(),
                entity: placed.entity,
                northing: now.northing,
                easting: now.easting,
                elevation: now.elevation,
            };
            let same = current.position.x == moved.easting
                && current.position.y == moved.northing
                && current.elevation == moved.elevation;
            if !same && let Some(entity) = current.entity {
                let mut updated = entity.clone();
                updated.geometry = Geometry::Point(PointGeometry {
                    position: Point2::new(now.easting, now.northing),
                });
                set_heights(&mut updated.properties, &[now.elevation]);
                if now.coordinate_source != CoordinateSource::Unknown {
                    updated.properties.insert(
                        String::from(COORDINATE_SOURCE_PROPERTY),
                        PropertyValue::from(survey_text(now.coordinate_source)),
                    );
                }
                change.modify.push(updated);
                changes.moved.push(placed.point_id.clone());
            }
            kept.push(moved);
        }
        if !changes.removed.is_empty() {
            warn(
                &mut plan.report,
                format!(
                    "{} point(s) the job placed are not computed by the new run and are deleted \
                     from the drawing.",
                    changes.removed.len()
                ),
            );
        }

        // Points the job has not drawn before: drawn as the import drew the
        // job's others (`options`, from the job), and never over a survey
        // point the job did not create (Skip) - that point is someone else's,
        // and the report says it was left alone. Built from the unaccounted
        // points only, not copied whole and pruned: on a re-adjustment that
        // merely moves points this is nearly empty.
        plan.fresh.name.clone_from(&outcome.reduced.name);
        plan.fresh.coordinate_system = outcome.reduced.coordinate_system.clone();
        plan.fresh.units = outcome.reduced.units;
        plan.fresh.source = outcome.reduced.source.clone();
        plan.fresh.points = outcome
            .reduced
            .points
            .iter()
            .filter(|point| {
                !accounted_for.contains(point.id.as_str()) && !leave_out.contains(&point.id)
            })
            .cloned()
            .collect();
        plan.point_number_property
            .clone_from(&options.point_number_property);
        if names_strings(survey_codes_loaded, &finish) {
            // As the import did before it drew the others.
            name_survey_strings(&mut plan.fresh.points, &outcome.reduced.features);
        }
        let mut drawing = SurveyPointImportReport::default();
        let mut draw: Option<CommandPtr> = None;
        if !plan.fresh.points.is_empty() {
            draw = import_survey_points_to(
                model,
                &plan.fresh,
                &options,
                ExistingPointPolicy::Skip,
                Some(&mut drawing),
            )?;
            plan.drawn = points_to_be_drawn(&plan.fresh, &drawing, ExistingPointPolicy::Skip);
            for &index in &plan.drawn {
                plan.changes
                    .created
                    .push(plan.fresh.points[index].id.clone());
            }
            plan.changes.not_drawn = drawing.existing_ids;
            if !plan.changes.not_drawn.is_empty() {
                warn(
                    &mut plan.report,
                    format!(
                        "{} point(s) the new run computes are not drawn, because the drawing \
                         already has a survey point of that id that this job did not create.",
                        plan.changes.not_drawn.len()
                    ),
                );
            }
        }

        let mut moving: Option<CommandPtr> = None;
        if !change.empty() {
            let destructive = !plan.deleted.is_empty();
            let command: CommandPtr = Box::new(ChangeSetCommand::new(
                "MOVE_SURVEY_JOB_POINTS",
                move |_: &CommandContext<'_>| Ok(change.clone()),
                destructive,
            ));
            moving = Some(command);
        }
        // A Transaction only when there are two parts: a ChangeSetCommand
        // keeps the set its validate() built for its execute(), where a
        // Transaction validates its first part again on execute - for a
        // re-adjustment that moves every point of a large job, that is a
        // second copy of all of them.
        plan.entities = match (moving, draw) {
            (Some(moving), Some(draw)) => {
                let mut transaction = Transaction::new("READJUST_SURVEY_JOB");
                transaction.add(moving);
                transaction.add(draw);
                let both: CommandPtr = Box::new(transaction);
                Some(both)
            }
            (Some(moving), None) => Some(moving),
            (None, draw) => draw,
        };
        if finish.any() && plan.entities.is_some() {
            // Finished again, in this one undo step: the points drawn now are
            // coded, and the job's lines - drawn from the coordinates this run
            // has just replaced - are strung again from the new ones. A line
            // this run still makes is the same entity afterwards, moved; only
            // one it no longer makes is deleted. A run that changes nothing in
            // the drawing leaves its lines be.
            let mut earlier = SurveyFinishEarlier::default();
            if finish.linework {
                // One the person deleted by hand is still listed here; the
                // finish passes over an id that is no longer a point in the
                // drawing.
                earlier.points = kept.iter().map(|placed| placed.entity).collect();
                for &id in &job_created {
                    if let Some(entity) = model.entities.find(id)
                        && !matches!(entity.geometry, Geometry::Point(_))
                    {
                        earlier.lines.push(id);
                    }
                }
                // Under Overwrite a deleted point the run computes is drawn
                // again and stands like any other; under Keep it stays
                // deleted, and the finish says which of its lines still run
                // through it.
                if keep_edits {
                    earlier
                        .deleted_by_hand
                        .clone_from(&plan.changes.deleted_by_hand);
                }
            }
            let shared: SharedFinishReport = Arc::default();
            plan.entities = with_survey_finish_to(
                self.link.clone(),
                plan.entities.take(),
                strings_of(&mut outcome.reduced),
                &options,
                finish,
                Some(Arc::clone(&shared)),
                earlier,
            );
            plan.finish = Some(shared);
        }
        if let Some(entities) = &plan.entities {
            entities.validate(context)?;
        }

        plan.revision.settings = request.settings.clone();
        plan.revision.parser_version = if request.parser_version.is_empty() {
            parser_version
        } else {
            request.parser_version.clone()
        };
        plan.revision.created_entities = job_created;
        if !plan.deleted.is_empty() {
            let deleted: HashSet<EntityId> = plan.deleted.iter().copied().collect();
            plan.revision
                .created_entities
                .retain(|id| !deleted.contains(id));
        }
        plan.revision.placed_points = kept;
        Ok(plan)
    }
}

impl Command for ReadjustSurveyJobCommand {
    fn name(&self) -> &str {
        "READJUST_SURVEY_JOB"
    }

    fn validate(&self, context: &CommandContext<'_>) -> Status {
        let plan = self.build(context)?;
        *self.pending.borrow_mut() = Some(plan);
        Ok(())
    }

    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        let pending = self.pending.get_mut().take();
        let mut plan = match pending {
            Some(plan) => plan,
            None => self.build(context)?,
        };

        let mut created: Vec<EntityId> = Vec::new();
        if let Some(entities) = &mut plan.entities {
            entities.execute(context)?;
            // The new points follow the moved and deleted ones in the
            // command's created list; only the drawing part creates anything
            // - and, for a job finished again, the lines after them, which
            // placed_points passes over.
            created = entities.created_entities();
            let placed = match placed_points(
                &*context.model,
                &created,
                &plan.fresh.points,
                &plan.drawn,
                &plan.point_number_property,
            ) {
                Ok(placed) => placed,
                Err(error) => {
                    let _ = entities.undo(context); // all or nothing
                    return Err(error);
                }
            };
            if let Some(finish) = &plan.finish {
                let removed = lock(finish).earlier_lines_removed.clone();
                if !removed.is_empty() {
                    // The lines this run no longer strings are gone from the
                    // drawing, so they are no longer the job's: known only
                    // now, because which go is decided when the linework step
                    // runs. The ones it redrew are the entities they were, and
                    // stay listed where they are.
                    let gone: HashSet<EntityId> = removed.into_iter().collect();
                    plan.revision
                        .created_entities
                        .retain(|id| !gone.contains(id));
                }
            }
            plan.revision.created_entities.extend_from_slice(&created);
            plan.revision.placed_points.extend(placed);
        }
        if let Some(finish) = &plan.finish {
            report_finish(&mut plan.report, &lock(finish));
        }
        plan.revision.report_text = render_text(&plan.report);
        plan.revision.report_html = render_html(&plan.report);
        plan.revision.report_created_utc = if self.request.context.created_utc.is_empty() {
            now_utc()
        } else {
            self.request.context.created_utc.clone()
        };
        let job_id = &self.request.job_id;
        let revision = &mut plan.revision;
        let swapped = self
            .link
            .with(|state| -> Status {
                // The last of the id, as the C++ loop leaves it.
                let Some(job) = state
                    .survey_jobs
                    .iter_mut()
                    .rev()
                    .find(|job| job.id == *job_id)
                else {
                    // Cannot happen: build() found it and nothing ran since.
                    return Err(no_such_job(job_id));
                };
                revision.swap_with(job);
                // A longer report and more points may take a job the project
                // could just hold past what it can: refused, as the import
                // would be, rather than left to fail every later save.
                if let Err(error) =
                    ProjectStore::check_survey_job_size(job, ProjectStore::MAX_SURVEY_JOB_BYTES)
                {
                    revision.swap_with(job); // back as it was
                    return Err(error);
                }
                state.survey_jobs_generation += 1;
                Ok(())
            })
            .and_then(|status| status);
        if let Err(error) = swapped {
            if let Some(entities) = &mut plan.entities {
                let _ = entities.undo(context);
            }
            return Err(error);
        }
        self.other = plan.revision; // now the old revision
        self.entities = plan.entities;
        self.applied = true;
        // The drawing's points, needed by the reduction only.
        self.request.context = SurveyJobContext::default();
        self.read = None;
        let mut results = lock(&self.results.0);
        results.report = Some(plan.report);
        results.changes = plan.changes;
        results.finish = plan.finish;
        results.created = created;
        Ok(())
    }

    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        if !self.applied {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "the re-adjustment has not been done",
            ));
        }
        self.swap_revision(context, false)
    }

    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        if self.applied {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "the re-adjustment has not been undone",
            ));
        }
        self.swap_revision(context, true)
    }

    fn created_entities(&self) -> Vec<EntityId> {
        self.entities
            .as_ref()
            .map(|entities| entities.created_entities())
            .unwrap_or_default()
    }
}

impl ReadjustSurveyJobCommand {
    // Undo (`forward` false) or redo: the drawing part, then the job's
    // revision swapped back or in.
    fn swap_revision(&mut self, context: &mut CommandContext<'_>, forward: bool) -> Status {
        let job_id = &self.request.job_id;
        let Some(index) = self
            .link
            .with(|state| index_of_job(&state.survey_jobs, job_id))?
        else {
            return Err(no_such_job(job_id));
        };
        if let Some(entities) = &mut self.entities {
            if forward {
                entities.redo(context)?;
            } else {
                entities.undo(context)?;
            }
        }
        let other = &mut self.other;
        self.link.with(|state| {
            other.swap_with(&mut state.survey_jobs[index]);
            state.survey_jobs_generation += 1;
        })?;
        self.applied = forward;
        if forward {
            lock(&self.results.0).created = self.created_entities();
        }
        Ok(())
    }
}

// ---- Remove ------------------------------------------------------------------------

/// Takes a job off the document's list - its raw bytes, settings and report -
/// and, when `with_points`, deletes the entities it created that are still in
/// the drawing (whether or not the person has edited them since: they are
/// the job's). Without, the points stay as ordinary survey points. Undo puts
/// the job back in its place in the list, and the entities with their ids.
/// NotFound for a job the document does not have.
pub struct RemoveSurveyJobCommand {
    link: StateLink,
    job_id: String,
    with_points: bool,
    // None: no points to delete.
    entities: Option<CommandPtr>,
    // While removed.
    job: SurveyJob,
    index: usize,
    removed: bool,
}

impl RemoveSurveyJobCommand {
    pub fn new(document: &Document, job_id: impl Into<String>, with_points: bool) -> Self {
        Self {
            link: document.state_link(),
            job_id: job_id.into(),
            with_points,
            entities: None,
            job: SurveyJob::default(),
            index: 0,
            removed: false,
        }
    }

    fn build(&self, context: &CommandContext<'_>) -> Result<Option<CommandPtr>> {
        require_document_model(&self.link)?;
        let job_id = &self.job_id;
        let created = self.link.with(|state| {
            state
                .survey_jobs
                .iter()
                .find(|job| job.id == *job_id)
                .map(|job| job.created_entities.clone())
        })?;
        let Some(created) = created else {
            return Err(no_such_job(job_id));
        };
        if !self.with_points {
            return Ok(None);
        }
        let mut change = ChangeSet::default();
        for id in created {
            // One the person deleted is gone already.
            if context.model.entities.contains(id) {
                change.remove.push(id);
            }
        }
        if change.empty() {
            return Ok(None);
        }
        let command: CommandPtr = Box::new(ChangeSetCommand::new(
            "DELETE_SURVEY_JOB_POINTS",
            move |_: &CommandContext<'_>| Ok(change.clone()),
            true,
        ));
        command.validate(context)?;
        Ok(Some(command))
    }
}

impl Command for RemoveSurveyJobCommand {
    fn name(&self) -> &str {
        "REMOVE_SURVEY_JOB"
    }

    // The job's raw data goes with it.
    fn is_destructive(&self) -> bool {
        true
    }

    fn validate(&self, context: &CommandContext<'_>) -> Status {
        self.build(context).map(|_| ())
    }

    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.entities = self.build(context)?;
        if let Some(entities) = &mut self.entities {
            entities.execute(context)?;
        }
        let job_id = &self.job_id;
        let taken = self.link.with(|state| {
            let index = index_of_job(&state.survey_jobs, job_id)?;
            state.survey_jobs_generation += 1;
            Some((index, state.survey_jobs.remove(index)))
        })?;
        // build() found it and nothing ran since; the C++ takes it unchecked.
        let Some((index, job)) = taken else {
            if let Some(entities) = &mut self.entities {
                let _ = entities.undo(context);
            }
            return Err(no_such_job(&self.job_id));
        };
        self.index = index;
        self.job = job;
        self.removed = true;
        Ok(())
    }

    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        if !self.removed {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "the survey job has not been removed",
            ));
        }
        if let Some(entities) = &mut self.entities {
            entities.undo(context)?;
        }
        let job = std::mem::take(&mut self.job);
        let index = self.index;
        self.link.with(|state| {
            let at = index.min(state.survey_jobs.len());
            state.survey_jobs.insert(at, job);
            state.survey_jobs_generation += 1;
        })?;
        self.removed = false;
        Ok(())
    }

    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        if self.removed {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "the survey job removal has not been undone",
            ));
        }
        let job_id = &self.job_id;
        let Some(index) = self
            .link
            .with(|state| index_of_job(&state.survey_jobs, job_id))?
        else {
            return Err(no_such_job(job_id));
        };
        if let Some(entities) = &mut self.entities {
            entities.redo(context)?;
        }
        self.index = index;
        self.job = self.link.with(|state| {
            state.survey_jobs_generation += 1;
            state.survey_jobs.remove(index)
        })?;
        self.removed = true;
        Ok(())
    }
}
