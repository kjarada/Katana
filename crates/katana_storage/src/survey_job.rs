//! A survey job: one imported field file, kept so that it can be reduced and
//! adjusted again. The port of include/katana/storage/survey_job.hpp.
//!
//! The value lives in storage, not in cad, because storage has to save it and
//! may not see cad (tools/check_layering.cmake); cad's SurveyJob is this type,
//! and the commands that create and change jobs are there. Plain values only:
//! nothing here reads a file or runs a reduction.
//!
//! Why the RAW BYTES are kept rather than the parsed project: "go back and
//! edit the adjustment" has to start from what was imported, not from what an
//! older reader made of it. Re-adjusting re-reads these bytes with the reader
//! the job names; if that reader has since been fixed, the job says so through
//! `parser_version`, and the report of the re-run shows the difference.

use katana_entity::entity::{EntityId, INVALID_ENTITY_ID};
use katana_survey::reduction_settings::ReductionSettings;

/// Another file the reader fetched while importing the job (a DBX job's other
/// files, a RINEX navigation file), exactly as read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SurveyJobFile {
    /// A file NAME, never a path (survey's source file name).
    pub name: String,
    pub bytes: Vec<u8>,
}

/// One point the job put on the drawing: which entity holds it and the
/// coordinates the job gave it, in the survey model's metres (northing,
/// easting; the drawing's y and x).
///
/// Kept on the job rather than read back from the entity because the entity
/// is the PERSON'S from the moment it is drawn: re-adjusting compares the two
/// to tell a point moved or re-levelled by hand since (whose edit the person
/// may want kept) from one the job may move, and a point deleted by hand from
/// one the new run no longer produces - neither of which the entity alone can
/// say.
#[derive(Clone, Debug, PartialEq)]
pub struct SurveyJobPoint {
    pub point_id: String,
    pub entity: EntityId,
    pub northing: f64,
    pub easting: f64,
    /// Absent is not zero.
    pub elevation: Option<f64>,
}

impl Default for SurveyJobPoint {
    fn default() -> Self {
        Self {
            point_id: String::new(),
            entity: INVALID_ENTITY_ID,
            northing: 0.0,
            easting: 0.0,
            elevation: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurveyJob {
    /// Unique within the project and never reused, so a report, a log line or
    /// an entity's metadata can name a job and still mean the same job after
    /// others were deleted. Assigned by the import command.
    pub id: String,
    /// Shown to the person; starts as the file name.
    pub name: String,
    /// The id of the surveyio format that read it.
    pub format_id: String,
    /// Of that reader, when this job was last read.
    pub parser_version: String,
    pub source_file_name: String,
    /// The whole file, exactly as imported.
    pub source_bytes: Vec<u8>,
    pub sibling_files: Vec<SurveyJobFile>,
    /// As last adjusted.
    pub settings: ReductionSettings,
    /// Where its points were put.
    pub layer: String,
    /// The entities the job put on the drawing (points and feature lines), in
    /// creation order. Re-adjusting moves these rather than making new ones,
    /// so a label or a dimension attached to a point survives it.
    pub created_entities: Vec<EntityId>,
    /// The last reduction report as rendered, and when it was made (ISO 8601).
    pub report_text: String,
    pub report_html: String,
    pub report_created_utc: String,
    pub imported_utc: String,
    /// The points among created_entities, one per point id, as the job last
    /// placed them (SurveyJobPoint). A point the person deleted by hand stays
    /// listed - the job still made it - so that re-adjusting can keep the
    /// deletion rather than silently draw the point again.
    pub placed_points: Vec<SurveyJobPoint>,
    /// How the import drew the job's points - a layer per field code or not,
    /// the property names, provenance - as versioned text written and read by
    /// cad (its survey import options without the layer, which is `layer`
    /// above). Kept so that a point a re-adjustment draws later goes where and
    /// as the job's other points went. Storage keeps it as it is and never
    /// reads it; empty means the defaults.
    pub import_options: String,
}
