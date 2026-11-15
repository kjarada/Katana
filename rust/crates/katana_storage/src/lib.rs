//! The `storage` layer: the project database (SQLite, schema 13).
//!
//! The port of src/katana_storage and include/katana/storage (docs/rust.md).
//! docs/storage.md says why the project file is SQLite, docs/model.md what
//! its schema holds.

mod ffi;

pub mod project_store;
pub mod sqlite_database;
pub mod survey_job;

pub use project_store::{
    ProjectContents, ProjectMetadata, ProjectStore, RecoveryReport, Relationship, SurfaceRecord,
    apply_to_model, apply_to_model_consuming, capture_model,
};
pub use sqlite_database::{
    BindValue, OpenMode, SqliteDatabase, SqliteStatement, SqliteTransaction, TextBytes,
};
pub use survey_job::{SurveyJob, SurveyJobFile, SurveyJobPoint};
