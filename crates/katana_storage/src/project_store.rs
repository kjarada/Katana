//! Project persistence (PLAN.MD Phase 07). The port of
//! include/katana/storage/project_store.hpp and
//! src/katana_storage/project_store.cpp.
//!
//! A project is a directory:
//!
//! ```text
//! <name>.katana/
//! ├── project.db     structured data (SQLite): entities, layers, styles,
//! │                  property definitions, relationships, metadata
//! ├── backups/       timestamped snapshots of project.db
//! ├── terrain/       large datasets live in files, not in SQLite rows
//! ├── pointcloud/
//! ├── assets/
//! └── cache/         disposable; safe to delete
//! ```
//!
//! Guarantees:
//!
//! ```text
//! Atomic save    save() is one SQLite transaction. A crash or power loss mid
//!                save leaves the previous state intact (rollback journal).
//! Exact values   doubles round-trip bit for bit.
//! Versioning     PRAGMA user_version holds the schema version. Older projects
//!                are migrated step by step on open(), after an automatic
//!                backup. Projects from a newer Katana are refused, not guessed at.
//! Recovery       open() runs an integrity check; recover() restores the newest
//!                sound backup and keeps the damaged file for forensics.
//! ```
//!
//! save() rewrites the whole model. That is simple and correct; incremental
//! saves driven by command change events are a later optimisation (measure
//! first).
//!
//! Text. The model holds validated UTF-8 (docs/rust.md, "Text in the model is
//! validated UTF-8"), so a text column read into a `String` that is not UTF-8
//! fails the load (DatabaseFailure, naming the table and column), where the
//! C++ would carry the bytes on to whatever reads them next. The one record
//! whose text a later layer decodes itself - a surface's name, source and
//! style, which the drawing reads as Windows-1252 when they are not UTF-8 - is
//! kept as bytes (SurfaceRecord).
//!
//! Threading: a ProjectStore belongs to one thread.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use katana_core::error::{Error, ErrorCode, Result, Status, make_error};
use katana_core::path_text::path_to_utf8;
use katana_core::text::{equals_ignoring_case, format_fixed};
use katana_core::text_encoding::utf8_or_windows1252;
use katana_entity::annotation::{DEFAULT_TEXT_STYLE_NAME, label_kind_from_string};
use katana_entity::entity::{INVALID_ENTITY_ID, to_string};
use katana_entity::geometry_blob::{geometry_from_blob, geometry_to_blob};
use katana_entity::serialization::{
    geometry_from_json, label_style_definition_from_json, label_style_definition_to_json,
    properties_from_json, properties_to_json,
};
use katana_entity::tables::{
    CONTINUOUS_LINETYPE, DEFAULT_DIMENSION_STYLE_NAME, DEFAULT_LAYER_NAME, NO_HATCH,
    arrow_head_from_string,
};
use katana_entity::{
    Alignment, Color, DimensionStyle, Entity, EntityId, HatchLineFamily, HatchPattern, LabelRule,
    LabelStyle, Layer, Linetype, LinetypeElement, Model, PropertyDefinition, PropertyMap,
    PropertyType, Style, TextStyle, validate,
};
use katana_geometry::{AlignmentPi, HorizontalAlignment, Point2, ProfilePvi};
use katana_survey::reduction_settings::{parse_reduction_settings, serialise_reduction_settings};

use crate::sqlite_database::{
    BindValue, OpenMode, SqliteDatabase, SqliteStatement, SqliteTransaction, TextBytes,
};
use crate::survey_job::{SurveyJob, SurveyJobFile, SurveyJobPoint};

// ---- the persisted values ------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectMetadata {
    pub name: String,
    pub description: String,
    pub linear_unit: String,
    /// e.g. "EPSG:32630"; empty for local coordinates.
    pub coordinate_system: String,
    /// ISO 8601; filled in by create().
    pub created_utc: String,
    /// ISO 8601; refreshed by save().
    pub modified_utc: String,
    pub application_version: String,
    /// The customisation the drawing was drawn with: the NAMES of the
    /// linestyle, symbol and map files that were loaded, in load order -
    /// never their paths, because a project travels between machines and a
    /// path says whose disk it was made on. A record, not a reference: nothing
    /// is loaded from it, and a name here need not exist where the project is
    /// opened. Stored as one `customisation` key with the names separated by
    /// line feeds, which is why save() refuses a name that is empty or holds a
    /// line break or a path separator.
    pub customisation: Vec<String>,
    /// The reference layers the drawing is worked on top of - rasters and
    /// point clouds (interop's reference data) - as opaque records, one a
    /// layer, in their order: where each came from and how it is shown, never
    /// its pixels or points, which are read from the source again when the
    /// project opens. Storage keeps them and does not read them; the records
    /// are interop's to write and read. Stored as one `reference_layers` key, a
    /// record a line - no schema change, and a build that does not know the
    /// key keeps it (unknown_keys) - which is why save() refuses a record that
    /// is empty or holds a line break.
    pub reference_layers: Vec<String>,
    /// Keys a NEWER Katana wrote that this one does not read, kept and written
    /// back as they were, so opening and saving a project in an older build
    /// does not strip what a newer one recorded. Never a key this build reads:
    /// save() refuses one, rather than let it overwrite the field.
    pub unknown_keys: BTreeMap<String, String>,
}

impl Default for ProjectMetadata {
    fn default() -> Self {
        Self {
            name: String::from("Untitled"),
            description: String::new(),
            linear_unit: String::from("metre"),
            coordinate_system: String::new(),
            created_utc: String::new(),
            modified_utc: String::new(),
            application_version: String::new(),
            customisation: Vec::new(),
            reference_layers: Vec::new(),
            unknown_keys: BTreeMap::new(),
        }
    }
}

/// Directed, typed link between two entities (a dimension and what it
/// measures, a label and its subject...). Rows disappear with either entity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Relationship {
    pub from: EntityId,
    pub to: EntityId,
    pub kind: String,
    pub data: String,
}

/// One TIN surface of the drawing, as the project keeps it (schema 13;
/// docs/terrain.md, "Surfaces in the drawing"). OPAQUE to this layer, which
/// may not see the terrain layer (tools/check_layering.cmake): `data` is the
/// surface's bytes as terrain's tin_codec writes them, and the Document (cad)
/// turns a record into a surface and back. The rest is what a person sees of
/// it: its name, where it came from, the display style it is drawn in (empty:
/// the drawing's default) and whether the views draw it.
///
/// The name, source and style are bytes, as the C++'s strings are: the
/// drawing reads them, and repairs one an earlier build stored in a Windows
/// code page (Windows-1252) rather than refusing the project, so they reach
/// it as they were stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceRecord {
    pub name: Vec<u8>,
    pub source: Vec<u8>,
    pub style: Vec<u8>,
    pub visible: bool,
    pub data: Vec<u8>,
}

impl Default for SurfaceRecord {
    fn default() -> Self {
        Self {
            name: Vec::new(),
            source: Vec::new(),
            style: Vec::new(),
            visible: true,
            data: Vec::new(),
        }
    }
}

/// Everything persisted for a project, as plain values.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectContents {
    pub metadata: ProjectMetadata,
    pub layers: Vec<Layer>,
    pub styles: Vec<Style>,
    pub linetypes: Vec<Linetype>,
    pub dimension_styles: Vec<DimensionStyle>,
    pub hatch_patterns: Vec<HatchPattern>,
    pub alignments: Vec<Alignment>,
    pub property_definitions: Vec<PropertyDefinition>,
    /// The annotation tables, schema 11 (entity's annotation module).
    pub text_styles: Vec<TextStyle>,
    pub label_styles: Vec<LabelStyle>,
    pub label_rules: Vec<LabelRule>,
    pub entities: Vec<Entity>,
    pub relationships: Vec<Relationship>,
    pub next_entity_id: EntityId,
    /// The survey jobs (survey_job.rs), in the order they were created. Saved
    /// and loaded in the same transaction as the entities they created, so a
    /// job can never name entities a crash left unsaved. Schema version 10.
    /// save() refuses a job without an id, two jobs of one id, and a job too
    /// large for the database (ProjectStore::check_survey_job_size); a project
    /// from before schema 10 loads with none.
    pub survey_jobs: Vec<SurveyJob>,
    /// The drawing's surfaces, in their order (SurfaceRecord), saved and
    /// loaded in the same transaction as everything else. Schema version 13.
    /// save() refuses a surface with no name or no bytes, two of one name
    /// compared without case (the drawing's store compares them so), and one
    /// too large for the database (ProjectStore::check_surface_size); a
    /// project from before schema 13 loads with none.
    pub surfaces: Vec<SurfaceRecord>,
}

impl Default for ProjectContents {
    fn default() -> Self {
        Self {
            metadata: ProjectMetadata::default(),
            layers: Vec::new(),
            styles: Vec::new(),
            linetypes: Vec::new(),
            dimension_styles: Vec::new(),
            hatch_patterns: Vec::new(),
            alignments: Vec::new(),
            property_definitions: Vec::new(),
            text_styles: Vec::new(),
            label_styles: Vec::new(),
            label_rules: Vec::new(),
            entities: Vec::new(),
            relationships: Vec::new(),
            next_entity_id: 1,
            survey_jobs: Vec::new(),
            surfaces: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    /// false: project.db was already sound.
    pub restored: bool,
    /// The backup that was used.
    pub restored_from: PathBuf,
    /// Where the corrupt database went.
    pub damaged_file_kept_as: PathBuf,
    /// What the integrity check reported.
    pub problems: String,
}

// ---- the schema ----------------------------------------------------------------------

/// "KTNA" - marks a SQLite file as a Katana project (PRAGMA application_id).
const APPLICATION_ID: i32 = 0x4B54_4E41;
const DATABASE_FILE: &str = "project.db";
const BACKUP_DIRECTORY: &str = "backups";
const DATA_DIRECTORIES: [&str; 4] = ["terrain", "pointcloud", "assets", "cache"];

struct Migration {
    to_version: i32,
    sql: &'static str,
}

// Append only. Never edit a released migration: projects in the field have
// already run it. Schema changes are new entries.
//
// The text is the C++'s byte for byte, indentation and all: SQLite keeps each
// CREATE statement in sqlite_master as written, so a project made by either
// program has the same schema text.
const MIGRATIONS: [Migration; 13] = [
    Migration {
        to_version: 1,
        sql: r#"
        CREATE TABLE metadata (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        ) WITHOUT ROWID;

        CREATE TABLE layers (
            name        TEXT PRIMARY KEY,
            color       TEXT NOT NULL,
            visible     INTEGER NOT NULL,
            locked      INTEGER NOT NULL,
            linetype    TEXT NOT NULL,
            line_weight REAL NOT NULL
        ) WITHOUT ROWID;

        CREATE TABLE styles (
            name        TEXT PRIMARY KEY,
            color       TEXT,
            line_weight REAL NOT NULL,
            linetype    TEXT NOT NULL
        ) WITHOUT ROWID;

        CREATE TABLE property_definitions (
            name          TEXT PRIMARY KEY,
            type          TEXT NOT NULL,
            description   TEXT NOT NULL,
            default_value TEXT
        ) WITHOUT ROWID;

        CREATE TABLE entities (
            id         INTEGER PRIMARY KEY,
            type       TEXT NOT NULL,
            layer      TEXT NOT NULL REFERENCES layers(name),
            style      TEXT NOT NULL,
            color      TEXT,
            visible    INTEGER NOT NULL,
            geometry   TEXT NOT NULL,
            properties TEXT NOT NULL,
            metadata   TEXT NOT NULL
        );
        CREATE INDEX entities_by_layer ON entities(layer);
        CREATE INDEX entities_by_type ON entities(type);
    "#,
    },
    Migration {
        to_version: 2,
        sql: r#"
        CREATE TABLE relationships (
            id          INTEGER PRIMARY KEY,
            from_entity INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
            to_entity   INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
            kind        TEXT NOT NULL,
            data        TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX relationships_by_from ON relationships(from_entity);
        CREATE INDEX relationships_by_to ON relationships(to_entity);
    "#,
    },
    // Geometry as a binary blob instead of JSON text. Parsing the JSON was 602
    // of the 891 ms it took to open a 50 000-entity project - 68%, against 289
    // ms for SQLite itself - which is why the encoding changed and the
    // database did not (docs/storage.md).
    //
    // The column is ADDED rather than replacing `geometry`, and is NULL for
    // every row written before this migration. A load reads the blob when
    // there is one and falls back to the JSON when there is not, so an
    // existing project opens untouched and is converted the next time it is
    // saved. The old column stays until a migration that rewrites every row
    // can be justified; dropping it now would mean rewriting the whole table
    // on open.
    Migration {
        to_version: 3,
        sql: r#"
        ALTER TABLE entities ADD COLUMN geometry_blob BLOB;
    "#,
    },
    // Linetype definitions. The elements go in their own table rather than an
    // encoded string in one column, because the order matters and a delimited
    // list of signed doubles is a parser waiting to be written twice.
    //
    // Layer::linetype and Style::linetype have always been free strings with
    // no table behind them, so an existing project may name a pattern that
    // does not exist. That is NOT repaired here: resolution treats an unknown
    // linetype as continuous, which is how those projects already drew, and
    // inventing an empty definition for every name found would shadow a real
    // built-in of the same name for ever after.
    Migration {
        to_version: 4,
        sql: r#"
        CREATE TABLE linetypes (
            name        TEXT PRIMARY KEY,
            description TEXT NOT NULL
        ) WITHOUT ROWID;

        CREATE TABLE linetype_elements (
            linetype TEXT NOT NULL REFERENCES linetypes(name) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            length   REAL NOT NULL,
            PRIMARY KEY (linetype, position)
        ) WITHOUT ROWID;
    "#,
    },
    // Dimension styles, and the layer column that names one. A layer with an
    // empty dimension_style uses the document default, which is how every
    // project written before this migration behaves - so nothing is invented
    // for existing rows.
    Migration {
        to_version: 5,
        sql: r#"
        CREATE TABLE dimension_styles (
            name              TEXT PRIMARY KEY,
            text_height       REAL NOT NULL,
            text_gap          REAL NOT NULL,
            extension_offset  REAL NOT NULL,
            extension_beyond  REAL NOT NULL,
            arrow_size        REAL NOT NULL,
            arrow_head        TEXT NOT NULL,
            unit_scale        REAL NOT NULL,
            prefix            TEXT NOT NULL,
            suffix            TEXT NOT NULL,
            decimals          INTEGER NOT NULL,
            round_to          REAL NOT NULL,
            suppress_zeros    INTEGER NOT NULL
        ) WITHOUT ROWID;

        ALTER TABLE layers ADD COLUMN dimension_style TEXT NOT NULL DEFAULT '';
    "#,
    },
    // Hatch patterns, and the layer and style columns that name one. The
    // layer column defaults to the built-in that draws no fill and the style
    // column to empty, which means ByLayer - so every project written before
    // this migration keeps drawing exactly as it did, and nothing acquires a
    // hatch it never asked for.
    Migration {
        to_version: 6,
        sql: r#"
        CREATE TABLE hatch_patterns (
            name        TEXT PRIMARY KEY,
            description TEXT NOT NULL,
            solid       INTEGER NOT NULL
        ) WITHOUT ROWID;

        CREATE TABLE hatch_families (
            pattern  TEXT NOT NULL REFERENCES hatch_patterns(name) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            angle    REAL NOT NULL,
            spacing  REAL NOT NULL,
            line_offset REAL NOT NULL,
            PRIMARY KEY (pattern, position)
        ) WITHOUT ROWID;

        ALTER TABLE layers ADD COLUMN hatch_pattern TEXT NOT NULL DEFAULT 'none';
        ALTER TABLE styles ADD COLUMN hatch_pattern TEXT NOT NULL DEFAULT '';
    "#,
    },
    // Alignments, stored as the PI definition and nothing else. The elements
    // are derived on load by geometry's solve_alignment, so a file can never
    // hold an alignment with a gap in it, and a better solver later applies to
    // every existing file for free.
    Migration {
        to_version: 7,
        sql: r#"
        CREATE TABLE alignments (
            name          TEXT PRIMARY KEY,
            description   TEXT NOT NULL,
            start_station REAL NOT NULL
        ) WITHOUT ROWID;

        CREATE TABLE alignment_pis (
            alignment  TEXT NOT NULL REFERENCES alignments(name) ON DELETE CASCADE,
            position   INTEGER NOT NULL,
            x          REAL NOT NULL,
            y          REAL NOT NULL,
            radius     REAL NOT NULL,
            spiral_in  REAL NOT NULL,
            spiral_out REAL NOT NULL,
            PRIMARY KEY (alignment, position)
        ) WITHOUT ROWID;
    "#,
    },
    // The design profile on an alignment, as its PVIs. An alignment with no
    // rows here has no profile - which is what every alignment written before
    // this migration has, so nothing is invented.
    Migration {
        to_version: 8,
        sql: r#"
        CREATE TABLE alignment_pvis (
            alignment    TEXT NOT NULL REFERENCES alignments(name) ON DELETE CASCADE,
            position     INTEGER NOT NULL,
            station      REAL NOT NULL,
            elevation    REAL NOT NULL,
            curve_length REAL NOT NULL,
            PRIMARY KEY (alignment, position)
        ) WITHOUT ROWID;
    "#,
    },
    // A style's description and, for points, its symbol (PLAN.MD 20.2). The
    // defaults are exactly what every style written before this had: no
    // description, no symbol, the viewport's own mark size - so an old drawing
    // draws as it did.
    Migration {
        to_version: 9,
        sql: r#"
        ALTER TABLE styles ADD COLUMN description TEXT NOT NULL DEFAULT '';
        ALTER TABLE styles ADD COLUMN symbol TEXT NOT NULL DEFAULT '';
        ALTER TABLE styles ADD COLUMN symbol_size REAL NOT NULL DEFAULT 0;
    "#,
    },
    // Survey jobs (survey_job.rs): each imported field file kept whole, so its
    // reduction and adjustment can be revisited. Two new tables and nothing
    // else changed, so a project written before this migration opens exactly
    // as it was, with no jobs.
    //
    // The file and its siblings are BLOBs, never TEXT: a field file is bytes
    // (a DBX job is binary, a GSI file may be in any code page) and a TEXT
    // column would invite SQLite or a reader to re-encode it. The settings are
    // their versioned text form (serialise_reduction_settings), so a later
    // setting needs no migration, and so are the import's drawing options (cad
    // writes and reads that text; storage only keeps it). The entity ids and
    // the placed points are packed binary (see encode_entity_ids /
    // encode_placed_points): a job of a hundred thousand points is one row,
    // not a hundred thousand. Ordinary rowid tables, because a WITHOUT ROWID
    // table stores a whole row in its b-tree and suits small rows, not a 50 MB
    // file.
    Migration {
        to_version: 10,
        sql: r#"
        CREATE TABLE survey_jobs (
            position           INTEGER PRIMARY KEY,
            id                 TEXT NOT NULL UNIQUE,
            name               TEXT NOT NULL,
            format_id          TEXT NOT NULL,
            parser_version     TEXT NOT NULL,
            source_file_name   TEXT NOT NULL,
            source_bytes       BLOB NOT NULL,
            settings           TEXT NOT NULL,
            layer              TEXT NOT NULL,
            created_entities   BLOB NOT NULL,
            placed_points      BLOB NOT NULL,
            report_text        TEXT NOT NULL,
            report_html        TEXT NOT NULL,
            report_created_utc TEXT NOT NULL,
            imported_utc       TEXT NOT NULL,
            import_options     TEXT NOT NULL
        );

        CREATE TABLE survey_job_files (
            job      TEXT NOT NULL REFERENCES survey_jobs(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            name     TEXT NOT NULL,
            bytes    BLOB NOT NULL,
            UNIQUE (job, position)
        );
    "#,
    },
    // The annotation tables (entity's annotation module, docs/annotation.md):
    // text styles, label styles and the auto-label rules, and a dimension
    // style's paper sizing. Three new tables, empty in a project written
    // before this, and a column whose default - model units - is what every
    // dimension style was; the built-in "Standard" text style is seeded by the
    // model, as the default dimension style is, not stored until it is
    // changed. So a project from before this migration opens exactly as it
    // was.
    //
    // A text style is columns, one per field: it is a fixed set a DXF STYLE
    // row also has. A label style keeps its name and kind as columns and the
    // rest - the template and a dozen placement options likely to grow - as
    // versioned JSON (serialization's label_style_definition_to_json), the way
    // a survey job keeps its settings, so a later option needs no migration.
    Migration {
        to_version: 11,
        sql: r#"
        CREATE TABLE text_styles (
            name         TEXT PRIMARY KEY,
            font_family  TEXT NOT NULL,
            paper_height REAL NOT NULL,
            width_factor REAL NOT NULL,
            oblique      REAL NOT NULL,
            bold         INTEGER NOT NULL,
            italic       INTEGER NOT NULL,
            color        TEXT NOT NULL,
            mask         INTEGER NOT NULL,
            mask_margin  REAL NOT NULL,
            readable     INTEGER NOT NULL,
            line_spacing REAL NOT NULL
        ) WITHOUT ROWID;

        CREATE TABLE label_styles (
            name       TEXT PRIMARY KEY,
            kind       TEXT NOT NULL,
            definition TEXT NOT NULL
        ) WITHOUT ROWID;

        CREATE TABLE label_rules (
            name        TEXT PRIMARY KEY,
            label_style TEXT NOT NULL,
            layer       TEXT NOT NULL,
            code        TEXT NOT NULL,
            entity_type TEXT NOT NULL,
            label_layer TEXT NOT NULL,
            enabled     INTEGER NOT NULL
        ) WITHOUT ROWID;

        ALTER TABLE dimension_styles ADD COLUMN paper_sized INTEGER NOT NULL DEFAULT 0;
    "#,
    },
    // The drawing system's geometry kinds (docs/drawing.md): CurvePolyline 9,
    // Ellipse 10 and Spline 11 in the entities' geometry blob. No table or
    // column changes - the blob column holds them as it holds every kind - so
    // a project from before this opens exactly as it was. The version moves so
    // that a build which cannot read those kinds refuses the whole project up
    // front as one written by a newer Katana, instead of opening it and
    // failing row by row with "unknown geometry kind in blob", which reads as
    // corruption rather than version skew (docs/model.md).
    //
    // Every project a build of this schema opens is raised to it, whether or
    // not it holds one of the kinds: the file has one version, as for every
    // migration before this. Writing 11 or 12 by what a save happened to
    // contain was rejected - it would be a second versioning rule beside this
    // table, and a project would change version with its contents.
    Migration {
        to_version: 12,
        sql: r#"
        -- The geometry kinds 9 to 11 need no schema change of their own.
    "#,
    },
    // The drawing's TIN surfaces (SurfaceRecord; docs/terrain.md, "Surfaces
    // in the drawing"). One new table and nothing else changed, so a project
    // written before this opens exactly as it was, with no surfaces.
    //
    // A surface is one row, its triangulation one BLOB in the terrain layer's
    // own versioned form (terrain's tin_codec), which this layer keeps without
    // reading: storage may not see terrain, and a later surface layout is then
    // a new codec version, not a migration. An ordinary rowid table, for the
    // reason the survey jobs' is one: a WITHOUT ROWID table keeps a whole row
    // in its b-tree, which suits small rows, not a surface of tens of
    // megabytes.
    //
    // Rejected: the surfaces as files in the project's terrain/ folder. A save
    // is ONE SQLite transaction ("Atomic save" above), and files beside it
    // would be written outside it - a crash between the two leaves a database
    // naming surfaces whose files are older, newer or gone.
    Migration {
        to_version: 13,
        sql: r#"
        CREATE TABLE surfaces (
            position INTEGER PRIMARY KEY,
            name     TEXT NOT NULL,
            source   TEXT NOT NULL,
            style    TEXT NOT NULL,
            visible  INTEGER NOT NULL,
            data     BLOB NOT NULL
        );
    "#,
    },
];

// ---- small helpers -------------------------------------------------------------------

/// A path as the C++'s `path.u8string()` gives it, for a message.
fn path_text(path: &Path) -> String {
    utf8_or_windows1252(path_to_utf8(path))
}

/// Bytes the C++ puts straight into a message, as text.
fn shown(bytes: &[u8]) -> String {
    utf8_or_windows1252(bytes)
}

/// The C++'s `std::error_code::message()` for a failed file operation: the
/// system's own sentence, which Rust's io::Error shows followed by
/// " (os error N)".
fn system_message(error: &std::io::Error) -> String {
    let text = format!("{error}");
    if let Some(code) = error.raw_os_error()
        && let Some(sentence) = text.strip_suffix(&format!(" (os error {code})"))
    {
        return sentence.to_owned();
    }
    text
}

/// Howard Hinnant's days_from_civil inverse: the proleptic Gregorian date of
/// a count of days since 1970-01-01, which the C++ gets from gmtime_r.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// `compact` gives a file-name-safe form: 20260919T101530-123.
fn utc_timestamp(compact: bool) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = now.as_secs() as i64;
    let millis = now.subsec_millis();
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let of_day = seconds.rem_euclid(86_400);
    let (hour, minute, second) = (of_day / 3600, of_day % 3600 / 60, of_day % 60);
    if compact {
        format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}-{millis:03}")
    } else {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
    }
}

fn property_type_name(r#type: PropertyType) -> &'static str {
    match r#type {
        PropertyType::Boolean => "Boolean",
        PropertyType::Integer => "Integer",
        PropertyType::Real => "Real",
        PropertyType::Text => "Text",
    }
}

fn property_type_from_string(text: &str) -> Result<PropertyType> {
    for r#type in [
        PropertyType::Boolean,
        PropertyType::Integer,
        PropertyType::Real,
        PropertyType::Text,
    ] {
        if property_type_name(r#type) == text {
            return Ok(r#type);
        }
    }
    Err(make_error(
        ErrorCode::DatabaseFailure,
        "unknown property type in project",
        text,
    ))
}

/// `std::stoull(text)`: leading C-locale white space, an optional sign (a
/// minus wraps, as strtoull's does), then base-10 digits up to the first that
/// is not one. None where it throws: no digits, or a value past u64.
fn stoull(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() && matches!(bytes[at], b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r') {
        at += 1;
    }
    let mut negative = false;
    if at < bytes.len() && (bytes[at] == b'+' || bytes[at] == b'-') {
        negative = bytes[at] == b'-';
        at += 1;
    }
    let start = at;
    let mut value: u64 = 0;
    let mut overflow = false;
    while at < bytes.len() && bytes[at].is_ascii_digit() {
        match value
            .checked_mul(10)
            .and_then(|tens| tens.checked_add(u64::from(bytes[at] - b'0')))
        {
            Some(next) => value = next,
            None => overflow = true,
        }
        at += 1;
    }
    if at == start || overflow {
        return None;
    }
    Some(if negative {
        value.wrapping_neg()
    } else {
        value
    })
}

/// The names joined one a line, as the C++'s loop joins them: a line feed
/// before each name once the text is not empty.
fn joined_lines(items: &[String]) -> String {
    let mut text = String::new();
    for item in items {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(item);
    }
    text
}

/// The lines of a key's value, as the C++ splits them: a part per line feed,
/// and none after a final one.
fn split_lines(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut start = 0;
    while start < value.len() {
        let end = value[start..]
            .find('\n')
            .map_or(value.len(), |offset| start + offset);
        parts.push(value[start..end].to_owned());
        start = end + 1;
    }
    parts
}

fn not_utf8(table: &str, column: i32) -> Error {
    make_error(
        ErrorCode::DatabaseFailure,
        "the project holds text that is not UTF-8",
        format!("table={table} column={column}"),
    )
}

/// A text column as the model holds it: validated UTF-8.
fn text_column(row: &mut SqliteStatement<'_>, column: i32, table: &str) -> Result<String> {
    String::from_utf8(row.column_text(column)).map_err(|_| not_utf8(table, column))
}

/// The same, borrowed from the row for a value parsed and discarded within it
/// (SqliteStatement::column_text_view says how long it lives).
fn text_column_view<'r>(
    row: &'r mut SqliteStatement<'_>,
    column: i32,
    table: &str,
) -> Result<&'r str> {
    std::str::from_utf8(row.column_text_view(column)).map_err(|_| not_utf8(table, column))
}

/// A nullable colour column.
fn read_color(row: &mut SqliteStatement<'_>, column: i32, table: &str) -> Result<Option<Color>> {
    if row.column_is_null(column) {
        return Ok(None);
    }
    Ok(Some(Color::from_hex(text_column_view(
        row, column, table,
    )?)?))
}

// ---- validation ----------------------------------------------------------------------

// Every metadata key this build reads and writes. Any other key in a project
// was written by a newer build, and is kept (unknown_keys).
const METADATA_KEYS: [&str; 10] = [
    "name",
    "description",
    "linear_unit",
    "coordinate_system",
    "created_utc",
    "modified_utc",
    "application_version",
    "next_entity_id",
    "customisation",
    "reference_layers",
];

// The keys the drawing above storage keeps in unknown_keys as text storage
// does not read - each the sheet-set pattern (cad's plotting sheet store): a
// document's data with no schema change. They are this build's, so one absent
// at a save was removed and its row is deleted (write_metadata); a key in
// neither list is a newer build's and is never deleted. A module that adds
// such a key adds it here (docs/model.md, "Metadata a newer build wrote").
const DRAWING_METADATA_KEYS: [&str; 6] = [
    "annotation_scale",
    "sheets",
    "surface_styles",
    "road_templates",
    "corridors",
    "road_objects",
];

fn is_metadata_key(key: &str) -> bool {
    METADATA_KEYS.contains(&key)
}

// Shared by save() and apply_to_model(): a project that fails this is never
// written and never loaded into a model.
//
// The customisation names are stored one per line (see ProjectMetadata), so a
// name that would split or vanish on the way back is refused here rather than
// read back as a different list.
fn validate_metadata(metadata: &ProjectMetadata) -> Status {
    for name in &metadata.customisation {
        if name.is_empty() || name.contains(['\n', '\r', '/', '\\']) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a customisation entry must be a file name: not empty, and with no line break or path separator",
                format!("customisation=\"{name}\""),
            ));
        }
    }
    // A reference layer's record is one line of the key (see
    // ProjectMetadata): an empty one, or one that would split, would not read
    // back as itself.
    for record in &metadata.reference_layers {
        if record.is_empty() || record.contains(['\n', '\r']) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a reference layer's record must be one line, not empty",
                format!("reference_layers=\"{record}\""),
            ));
        }
    }
    for key in metadata.unknown_keys.keys() {
        if is_metadata_key(key) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "an unknown metadata key names a field this build writes itself",
                format!("key={key}"),
            ));
        }
    }
    Ok(())
}

fn validate_contents(contents: &ProjectContents) -> Status {
    validate_metadata(&contents.metadata)?;
    let mut layer_names: BTreeSet<&str> = BTreeSet::from([DEFAULT_LAYER_NAME]);
    for layer in &contents.layers {
        if layer.name.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "project contains a layer without a name",
            ));
        }
        layer_names.insert(layer.name.as_str());
    }
    let style_names: BTreeSet<&str> = contents
        .styles
        .iter()
        .map(|style| style.name.as_str())
        .collect();
    let mut ids: BTreeSet<EntityId> = BTreeSet::new();
    let mut highest: EntityId = 0;
    for entity in &contents.entities {
        let context = format!("id={}", entity.id);
        if entity.id == INVALID_ENTITY_ID || !ids.insert(entity.id) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "entity id is missing or duplicated",
                context,
            ));
        }
        if !layer_names.contains(entity.layer.as_str()) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "entity refers to an unknown layer",
                format!("{context} layer={}", entity.layer),
            ));
        }
        if !entity.style.is_empty() && !style_names.contains(entity.style.as_str()) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "entity refers to an unknown style",
                format!("{context} style={}", entity.style),
            ));
        }
        if let Err(error) = validate(&entity.geometry) {
            return Err(make_error(error.code, error.message, context));
        }
        highest = highest.max(entity.id);
    }
    if contents.next_entity_id <= highest {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "next entity id would collide with an existing entity",
            format!("next={}", contents.next_entity_id),
        ));
    }
    for relationship in &contents.relationships {
        if !ids.contains(&relationship.from) || !ids.contains(&relationship.to) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "relationship refers to an entity that is not in the project",
                format!("kind={}", relationship.kind),
            ));
        }
    }
    Ok(())
}

fn migrate(database: &SqliteDatabase, from_version: i32) -> Status {
    for migration in &MIGRATIONS {
        if migration.to_version <= from_version {
            continue;
        }
        let mut transaction = SqliteTransaction::begin(database)?;
        if let Err(error) = database.execute(migration.sql) {
            return Err(make_error(
                ErrorCode::DatabaseFailure,
                "schema migration failed",
                format!("to={} {}", migration.to_version, error.context),
            ));
        }
        database.set_user_version(migration.to_version)?;
        transaction.commit()?;
    }
    Ok(())
}

// A database that opens, carries the Katana application id and passes the
// integrity check. Returns the problem description otherwise.
fn inspect_database(file: &Path) -> Result<Option<String>> {
    // Read-WRITE, not read-only. A database whose process was killed
    // mid-commit has a hot rollback journal beside it, and SQLite replays that
    // journal when the file is next opened for writing - after which it is
    // perfectly sound. A read-only connection cannot replay it and fails with
    // SQLITE_READONLY_ROLLBACK, which is indistinguishable from real
    // corruption here: recover() would then throw away a recoverable database
    // and restore an older backup, losing every change since.
    //
    // Read-only is still tried as a fallback, for genuinely unwritable media.
    let database = match SqliteDatabase::open(file, OpenMode::ReadWrite) {
        Ok(database) => database,
        Err(_) => match SqliteDatabase::open(file, OpenMode::ReadOnly) {
            Ok(database) => database,
            Err(error) => return Ok(Some(error.describe())),
        },
    };
    let application_id = match database.application_id() {
        Ok(application_id) => application_id,
        Err(error) => return Ok(Some(error.describe())),
    };
    if application_id != i64::from(APPLICATION_ID) {
        return Ok(Some(String::from("not a Katana project database")));
    }
    match database.integrity_problems() {
        Ok(problems) => Ok(problems),
        Err(error) => Ok(Some(error.describe())),
    }
}

// Backups are written as "project-<compact UTC>.db", and "project-<compact
// UTC>-<n>.db" when several land in the same second. Parsing that back into
// (timestamp, sequence) is what lets them be ordered correctly; ordering by
// filename alone gets the collision case BACKWARDS, because '.' (0x2E) sorts
// above '-' (0x2D) and so "...T120000.db" outranks the later
// "...T120000-1.db".
struct BackupName {
    timestamp: String,
    sequence: i64,
}

fn parse_backup_name(file: &Path) -> Option<BackupName> {
    const PREFIX: &str = "project-";
    // utc_timestamp(true) produces "YYYYMMDDTHHMMSS-mmm" - fixed width, and it
    // ALREADY contains a dash, before the milliseconds. So the collision
    // suffix cannot be found by looking for the first '-' (that finds the
    // milliseconds) nor safely by the last one. The timestamp is a known
    // shape, so it is matched as one and whatever follows must be the suffix.
    const TIMESTAMP_LENGTH: usize = 19; // YYYYMMDDTHHMMSS-mmm
    // A stem that is not Unicode cannot match the ASCII shape below, as the
    // C++'s bytes cannot.
    let stem = file.file_stem()?.to_str()?;
    if file.extension() != Some(OsStr::new("db"))
        || stem.len() < PREFIX.len() + TIMESTAMP_LENGTH
        || !stem.starts_with(PREFIX)
    {
        return None;
    }
    let rest = &stem[PREFIX.len()..];
    let bytes = rest.as_bytes();
    for (i, &ch) in bytes.iter().enumerate().take(TIMESTAMP_LENGTH) {
        let valid = match i {
            8 => ch == b'T',
            15 => ch == b'-',
            _ => ch.is_ascii_digit(),
        };
        if !valid {
            return None;
        }
    }
    // The first TIMESTAMP_LENGTH bytes are ASCII, so these are character
    // boundaries.
    let mut parsed = BackupName {
        timestamp: rest[..TIMESTAMP_LENGTH].to_owned(),
        sequence: 0,
    };

    if rest.len() > TIMESTAMP_LENGTH {
        // Only "-<digits>", the suffix added when two backups land in the same
        // millisecond, may follow.
        if bytes[TIMESTAMP_LENGTH] != b'-' {
            return None;
        }
        let sequence = &rest[TIMESTAMP_LENGTH + 1..];
        if sequence.is_empty() || !sequence.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        // std::stol throws std::out_of_range for a suffix past a long, and
        // nothing catches it: a stray file of that name in backups/ ends the
        // program. Ported as it is (docs/rust.md, "Not done").
        parsed.sequence = sequence
            .parse::<i64>()
            .expect("std::stol: the backup's sequence is out of range");
    }
    Some(parsed)
}

fn backups_newest_first(project_directory: &Path) -> Vec<PathBuf> {
    // Only files this store actually wrote are considered. Any other .db
    // dropped into backups/ - a copy someone made by hand, an editor's stray
    // save - would otherwise be offered as a restore candidate and could
    // outrank every real snapshot on name alone.
    let mut backups: Vec<(BackupName, PathBuf)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(project_directory.join(BACKUP_DIRECTORY)) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if let Some(parsed) = parse_backup_name(&path) {
                backups.push((parsed, path));
            }
        }
    }
    backups.sort_by(|a, b| {
        b.0.timestamp
            .cmp(&a.0.timestamp)
            .then_with(|| b.0.sequence.cmp(&a.0.sequence))
    });
    backups.into_iter().map(|(_, path)| path).collect()
}

// ---- model <-> contents --------------------------------------------------------------

pub fn capture_model(
    model: &Model,
    metadata: ProjectMetadata,
    relationships: Vec<Relationship>,
) -> ProjectContents {
    let mut entities = Vec::with_capacity(model.entities.size());
    model
        .entities
        .for_each(|entity| entities.push(entity.clone()));
    ProjectContents {
        metadata,
        layers: model.layers.all(),
        styles: model.styles.all(),
        linetypes: model.linetypes.all(),
        dimension_styles: model.dimension_styles.all(),
        hatch_patterns: model.hatch_patterns.all(),
        alignments: model.alignments.all(),
        property_definitions: model.properties.all(),
        text_styles: model.text_styles.all(),
        label_styles: model.label_styles.all(),
        label_rules: model.label_rules.all(),
        entities,
        relationships,
        next_entity_id: model.entities.next_id(),
        survey_jobs: Vec::new(),
        surfaces: Vec::new(),
    }
}

/// One table's records on their way into the model: borrowed and copied one
/// at a time for the copying apply, moved for the consuming one. The C++'s
/// `take` (the identity or std::move), as an iterator of `Cow`.
enum Items<'a, T: Clone + 'a> {
    Borrowed(std::slice::Iter<'a, T>),
    Owned(std::vec::IntoIter<T>),
}

impl<'a, T: Clone + 'a> Items<'a, T> {
    fn borrowed(items: &'a [T]) -> Self {
        Items::Borrowed(items.iter())
    }

    fn owned(items: Vec<T>) -> Self {
        Items::Owned(items.into_iter())
    }
}

impl<'a, T: Clone + 'a> Iterator for Items<'a, T> {
    type Item = Cow<'a, T>;

    fn next(&mut self) -> Option<Cow<'a, T>> {
        match self {
            Items::Borrowed(items) => items.next().map(Cow::Borrowed),
            Items::Owned(items) => items.next().map(Cow::Owned),
        }
    }
}

/// What apply_contents reads of a ProjectContents.
struct Staging<'a> {
    layers: Items<'a, Layer>,
    styles: Items<'a, Style>,
    linetypes: Items<'a, Linetype>,
    dimension_styles: Items<'a, DimensionStyle>,
    hatch_patterns: Items<'a, HatchPattern>,
    alignments: Items<'a, Alignment>,
    property_definitions: Items<'a, PropertyDefinition>,
    text_styles: Items<'a, TextStyle>,
    label_styles: Items<'a, LabelStyle>,
    label_rules: Items<'a, LabelRule>,
    entities: Items<'a, Entity>,
    next_entity_id: EntityId,
}

// One body for both apply_to_model functions. The staging hands out borrowed
// records for the copying one and owned records for the consuming one, so the
// loops below read the same either way and neither copies more than its
// caller asked for.
//
// One function rather than two because the loops ARE the function:
// duplicating forty lines of insertion order and built-in-name rules would be
// a second place for them to drift apart.
//
// Every table below takes its item BY VALUE (NamedTable::add,
// PropertyDatabase::define, EntityDatabase::insert), so a moved record goes
// all the way in. update() takes a reference and is never handed a copy, but
// it only ever applies to the built-in names.
fn apply_contents(contents: Staging<'_>, model: &mut Model) -> Status {
    // Built ASIDE and committed only once every insertion has succeeded.
    //
    // validate_contents() does not cover everything the tables themselves
    // enforce - line weights, empty or duplicate names, property defaults -
    // so an insertion here really can fail on a file that passed validation.
    // The previous version reset the caller's model first and reset it again
    // on failure, which destroyed the open drawing; Document::open documents
    // the opposite ("a failed open leaves the current drawing exactly as it
    // was") and relies on it.
    let mut staged = Model::default();
    for layer in contents.layers {
        if layer.name == DEFAULT_LAYER_NAME {
            staged.layers.update(&layer)?;
        } else {
            staged.layers.add(layer.into_owned())?;
        }
    }
    for style in contents.styles {
        staged.styles.add(style.into_owned())?;
    }
    for linetype in contents.linetypes {
        // "continuous" is built in, so a stored one updates rather than adds -
        // the same rule layer "0" follows above.
        if linetype.name == CONTINUOUS_LINETYPE {
            staged.linetypes.update(&linetype)?;
        } else {
            staged.linetypes.add(linetype.into_owned())?;
        }
    }
    for style in contents.dimension_styles {
        if style.name == DEFAULT_DIMENSION_STYLE_NAME {
            staged.dimension_styles.update(&style)?;
        } else {
            staged.dimension_styles.add(style.into_owned())?;
        }
    }
    for pattern in contents.hatch_patterns {
        // "none" is built in, so a stored one updates rather than adds - the
        // same rule layer "0" and the continuous linetype follow above.
        if pattern.name == NO_HATCH {
            staged.hatch_patterns.update(&pattern)?;
        } else {
            staged.hatch_patterns.add(pattern.into_owned())?;
        }
    }
    for alignment in contents.alignments {
        // add() solves the definition, so a file holding an alignment that
        // cannot be built is refused here with the PI named, not opened and
        // drawn wrong.
        staged.alignments.add(alignment.into_owned())?;
    }
    for definition in contents.property_definitions {
        staged.properties.define(definition.into_owned())?;
    }
    for style in contents.text_styles {
        // "Standard" is built in, so a stored one updates rather than adds.
        if style.name == DEFAULT_TEXT_STYLE_NAME {
            staged.text_styles.update(&style)?;
        } else {
            staged.text_styles.add(style.into_owned())?;
        }
    }
    for style in contents.label_styles {
        staged.label_styles.add(style.into_owned())?;
    }
    for rule in contents.label_rules {
        staged.label_rules.add(rule.into_owned())?;
    }
    for entity in contents.entities {
        staged.entities.insert(entity.into_owned())?;
    }
    staged.entities.reserve_ids_below(contents.next_entity_id);

    // Nothing below can fail, so this is the commit point.
    model.adopt_contents(staged);
    Ok(())
}

/// Replaces the content of `model`. The contents are checked first, so a
/// failure leaves the model untouched.
pub fn apply_to_model(contents: &ProjectContents, model: &mut Model) -> Status {
    validate_contents(contents)?;
    apply_contents(
        Staging {
            layers: Items::borrowed(&contents.layers),
            styles: Items::borrowed(&contents.styles),
            linetypes: Items::borrowed(&contents.linetypes),
            dimension_styles: Items::borrowed(&contents.dimension_styles),
            hatch_patterns: Items::borrowed(&contents.hatch_patterns),
            alignments: Items::borrowed(&contents.alignments),
            property_definitions: Items::borrowed(&contents.property_definitions),
            text_styles: Items::borrowed(&contents.text_styles),
            label_styles: Items::borrowed(&contents.label_styles),
            label_rules: Items::borrowed(&contents.label_rules),
            entities: Items::borrowed(&contents.entities),
            next_entity_id: contents.next_entity_id,
        },
        model,
    )
}

/// The same, CONSUMING the contents: the C++ `applyToModel(ProjectContents&&)`.
/// Opening a project loads a ProjectContents, applies it and throws it away,
/// and the copy in between was 22.6% of every heap allocation a
/// 50,000-entity open made - an Entity carries two maps and a geometry
/// holding vectors, and each one was duplicated into the model and dropped
/// moments later.
///
/// `model` is still untouched when this fails, so a failed open leaves the
/// caller's drawing as it was - but take anything still needed out of the
/// contents (the metadata) BEFORE calling this, as they are gone either way.
pub fn apply_to_model_consuming(contents: ProjectContents, model: &mut Model) -> Status {
    validate_contents(&contents)?;
    let ProjectContents {
        layers,
        styles,
        linetypes,
        dimension_styles,
        hatch_patterns,
        alignments,
        property_definitions,
        text_styles,
        label_styles,
        label_rules,
        entities,
        next_entity_id,
        ..
    } = contents;
    apply_contents(
        Staging {
            layers: Items::owned(layers),
            styles: Items::owned(styles),
            linetypes: Items::owned(linetypes),
            dimension_styles: Items::owned(dimension_styles),
            hatch_patterns: Items::owned(hatch_patterns),
            alignments: Items::owned(alignments),
            property_definitions: Items::owned(property_definitions),
            text_styles: Items::owned(text_styles),
            label_styles: Items::owned(label_styles),
            label_rules: Items::owned(label_rules),
            entities: Items::owned(entities),
            next_entity_id,
        },
        model,
    )
}

// ---- writing -------------------------------------------------------------------------

fn write_metadata_row(insert: &mut SqliteStatement<'_>, key: &str, value: &str) -> Status {
    insert.bind(1, key)?;
    insert.bind(2, value)?;
    insert.run()
}

fn write_metadata(database: &SqliteDatabase, contents: &ProjectContents) -> Status {
    // REPLACE, because save() does not empty this table: a key this build
    // does not know stays where a newer build put it, and every key it does
    // know is written on every save, so none can go stale.
    let mut insert =
        database.prepare("INSERT OR REPLACE INTO metadata (key, value) VALUES (?1, ?2)")?;
    let m = &contents.metadata;
    // One row per key the reader knows, the array's length checked against
    // the list's: a key added to one list and not the other would be read
    // back as unknown, or never written.
    let rows: [(&str, String); METADATA_KEYS.len()] = [
        ("name", m.name.clone()),
        ("description", m.description.clone()),
        ("linear_unit", m.linear_unit.clone()),
        ("coordinate_system", m.coordinate_system.clone()),
        ("created_utc", m.created_utc.clone()),
        ("modified_utc", utc_timestamp(false)),
        ("application_version", m.application_version.clone()),
        ("next_entity_id", format!("{}", contents.next_entity_id)),
        ("customisation", joined_lines(&m.customisation)),
        ("reference_layers", joined_lines(&m.reference_layers)),
    ];
    for (key, value) in &rows {
        write_metadata_row(&mut insert, key, value)?;
    }
    for (key, value) in &m.unknown_keys {
        write_metadata_row(&mut insert, key, value)?;
    }
    // A key of the drawing's own that it no longer writes was REMOVED by this
    // build - a sheet set emptied, the annotation scale reset, the road model
    // deleted - so its row goes too: REPLACE alone left the last save's value
    // standing, and it came back when the project was opened again. A key
    // this build does not know is still never deleted (above).
    let mut remove = database.prepare("DELETE FROM metadata WHERE key = ?1")?;
    for key in DRAWING_METADATA_KEYS {
        if m.unknown_keys.contains_key(key) {
            continue;
        }
        remove.bind(1, key)?;
        remove.run()?;
    }
    Ok(())
}

/// Chains bind calls, stopping at the first failure.
struct Binder<'s, 'db> {
    statement: &'s mut SqliteStatement<'db>,
    status: Status,
}

impl<'s, 'db> Binder<'s, 'db> {
    fn new(statement: &'s mut SqliteStatement<'db>) -> Self {
        Self {
            statement,
            status: Ok(()),
        }
    }

    fn bind<V: BindValue>(mut self, index: i32, value: V) -> Self {
        if self.status.is_ok() {
            self.status = self.statement.bind(index, value);
        }
        self
    }

    /// Binds a nullable colour column.
    fn color(mut self, index: i32, value: Option<Color>) -> Self {
        if self.status.is_ok() {
            self.status = match value {
                Some(color) => self.statement.bind(index, color.to_hex().as_str()),
                None => self.statement.bind_null(index),
            };
        }
        self
    }

    /// The statement, for a bind the chain does not make (the C++ binds a
    /// property definition's NULL default past its binder).
    fn statement(&mut self) -> &mut SqliteStatement<'db> {
        self.statement
    }

    fn run(self) -> Status {
        if self.status.is_err() {
            self.statement.reset();
            return self.status;
        }
        self.statement.run()
    }
}

// ---- survey jobs ---------------------------------------------------------------------
//
// Two lists of a job are stored packed rather than one row per element: the
// entity ids it created and the points it placed. Little-endian throughout,
// written byte by byte so the file reads the same on any machine; doubles by
// their bit pattern, so they come back bit for bit.

fn append_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn append_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

// Reads what the append functions wrote. Every read is bounds-checked: a
// damaged row fails the load with a sentence, never reads past the blob.
struct PackedReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> PackedReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn read_u8(&mut self) -> Option<u8> {
        if self.remaining() < 1 {
            return None;
        }
        let value = self.bytes[self.offset];
        self.offset += 1;
        Some(value)
    }

    fn read_u32(&mut self) -> Option<u32> {
        self.little(4).map(|wide| wide as u32)
    }

    fn read_u64(&mut self) -> Option<u64> {
        self.little(8)
    }

    fn read_f64(&mut self) -> Option<f64> {
        self.little(8).map(f64::from_bits)
    }

    fn read_text(&mut self, size: usize) -> Option<&'a [u8]> {
        if self.remaining() < size {
            return None;
        }
        let text = &self.bytes[self.offset..self.offset + size];
        self.offset += size;
        Some(text)
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }

    fn little(&mut self, size: usize) -> Option<u64> {
        if self.remaining() < size {
            return None;
        }
        let mut value: u64 = 0;
        for (i, &byte) in self.bytes[self.offset..self.offset + size]
            .iter()
            .enumerate()
        {
            value |= u64::from(byte) << (8 * i);
        }
        self.offset += size;
        Some(value)
    }
}

// Eight bytes per id, no header: the count is the size over eight.
fn encode_entity_ids(ids: &[EntityId]) -> Vec<u8> {
    let mut out = Vec::with_capacity(ids.len() * 8);
    for &id in ids {
        append_u64(&mut out, id);
    }
    out
}

// Version 1: a version byte, a u64 count, then per point the entity (u64),
// northing and easting (f64), a byte saying whether an elevation follows, the
// elevation (f64) when it does, the id's length (u32) and the id's bytes. The
// version byte is there so a later layout can be read beside this one.
const PLACED_POINTS_VERSION: u8 = 1;
// The fewest bytes a point can take: entity, northing, easting, the elevation
// flag and the id's length.
const SMALLEST_PLACED_POINT: usize = 8 + 8 + 8 + 1 + 4;

// What encode_placed_points writes, in bytes, without writing it: the size
// check runs on every save and every import, and a job's list may hold a
// hundred thousand points.
fn placed_points_bytes(points: &[SurveyJobPoint]) -> u64 {
    let mut bytes: u64 = 1 + 8; // the version byte and the count
    for point in points {
        let elevation: u64 = if point.elevation.is_some() { 8 } else { 0 };
        bytes += SMALLEST_PLACED_POINT as u64 + elevation + point.point_id.len() as u64;
    }
    bytes
}

fn encode_placed_points(points: &[SurveyJobPoint]) -> Vec<u8> {
    let mut out = Vec::with_capacity(placed_points_bytes(points) as usize);
    out.push(PLACED_POINTS_VERSION);
    append_u64(&mut out, points.len() as u64);
    for point in points {
        append_u64(&mut out, point.entity);
        append_u64(&mut out, point.northing.to_bits());
        append_u64(&mut out, point.easting.to_bits());
        out.push(u8::from(point.elevation.is_some()));
        if let Some(elevation) = point.elevation {
            append_u64(&mut out, elevation.to_bits());
        }
        append_u32(&mut out, point.point_id.len() as u32);
        out.extend_from_slice(point.point_id.as_bytes());
    }
    out
}

fn decode_entity_ids(bytes: &[u8], job: &str) -> Result<Vec<EntityId>> {
    if !bytes.len().is_multiple_of(8) {
        return Err(make_error(
            ErrorCode::DatabaseFailure,
            "a survey job's list of the entities it created is damaged",
            format!("job={job}"),
        ));
    }
    let mut ids = Vec::with_capacity(bytes.len() / 8);
    let mut reader = PackedReader::new(bytes);
    while let Some(value) = reader.read_u64() {
        ids.push(value);
    }
    Ok(ids)
}

fn decode_placed_points(bytes: &[u8], job: &str) -> Result<Vec<SurveyJobPoint>> {
    let damaged = |what: &str| {
        make_error(
            ErrorCode::DatabaseFailure,
            "a survey job's list of the points it placed is damaged",
            format!("job={job} {what}"),
        )
    };
    let mut reader = PackedReader::new(bytes);
    let Some(version) = reader.read_u8() else {
        return Err(damaged("header"));
    };
    let Some(count) = reader.read_u64() else {
        return Err(damaged("header"));
    };
    if version != PLACED_POINTS_VERSION {
        return Err(make_error(
            ErrorCode::Unsupported,
            "a survey job was saved by a newer version of Katana",
            format!("job={job} placed points version={version}"),
        ));
    }
    // A count the blob cannot hold is damage - and reserving for it would be
    // an allocation of a size the file chose.
    if count > (reader.remaining() / SMALLEST_PLACED_POINT) as u64 {
        return Err(damaged("count"));
    }
    let mut points = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let Some(entity) = reader.read_u64() else {
            return Err(damaged("point"));
        };
        let Some(northing) = reader.read_f64() else {
            return Err(damaged("point"));
        };
        let Some(easting) = reader.read_f64() else {
            return Err(damaged("point"));
        };
        let Some(has_elevation) = reader.read_u8() else {
            return Err(damaged("point"));
        };
        if has_elevation > 1 {
            return Err(damaged("point"));
        }
        let mut elevation = None;
        if has_elevation == 1 {
            let Some(value) = reader.read_f64() else {
                return Err(damaged("elevation"));
            };
            elevation = Some(value);
        }
        let Some(id_size) = reader.read_u32() else {
            return Err(damaged("point id"));
        };
        // An id that is not UTF-8 is damage too: the model's text is.
        let Some(point_id) = reader
            .read_text(id_size as usize)
            .and_then(|id| std::str::from_utf8(id).ok())
        else {
            return Err(damaged("point id"));
        };
        points.push(SurveyJobPoint {
            point_id: point_id.to_owned(),
            entity,
            northing,
            easting,
            elevation,
        });
    }
    if reader.remaining() != 0 {
        return Err(damaged("trailing bytes"));
    }
    Ok(points)
}

// SQLite's record header for a survey_jobs row: a varint per column and one
// for the header's own size, at most nine bytes each for sixteen columns, and
// the position's eight-byte integer. A kilobyte is ample.
const RECORD_HEADER_ALLOWANCE: i64 = 1024;

// What save() refuses before it writes anything: a job it could not load back
// as the same job, and one too large for the database to hold.
fn validate_survey_jobs(jobs: &[SurveyJob], size_limit: u64) -> Status {
    let mut ids: BTreeSet<&str> = BTreeSet::new();
    for job in jobs {
        if job.id.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a survey job has no id",
                format!("name={}", job.name),
            ));
        }
        if !ids.insert(job.id.as_str()) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "two survey jobs have the same id",
                format!("job={}", job.id),
            ));
        }
        for file in &job.sibling_files {
            if file.name.is_empty() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "a file kept with a survey job has no name",
                    format!("job={}", job.id),
                ));
            }
        }
        ProjectStore::check_survey_job_size(job, size_limit)?;
    }
    Ok(())
}

fn write_survey_jobs(database: &SqliteDatabase, jobs: &[SurveyJob]) -> Status {
    if jobs.is_empty() {
        return Ok(());
    }
    let mut insert_job = database.prepare(concat!(
        "INSERT INTO survey_jobs (position, id, name, format_id, parser_version,",
        " source_file_name, source_bytes, settings, layer, created_entities, placed_points,",
        " report_text, report_html, report_created_utc, imported_utc, import_options)",
        " VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)"
    ))?;
    let mut insert_file = database.prepare(
        "INSERT INTO survey_job_files (job, position, name, bytes) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for (position, job) in jobs.iter().enumerate() {
        let settings = serialise_reduction_settings(&job.settings);
        let created = encode_entity_ids(&job.created_entities);
        let placed = encode_placed_points(&job.placed_points);
        Binder::new(&mut insert_job)
            .bind(1, position as i64)
            .bind(2, &job.id)
            .bind(3, &job.name)
            .bind(4, &job.format_id)
            .bind(5, &job.parser_version)
            .bind(6, &job.source_file_name)
            .bind(7, job.source_bytes.as_slice())
            .bind(8, &settings)
            .bind(9, &job.layer)
            .bind(10, created.as_slice())
            .bind(11, placed.as_slice())
            .bind(12, &job.report_text)
            .bind(13, &job.report_html)
            .bind(14, &job.report_created_utc)
            .bind(15, &job.imported_utc)
            .bind(16, &job.import_options)
            .run()
            .map_err(|error| {
                make_error(
                    error.code,
                    format!(
                        "survey job {} could not be saved: {}",
                        job.id, error.message
                    ),
                    format!("bytes={} {}", job.source_bytes.len(), error.context),
                )
            })?;
        for (index, file) in job.sibling_files.iter().enumerate() {
            Binder::new(&mut insert_file)
                .bind(1, &job.id)
                .bind(2, index as i64)
                .bind(3, &file.name)
                .bind(4, file.bytes.as_slice())
                .run()
                .map_err(|error| {
                    make_error(
                        error.code,
                        format!(
                            "file {} of survey job {} could not be saved: {}",
                            file.name, job.id, error.message
                        ),
                        error.context,
                    )
                })?;
        }
    }
    Ok(())
}

fn read_survey_jobs(database: &SqliteDatabase) -> Result<Vec<SurveyJob>> {
    const JOBS: &str = "survey_jobs";
    const FILES: &str = "survey_job_files";
    let mut jobs: Vec<SurveyJob> = Vec::new();
    let mut select = database.prepare(concat!(
        "SELECT id, name, format_id, parser_version, source_file_name, source_bytes, settings,",
        " layer, created_entities, placed_points, report_text, report_html,",
        " report_created_utc, imported_utc, import_options FROM survey_jobs ORDER BY position"
    ))?;
    while select.step()? {
        let id = text_column(&mut select, 0, JOBS)?;
        let name = text_column(&mut select, 1, JOBS)?;
        let format_id = text_column(&mut select, 2, JOBS)?;
        let parser_version = text_column(&mut select, 3, JOBS)?;
        let source_file_name = text_column(&mut select, 4, JOBS)?;
        // Straight from SQLite's buffer into the vector: one copy of a file
        // that may be tens of megabytes, not two.
        let source_bytes = select.column_blob(5);
        // A setting a newer build added is skipped by the parser, which is
        // what its versioning promises; text from a newer VERSION of the
        // settings is refused, and so is the load - half-read settings would
        // re-adjust the job with something the person never chose.
        let settings = text_column_view(&mut select, 6, JOBS)
            .and_then(|text| parse_reduction_settings(text, None))
            .map_err(|error| {
                make_error(
                    error.code,
                    format!(
                        "the reduction settings of survey job {id} cannot be read: {}",
                        error.message
                    ),
                    error.context,
                )
            })?;
        let layer = text_column(&mut select, 7, JOBS)?;
        let created_entities = decode_entity_ids(select.column_blob_span(8), &id)?;
        let placed_points = decode_placed_points(select.column_blob_span(9), &id)?;
        let report_text = text_column(&mut select, 10, JOBS)?;
        let report_html = text_column(&mut select, 11, JOBS)?;
        let report_created_utc = text_column(&mut select, 12, JOBS)?;
        let imported_utc = text_column(&mut select, 13, JOBS)?;
        let import_options = text_column(&mut select, 14, JOBS)?;
        jobs.push(SurveyJob {
            id,
            name,
            format_id,
            parser_version,
            source_file_name,
            source_bytes,
            sibling_files: Vec::new(),
            settings,
            layer,
            created_entities,
            report_text,
            report_html,
            report_created_utc,
            imported_utc,
            placed_points,
            import_options,
        });
    }

    let mut files =
        database.prepare("SELECT job, name, bytes FROM survey_job_files ORDER BY job, position")?;
    while files.step()? {
        let owner = text_column(&mut files, 0, FILES)?;
        let Some(found) = jobs.iter_mut().find(|job| job.id == owner) else {
            return Err(make_error(
                ErrorCode::DatabaseFailure,
                "a file kept with a survey job names a job that is not there",
                format!("job={owner}"),
            ));
        };
        let name = text_column(&mut files, 1, FILES)?;
        let bytes = files.column_blob(2);
        found.sibling_files.push(SurveyJobFile { name, bytes });
    }
    Ok(jobs)
}

// ---- surfaces ------------------------------------------------------------------------
//
// Opaque records (SurfaceRecord): the bytes are the terrain layer's, written
// and read whole.

// What save() refuses before it writes anything: a surface it could not load
// back as the same surface - no name, no bytes, a name another has (the
// drawing's store compares names without case, so "Ground" and "ground" could
// not both be loaded) - and one too large for the database.
fn validate_surfaces(surfaces: &[SurfaceRecord], size_limit: u64) -> Status {
    for (i, surface) in surfaces.iter().enumerate() {
        if surface.name.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a surface has no name",
                format!("position={i}"),
            ));
        }
        if surface.data.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a surface has nothing to keep",
                format!("surface={}", shown(&surface.name)),
            ));
        }
        for earlier in &surfaces[..i] {
            if equals_ignoring_case(&earlier.name, &surface.name) {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "two surfaces have the same name",
                    format!("surface={}", shown(&surface.name)),
                ));
            }
        }
        ProjectStore::check_surface_size(surface, size_limit)?;
    }
    Ok(())
}

fn write_surfaces(database: &SqliteDatabase, surfaces: &[SurfaceRecord]) -> Status {
    if surfaces.is_empty() {
        return Ok(());
    }
    let mut insert = database.prepare(concat!(
        "INSERT INTO surfaces (position, name, source, style, visible, data)",
        " VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
    ))?;
    for (position, surface) in surfaces.iter().enumerate() {
        Binder::new(&mut insert)
            .bind(1, position as i64)
            .bind(2, TextBytes(&surface.name))
            .bind(3, TextBytes(&surface.source))
            .bind(4, TextBytes(&surface.style))
            .bind(5, i64::from(surface.visible))
            .bind(6, surface.data.as_slice())
            .run()
            .map_err(|error| {
                make_error(
                    error.code,
                    format!(
                        "surface {} could not be saved: {}",
                        shown(&surface.name),
                        error.message
                    ),
                    format!("bytes={} {}", surface.data.len(), error.context),
                )
            })?;
    }
    Ok(())
}

fn read_surfaces(database: &SqliteDatabase) -> Result<Vec<SurfaceRecord>> {
    let mut surfaces = Vec::new();
    let mut select = database
        .prepare("SELECT name, source, style, visible, data FROM surfaces ORDER BY position")?;
    while select.step()? {
        let name = select.column_text(0);
        let source = select.column_text(1);
        let style = select.column_text(2);
        let visible = select.column_int64(3) != 0;
        // Straight from SQLite's buffer: one copy of a blob that may be tens
        // of megabytes, as a survey job's field file is read.
        let data = select.column_blob(4);
        surfaces.push(SurfaceRecord {
            name,
            source,
            style,
            visible,
            data,
        });
    }
    Ok(surfaces)
}

// ---- reading -------------------------------------------------------------------------

/// Runs `sql` and calls `on_row` for every row; stops at the first failure.
fn for_each_row(
    database: &SqliteDatabase,
    sql: &str,
    mut on_row: impl FnMut(&mut SqliteStatement<'_>) -> Status,
) -> Status {
    let mut statement = database.prepare(sql)?;
    while statement.step()? {
        on_row(&mut statement)?;
    }
    Ok(())
}

// ---- ProjectStore --------------------------------------------------------------------

#[derive(Debug)]
pub struct ProjectStore {
    directory: PathBuf,
    database: SqliteDatabase,
}

impl ProjectStore {
    /// 3: geometry stored as a binary blob rather than JSON text. See the
    /// migration table above and docs/storage.md for why.
    /// 4: linetype definitions (Phase 09).
    /// 5: dimension styles, and Layer::dimension_style (Phase 09).
    /// 6: hatch patterns, and Layer/Style::hatch_pattern (Phase 09).
    /// 7: alignments, stored as their PI definitions (Phase 21).
    /// 8: design profiles on alignments, as PVIs (Phase 21).
    /// 9: a style's description and point symbol.
    /// 10: survey jobs - the raw bytes of each imported field file and its
    /// siblings, its reduction settings, report and placed points.
    /// 11: the annotation tables - text styles, label styles, auto-label rules
    /// and a dimension style's paper sizing (docs/annotation.md).
    /// 12: the drawing system's geometry kinds - curve polyline, ellipse and
    /// spline (docs/drawing.md) - which a schema-11 build cannot read.
    /// 13: the drawing's TIN surfaces (SurfaceRecord; docs/terrain.md).
    pub const CURRENT_SCHEMA_VERSION: i32 = 13;
    pub const DEFAULT_BACKUPS_TO_KEEP: usize = 10;

    /// The most bytes one survey job may take in a project: its own row (the
    /// field file, its names, settings, report and packed point lists) and,
    /// separately, each file kept with it. SQLite refuses a string, a BLOB or
    /// a row over its SQLITE_MAX_LENGTH - 1,000,000,000 bytes by default, and
    /// as MSYS2 builds it - while a reader accepts a field file of up to 1 GiB
    /// (surveyio's largest survey file). Without this a job could be imported
    /// that no save could write, and every later save of the whole project
    /// would fail. A round figure, well under the library's limit; save() also
    /// honours the connection's own limit, should a build of SQLite have a
    /// lower one.
    pub const MAX_SURVEY_JOB_BYTES: u64 = 900_000_000;

    /// The most bytes one surface may take in a project, its row whole: the
    /// same round figure as a survey job's and for the same reason - SQLite
    /// refuses a BLOB or a row over SQLITE_MAX_LENGTH (1,000,000,000 bytes by
    /// default), and a surface that no save could write would fail every save
    /// of the whole project after it. At 13 bytes a triangle and 24 a vertex
    /// (terrain's tin_codec) it is some 30 million triangles; the surfaces
    /// Katana makes are capped at 400 000 points (SURFACE FROM's max=).
    pub const MAX_SURFACE_BYTES: u64 = 900_000_000;

    /// The bytes `job` takes in its own row: every column's content, without
    /// the few bytes of SQLite's record header.
    pub fn survey_job_row_bytes(job: &SurveyJob) -> u64 {
        job.id.len() as u64
            + job.name.len() as u64
            + job.format_id.len() as u64
            + job.parser_version.len() as u64
            + job.source_file_name.len() as u64
            + job.source_bytes.len() as u64
            + serialise_reduction_settings(&job.settings).len() as u64
            + job.layer.len() as u64
            + 8 * job.created_entities.len() as u64
            + placed_points_bytes(&job.placed_points)
            + job.report_text.len() as u64
            + job.report_html.len() as u64
            + job.report_created_utc.len() as u64
            + job.imported_utc.len() as u64
            + job.import_options.len() as u64
    }

    /// InvalidArgument, with a sentence naming the job's field file and saying
    /// what to do instead, when the job's row or one of its files' rows would
    /// be over `limit` bytes (MAX_SURVEY_JOB_BYTES, the C++'s default). The
    /// survey job commands call it before they change anything; save() calls
    /// it for every job, so a job too large is refused before a byte is
    /// written, not by SQLite halfway through.
    pub fn check_survey_job_size(job: &SurveyJob, limit: u64) -> Status {
        const INSTEAD: &str = ". Split the survey into smaller files - fewer days, or a shorter or thinned-out observation session - and import each one as its own job.";
        // Named by its field file, which is what the person chose and will
        // look for; the id exists only once the job is on the drawing's list.
        let file = if job.source_file_name.is_empty() {
            &job.name
        } else {
            &job.source_file_name
        };
        let what = if job.id.is_empty() {
            if file.is_empty() {
                String::from("the survey job")
            } else {
                file.clone()
            }
        } else if file.is_empty() {
            format!("survey job {}", job.id)
        } else {
            format!("survey job {} ({file})", job.id)
        };
        let megabytes = |bytes: u64| format!("{} MB", format_fixed(bytes as f64 / 1e6, 1));
        let row = Self::survey_job_row_bytes(job);
        if row > limit {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "{what} is too large to keep in a project: with its report and point lists it comes to {}, and a project keeps at most {} for one survey job{INSTEAD}",
                    megabytes(row),
                    megabytes(limit)
                ),
                format!("job={} bytes={row} limit={limit}", job.id),
            ));
        }
        for sibling in &job.sibling_files {
            // Its own row in survey_job_files: the job's id, its name and its
            // bytes.
            let bytes =
                job.id.len() as u64 + sibling.name.len() as u64 + sibling.bytes.len() as u64;
            if bytes > limit {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!(
                        "{}, read with {what}, is too large to keep in a project: it comes to {}, and a project keeps at most {} for one file{INSTEAD}",
                        sibling.name,
                        megabytes(bytes),
                        megabytes(limit)
                    ),
                    format!(
                        "job={} file={} bytes={bytes} limit={limit}",
                        job.id, sibling.name
                    ),
                ));
            }
        }
        Ok(())
    }

    /// The bytes `surface` takes in its own row: its name, source, style and
    /// data, and eight for the flag and the position.
    pub fn surface_row_bytes(surface: &SurfaceRecord) -> u64 {
        surface.name.len() as u64
            + surface.source.len() as u64
            + surface.style.len() as u64
            + surface.data.len() as u64
            + 8
    }

    /// InvalidArgument, with a sentence naming the surface and saying what to
    /// do instead, when its row would be over `limit` bytes
    /// (MAX_SURFACE_BYTES, the C++'s default). save() calls it for every
    /// surface before a byte is written, so a surface too large is refused
    /// whole, not by SQLite halfway through.
    pub fn check_surface_size(surface: &SurfaceRecord, limit: u64) -> Status {
        let row = Self::surface_row_bytes(surface);
        if row <= limit {
            return Ok(());
        }
        let megabytes = |bytes: u64| format!("{} MB", format_fixed(bytes as f64 / 1e6, 1));
        let name = shown(&surface.name);
        Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "surface {name} is too large to keep in a project: it comes to {}, and a project keeps at most {} for one surface. Make it from fewer points (SURFACE FROM ... max=), or clip it into parts.",
                megabytes(row),
                megabytes(limit)
            ),
            format!("surface={name} bytes={row} limit={limit}"),
        ))
    }

    /// Creates the directory layout and an empty database. Fails if a project
    /// already exists there.
    pub fn create(project_directory: &Path, metadata: &ProjectMetadata) -> Result<ProjectStore> {
        if Self::is_project_directory(project_directory) {
            return Err(make_error(
                ErrorCode::AlreadyExists,
                "a project already exists in this directory",
                path_text(project_directory),
            ));
        }
        let mut failure = std::fs::create_dir_all(project_directory.join(BACKUP_DIRECTORY)).err();
        for name in DATA_DIRECTORIES {
            if failure.is_none() {
                failure = std::fs::create_dir_all(project_directory.join(name)).err();
            }
        }
        if let Some(error) = failure {
            return Err(make_error(
                ErrorCode::DatabaseFailure,
                "could not create the project directories",
                format!(
                    "{}: {}",
                    path_text(project_directory),
                    system_message(&error)
                ),
            ));
        }

        let database = SqliteDatabase::open(
            &project_directory.join(DATABASE_FILE),
            OpenMode::ReadWriteCreate,
        )?;
        database.set_application_id(APPLICATION_ID)?;
        migrate(&database, 0)?;

        let mut store = ProjectStore {
            directory: project_directory.to_path_buf(),
            database,
        };
        let mut stamped = metadata.clone();
        stamped.created_utc = utc_timestamp(false);
        let empty = ProjectContents {
            metadata: stamped,
            layers: vec![Layer::default()],
            ..ProjectContents::default()
        };
        store.save(&empty)?;
        Ok(store)
    }

    /// Opens an existing project: verifies it is a Katana database, checks its
    /// integrity and migrates older schemas (backing up first).
    pub fn open(project_directory: &Path) -> Result<ProjectStore> {
        if !Self::is_project_directory(project_directory) {
            return Err(make_error(
                ErrorCode::NotFound,
                "no Katana project in this directory",
                path_text(project_directory),
            ));
        }
        let file = project_directory.join(DATABASE_FILE);
        if let Some(problems) = inspect_database(&file)? {
            return Err(make_error(
                ErrorCode::DatabaseFailure,
                "project database is damaged; ProjectStore::recover() can restore a backup",
                problems,
            ));
        }

        let database = SqliteDatabase::open(&file, OpenMode::ReadWrite)?;
        let version = database.user_version()?;
        if version > Self::CURRENT_SCHEMA_VERSION {
            return Err(make_error(
                ErrorCode::Unsupported,
                "project was written by a newer version of Katana",
                format!(
                    "schema={version} supported={}",
                    Self::CURRENT_SCHEMA_VERSION
                ),
            ));
        }

        let mut store = ProjectStore {
            directory: project_directory.to_path_buf(),
            database,
        };
        if version < Self::CURRENT_SCHEMA_VERSION {
            // Never migrate without a way back.
            store.backup(Self::DEFAULT_BACKUPS_TO_KEEP)?;
            migrate(&store.database, version)?;
        }
        Ok(store)
    }

    /// Restores the newest backup that passes an integrity check when
    /// project.db is missing, unreadable or corrupt. Never deletes data.
    pub fn recover(project_directory: &Path) -> Result<RecoveryReport> {
        let file = project_directory.join(DATABASE_FILE);
        let mut report = RecoveryReport::default();

        if file.exists() {
            match inspect_database(&file)? {
                None => return Ok(report), // sound: nothing to do
                Some(problems) => report.problems = problems,
            }
        } else {
            report.problems = String::from("project.db is missing");
        }

        for candidate in backups_newest_first(project_directory) {
            if !matches!(inspect_database(&candidate), Ok(None)) {
                continue; // this backup is damaged too; try an older one
            }
            if file.exists() {
                report.damaged_file_kept_as = project_directory
                    .join(format!("{DATABASE_FILE}.damaged-{}", utc_timestamp(true)));
                if let Err(error) =
                    std::fs::rename(file.as_path(), report.damaged_file_kept_as.as_path())
                {
                    return Err(make_error(
                        ErrorCode::DatabaseFailure,
                        "could not move the damaged database aside",
                        system_message(&error),
                    ));
                }
            }
            // A rollback journal of the damaged file must never be left beside
            // the restored one: SQLite would replay it over the good data on
            // the next open, corrupting the file that was just recovered.
            let journal = project_directory.join(format!("{DATABASE_FILE}-journal"));
            if journal.exists() {
                // damaged_file_kept_as is empty when project.db was missing
                // entirely, and appending to an empty path yields the RELATIVE
                // name "-journal", which moves the journal into the working
                // directory and leaves it beside the restored database. The
                // target is always built inside project_directory instead.
                let journal_kept_as = if report.damaged_file_kept_as.as_os_str().is_empty() {
                    project_directory.join(format!(
                        "{DATABASE_FILE}.damaged-{}-journal",
                        utc_timestamp(true)
                    ))
                } else {
                    let mut name = report.damaged_file_kept_as.clone().into_os_string();
                    name.push("-journal");
                    PathBuf::from(name)
                };
                if let Err(error) = std::fs::rename(journal.as_path(), journal_kept_as.as_path()) {
                    // Restoring on top of a journal that is still there would
                    // be worse than not restoring at all.
                    return Err(make_error(
                        ErrorCode::DatabaseFailure,
                        "could not move the damaged database's rollback journal aside",
                        system_message(&error),
                    ));
                }
            }
            if let Err(error) = std::fs::copy(candidate.as_path(), file.as_path()) {
                return Err(make_error(
                    ErrorCode::DatabaseFailure,
                    "could not restore the backup",
                    system_message(&error),
                ));
            }
            report.restored = true;
            report.restored_from = candidate;
            return Ok(report);
        }
        Err(make_error(
            ErrorCode::DatabaseFailure,
            "no usable backup was found",
            format!("{}: {}", path_text(project_directory), report.problems),
        ))
    }

    pub fn is_project_directory(directory: &Path) -> bool {
        directory.join(DATABASE_FILE).is_file()
    }

    pub fn save(&mut self, contents: &ProjectContents) -> Status {
        validate_contents(contents)?;
        // The job limit, or this connection's own should the SQLite it was
        // built with have a lower one.
        let library = self.database.length_limit() - RECORD_HEADER_ALLOWANCE;
        let job_limit = if library <= 0 {
            0
        } else {
            Self::MAX_SURVEY_JOB_BYTES.min(library as u64)
        };
        validate_survey_jobs(&contents.survey_jobs, job_limit)?;
        // The surfaces' limit likewise.
        let surface_limit = if library <= 0 {
            0
        } else {
            Self::MAX_SURFACE_BYTES.min(library as u64)
        };
        validate_surfaces(&contents.surfaces, surface_limit)?;
        let database = &self.database;
        let mut transaction = SqliteTransaction::begin(database)?;
        // Every table a save rewrites must be cleared here, or the SECOND save
        // of a project fails on a primary key it already wrote. A round-trip
        // test that saves once does not catch that; cad's
        // SaveReopenAndModifiedFlag does, which is why it saves twice.
        //
        // The metadata table is the one exception, and is not cleared: it
        // holds keys a newer build wrote, which an older build's save must not
        // strip. write_metadata replaces every key this build knows instead.
        //
        // linetype_elements is deleted explicitly rather than left to the ON
        // DELETE CASCADE: foreign keys are only enforced when the pragma is on,
        // so relying on the cascade would make correctness depend on a
        // connection setting.
        database.execute(concat!(
            "DELETE FROM surfaces;",
            "DELETE FROM survey_job_files; DELETE FROM survey_jobs;",
            "DELETE FROM relationships; DELETE FROM entities;",
            "DELETE FROM property_definitions; DELETE FROM styles;",
            "DELETE FROM linetype_elements; DELETE FROM linetypes;",
            "DELETE FROM dimension_styles;",
            "DELETE FROM text_styles; DELETE FROM label_styles;",
            "DELETE FROM label_rules;",
            "DELETE FROM hatch_families; DELETE FROM hatch_patterns;",
            "DELETE FROM alignment_pvis; DELETE FROM alignment_pis; DELETE FROM alignments;",
            "DELETE FROM layers;"
        ))?;
        write_metadata(database, contents)?;

        let mut insert_layer = database.prepare(concat!(
            "INSERT INTO layers (name, color, visible, locked, linetype, line_weight,",
            " dimension_style, hatch_pattern) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
        ))?;
        let mut has_default_layer = false;
        for layer in &contents.layers {
            has_default_layer = has_default_layer || layer.name == DEFAULT_LAYER_NAME;
            Binder::new(&mut insert_layer)
                .bind(1, &layer.name)
                .bind(2, &layer.color.to_hex())
                .bind(3, layer.visible)
                .bind(4, layer.locked)
                .bind(5, &layer.linetype)
                .bind(6, layer.line_weight)
                .bind(7, &layer.dimension_style)
                .bind(8, &layer.hatch_pattern)
                .run()?;
        }
        if !has_default_layer {
            // Entities may legitimately refer to it.
            let fallback = Layer::default();
            Binder::new(&mut insert_layer)
                .bind(1, &fallback.name)
                .bind(2, &fallback.color.to_hex())
                .bind(3, fallback.visible)
                .bind(4, fallback.locked)
                .bind(5, &fallback.linetype)
                .bind(6, fallback.line_weight)
                .bind(7, &fallback.dimension_style)
                .bind(8, &fallback.hatch_pattern)
                .run()?;
        }

        let mut insert_style = database.prepare(concat!(
            "INSERT INTO styles (name, color, line_weight, linetype, hatch_pattern, description,",
            " symbol, symbol_size) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
        ))?;
        for style in &contents.styles {
            Binder::new(&mut insert_style)
                .bind(1, &style.name)
                .color(2, style.color)
                .bind(3, style.line_weight)
                .bind(4, &style.linetype)
                .bind(5, &style.hatch_pattern)
                .bind(6, &style.description)
                .bind(7, &style.symbol)
                .bind(8, style.symbol_size)
                .run()?;
        }

        let mut insert_definition = database.prepare(concat!(
            "INSERT INTO property_definitions (name, type, description, default_value)",
            " VALUES (?1, ?2, ?3, ?4)"
        ))?;
        for definition in &contents.property_definitions {
            let mut binder = Binder::new(&mut insert_definition)
                .bind(1, &definition.name)
                .bind(2, property_type_name(definition.r#type))
                .bind(3, &definition.description);
            let status = match &definition.default_value {
                Some(default_value) => {
                    let mut wrapped = PropertyMap::new();
                    wrapped.insert(String::from("value"), default_value.clone());
                    let json = properties_to_json(&wrapped)?;
                    binder.bind(4, &json).run()
                }
                None => binder.statement().bind_null(4).and_then(|()| binder.run()),
            };
            status?;
        }

        let mut insert_dimension_style = database.prepare(concat!(
            "INSERT INTO dimension_styles (name, text_height, text_gap, extension_offset,",
            " extension_beyond, arrow_size, arrow_head, unit_scale, prefix, suffix, decimals,",
            " round_to, suppress_zeros, paper_sized)",
            " VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)"
        ))?;
        for style in &contents.dimension_styles {
            Binder::new(&mut insert_dimension_style)
                .bind(1, &style.name)
                .bind(2, style.text_height)
                .bind(3, style.text_gap)
                .bind(4, style.extension_offset)
                .bind(5, style.extension_beyond)
                .bind(6, style.arrow_size)
                .bind(7, to_string(style.arrow_head))
                .bind(8, style.unit_scale)
                .bind(9, &style.prefix)
                .bind(10, &style.suffix)
                .bind(11, i64::from(style.decimals))
                .bind(12, style.round_to)
                .bind(13, style.suppress_trailing_zeros)
                .bind(14, style.paper_sized)
                .run()
                .map_err(|error| {
                    make_error(
                        error.code,
                        error.message,
                        format!("dimension style={} {}", style.name, error.context),
                    )
                })?;
        }

        let mut insert_text_style = database.prepare(concat!(
            "INSERT INTO text_styles (name, font_family, paper_height, width_factor, oblique, bold,",
            " italic, color, mask, mask_margin, readable, line_spacing)",
            " VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"
        ))?;
        for style in &contents.text_styles {
            // An empty colour is ByLayer, the one way the column says "none".
            let colour = style.color.map(Color::to_hex).unwrap_or_default();
            Binder::new(&mut insert_text_style)
                .bind(1, &style.name)
                .bind(2, &style.font_family)
                .bind(3, style.paper_height)
                .bind(4, style.width_factor)
                .bind(5, style.oblique)
                .bind(6, style.bold)
                .bind(7, style.italic)
                .bind(8, &colour)
                .bind(9, style.mask)
                .bind(10, style.mask_margin)
                .bind(11, style.readable)
                .bind(12, style.line_spacing)
                .run()
                .map_err(|error| {
                    make_error(
                        error.code,
                        error.message,
                        format!("text style={} {}", style.name, error.context),
                    )
                })?;
        }

        let mut insert_label_style = database
            .prepare("INSERT INTO label_styles (name, kind, definition) VALUES (?1, ?2, ?3)")?;
        for style in &contents.label_styles {
            let definition = label_style_definition_to_json(style).map_err(|error| {
                make_error(
                    error.code,
                    error.message,
                    format!("label style={}", style.name),
                )
            })?;
            Binder::new(&mut insert_label_style)
                .bind(1, &style.name)
                .bind(2, to_string(style.kind))
                .bind(3, &definition)
                .run()
                .map_err(|error| {
                    make_error(
                        error.code,
                        error.message,
                        format!("label style={} {}", style.name, error.context),
                    )
                })?;
        }

        let mut insert_label_rule = database.prepare(concat!(
            "INSERT INTO label_rules (name, label_style, layer, code, entity_type, label_layer,",
            " enabled) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
        ))?;
        for rule in &contents.label_rules {
            Binder::new(&mut insert_label_rule)
                .bind(1, &rule.name)
                .bind(2, &rule.label_style)
                .bind(3, &rule.layer)
                .bind(4, &rule.code)
                .bind(5, &rule.entity_type)
                .bind(6, &rule.label_layer)
                .bind(7, rule.enabled)
                .run()
                .map_err(|error| {
                    make_error(
                        error.code,
                        error.message,
                        format!("label rule={} {}", rule.name, error.context),
                    )
                })?;
        }

        let mut insert_linetype =
            database.prepare("INSERT INTO linetypes (name, description) VALUES (?1, ?2)")?;
        let mut insert_element = database.prepare(
            "INSERT INTO linetype_elements (linetype, position, length) VALUES (?1, ?2, ?3)",
        )?;
        for linetype in &contents.linetypes {
            Binder::new(&mut insert_linetype)
                .bind(1, &linetype.name)
                .bind(2, &linetype.description)
                .run()
                .map_err(|error| {
                    make_error(
                        error.code,
                        error.message,
                        format!("linetype={} {}", linetype.name, error.context),
                    )
                })?;
            for (i, element) in linetype.pattern.iter().enumerate() {
                Binder::new(&mut insert_element)
                    .bind(1, &linetype.name)
                    .bind(2, i as i64)
                    .bind(3, element.length)
                    .run()
                    .map_err(|error| {
                        make_error(
                            error.code,
                            error.message,
                            format!("linetype={} element={i}", linetype.name),
                        )
                    })?;
            }
        }

        let mut insert_pattern = database
            .prepare("INSERT INTO hatch_patterns (name, description, solid) VALUES (?1, ?2, ?3)")?;
        let mut insert_family = database.prepare(concat!(
            "INSERT INTO hatch_families (pattern, position, angle, spacing, line_offset)",
            " VALUES (?1, ?2, ?3, ?4, ?5)"
        ))?;
        for pattern in &contents.hatch_patterns {
            Binder::new(&mut insert_pattern)
                .bind(1, &pattern.name)
                .bind(2, &pattern.description)
                .bind(3, pattern.solid)
                .run()
                .map_err(|error| {
                    make_error(
                        error.code,
                        error.message,
                        format!("hatch pattern={} {}", pattern.name, error.context),
                    )
                })?;
            for (i, family) in pattern.families.iter().enumerate() {
                Binder::new(&mut insert_family)
                    .bind(1, &pattern.name)
                    .bind(2, i as i64)
                    .bind(3, family.angle)
                    .bind(4, family.spacing)
                    .bind(5, family.offset)
                    .run()
                    .map_err(|error| {
                        make_error(
                            error.code,
                            error.message,
                            format!("hatch pattern={} family={i}", pattern.name),
                        )
                    })?;
            }
        }

        let mut insert_alignment = database.prepare(
            "INSERT INTO alignments (name, description, start_station) VALUES (?1, ?2, ?3)",
        )?;
        let mut insert_pi = database.prepare(concat!(
            "INSERT INTO alignment_pis (alignment, position, x, y, radius, spiral_in, spiral_out)",
            " VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
        ))?;
        let mut insert_pvi = database.prepare(concat!(
            "INSERT INTO alignment_pvis (alignment, position, station, elevation, curve_length)",
            " VALUES (?1, ?2, ?3, ?4, ?5)"
        ))?;
        for alignment in &contents.alignments {
            Binder::new(&mut insert_alignment)
                .bind(1, &alignment.name)
                .bind(2, &alignment.description)
                .bind(3, alignment.horizontal.start_station)
                .run()
                .map_err(|error| {
                    make_error(
                        error.code,
                        error.message,
                        format!("alignment={} {}", alignment.name, error.context),
                    )
                })?;
            for (i, pi) in alignment.horizontal.pis.iter().enumerate() {
                Binder::new(&mut insert_pi)
                    .bind(1, &alignment.name)
                    .bind(2, i as i64)
                    .bind(3, pi.point.x)
                    .bind(4, pi.point.y)
                    .bind(5, pi.radius)
                    .bind(6, pi.spiral_in)
                    .bind(7, pi.spiral_out)
                    .run()
                    .map_err(|error| {
                        make_error(
                            error.code,
                            error.message,
                            format!("alignment={} pi={i}", alignment.name),
                        )
                    })?;
            }
            if let Some(vertical) = &alignment.vertical {
                for (i, pvi) in vertical.pvis.iter().enumerate() {
                    Binder::new(&mut insert_pvi)
                        .bind(1, &alignment.name)
                        .bind(2, i as i64)
                        .bind(3, pvi.station)
                        .bind(4, pvi.elevation)
                        .bind(5, pvi.curve_length)
                        .run()
                        .map_err(|error| {
                            make_error(
                                error.code,
                                error.message,
                                format!("alignment={} pvi={i}", alignment.name),
                            )
                        })?;
                }
            }
        }

        let mut insert_entity = database.prepare(concat!(
            "INSERT INTO entities (id, type, layer, style, color, visible, geometry, properties, ",
            "metadata, geometry_blob)",
            " VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
        ))?;
        for entity in &contents.entities {
            // A write that cannot be encoded fails the save cleanly instead of
            // aborting the process partway through the transaction.
            //
            // Geometry goes in as a binary blob; the legacy `geometry` TEXT
            // column is written empty. Storing the JSON as well would double
            // the cost of a save for a column nothing reads any more -
            // geometry_to_json is still there for exports and for inspecting a
            // database by hand. Rows written before schema 3 keep their JSON
            // and are read from it until the project is next saved.
            let geometry = geometry_to_blob(&entity.geometry)?;
            let properties = properties_to_json(&entity.properties)?;
            let metadata = properties_to_json(&entity.metadata)?;
            Binder::new(&mut insert_entity)
                .bind(1, entity.id as i64)
                .bind(2, to_string(entity.r#type()))
                .bind(3, &entity.layer)
                .bind(4, &entity.style)
                .color(5, entity.color)
                .bind(6, entity.visible)
                .bind(7, "")
                .bind(8, &properties)
                .bind(9, &metadata)
                .bind(10, geometry.as_slice())
                .run()
                .map_err(|error| {
                    make_error(
                        error.code,
                        error.message,
                        format!("entity id={} {}", entity.id, error.context),
                    )
                })?;
        }

        let mut insert_relationship = database.prepare(
            "INSERT INTO relationships (from_entity, to_entity, kind, data) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for relationship in &contents.relationships {
            Binder::new(&mut insert_relationship)
                .bind(1, relationship.from as i64)
                .bind(2, relationship.to as i64)
                .bind(3, &relationship.kind)
                .bind(4, &relationship.data)
                .run()?;
        }
        write_survey_jobs(database, &contents.survey_jobs)?;
        write_surfaces(database, &contents.surfaces)?;
        transaction.commit()
    }

    pub fn load(&self) -> Result<ProjectContents> {
        let database = &self.database;
        let mut contents = ProjectContents::default();

        for_each_row(database, "SELECT key, value FROM metadata", |row| {
            let key = text_column(row, 0, "metadata")?;
            let value = text_column(row, 1, "metadata")?;
            let m = &mut contents.metadata;
            match key.as_str() {
                "name" => m.name = value,
                "description" => m.description = value,
                "linear_unit" => m.linear_unit = value,
                "coordinate_system" => m.coordinate_system = value,
                "created_utc" => m.created_utc = value,
                "modified_utc" => m.modified_utc = value,
                "application_version" => m.application_version = value,
                "next_entity_id" => match stoull(&value) {
                    Some(next) => contents.next_entity_id = next,
                    None => {
                        return Err(make_error(
                            ErrorCode::DatabaseFailure,
                            "next_entity_id is not a number",
                            value,
                        ));
                    }
                },
                "customisation" => m.customisation = split_lines(&value),
                "reference_layers" => m.reference_layers = split_lines(&value),
                // A key from a newer build: kept, so this build's save writes
                // it back. Every key this build knows is handled above.
                _ => {
                    m.unknown_keys.entry(key).or_insert(value);
                }
            }
            Ok(())
        })?;

        for_each_row(
            database,
            concat!(
                "SELECT name, color, visible, locked, linetype, line_weight, dimension_style,",
                " hatch_pattern FROM layers ORDER BY name"
            ),
            |row| {
                const TABLE: &str = "layers";
                let name = text_column(row, 0, TABLE)?;
                let color = Color::from_hex(text_column_view(row, 1, TABLE)?)?;
                let visible = row.column_int64(2) != 0;
                let locked = row.column_int64(3) != 0;
                let linetype = text_column(row, 4, TABLE)?;
                let line_weight = row.column_double(5);
                let dimension_style = text_column(row, 6, TABLE)?;
                let hatch_pattern = text_column(row, 7, TABLE)?;
                contents.layers.push(Layer {
                    name,
                    color,
                    visible,
                    locked,
                    linetype,
                    line_weight,
                    dimension_style,
                    hatch_pattern,
                });
                Ok(())
            },
        )?;

        for_each_row(
            database,
            concat!(
                "SELECT name, text_height, text_gap, extension_offset, extension_beyond, arrow_size,",
                " arrow_head, unit_scale, prefix, suffix, decimals, round_to, suppress_zeros,",
                " paper_sized FROM dimension_styles ORDER BY name"
            ),
            |row| {
                const TABLE: &str = "dimension_styles";
                let name = text_column(row, 0, TABLE)?;
                let text_height = row.column_double(1);
                let text_gap = row.column_double(2);
                let extension_offset = row.column_double(3);
                let extension_beyond = row.column_double(4);
                let arrow_size = row.column_double(5);
                let arrow_head = text_column_view(row, 6, TABLE)
                    .and_then(arrow_head_from_string)
                    .map_err(|error| {
                        make_error(error.code, error.message, format!("dimension style={name}"))
                    })?;
                let unit_scale = row.column_double(7);
                let prefix = text_column(row, 8, TABLE)?;
                let suffix = text_column(row, 9, TABLE)?;
                let decimals = row.column_int64(10) as i32;
                let round_to = row.column_double(11);
                let suppress_trailing_zeros = row.column_int64(12) != 0;
                let paper_sized = row.column_int64(13) != 0;
                contents.dimension_styles.push(DimensionStyle {
                    name,
                    text_height,
                    text_gap,
                    extension_offset,
                    extension_beyond,
                    arrow_size,
                    arrow_head,
                    unit_scale,
                    prefix,
                    suffix,
                    decimals,
                    round_to,
                    suppress_trailing_zeros,
                    paper_sized,
                });
                Ok(())
            },
        )?;

        for_each_row(
            database,
            concat!(
                "SELECT name, font_family, paper_height, width_factor, oblique, bold, italic, color,",
                " mask, mask_margin, readable, line_spacing FROM text_styles ORDER BY name"
            ),
            |row| {
                const TABLE: &str = "text_styles";
                let name = text_column(row, 0, TABLE)?;
                let font_family = text_column(row, 1, TABLE)?;
                let paper_height = row.column_double(2);
                let width_factor = row.column_double(3);
                let oblique = row.column_double(4);
                let bold = row.column_int64(5) != 0;
                let italic = row.column_int64(6) != 0;
                let colour = text_column(row, 7, TABLE)?;
                let color = if colour.is_empty() {
                    None
                } else {
                    Some(Color::from_hex(&colour).map_err(|error| {
                        make_error(error.code, error.message, format!("text style={name}"))
                    })?)
                };
                let mask = row.column_int64(8) != 0;
                let mask_margin = row.column_double(9);
                let readable = row.column_int64(10) != 0;
                let line_spacing = row.column_double(11);
                contents.text_styles.push(TextStyle {
                    name,
                    font_family,
                    paper_height,
                    width_factor,
                    oblique,
                    bold,
                    italic,
                    color,
                    mask,
                    mask_margin,
                    readable,
                    line_spacing,
                });
                Ok(())
            },
        )?;

        for_each_row(
            database,
            "SELECT name, kind, definition FROM label_styles ORDER BY name",
            |row| {
                const TABLE: &str = "label_styles";
                let name = text_column(row, 0, TABLE)?;
                let kind = text_column_view(row, 1, TABLE)
                    .and_then(label_kind_from_string)
                    .map_err(|error| {
                        make_error(error.code, error.message, format!("label style={name}"))
                    })?;
                let mut style = LabelStyle {
                    name,
                    kind,
                    ..LabelStyle::default()
                };
                let read = text_column_view(row, 2, TABLE).and_then(|definition| {
                    label_style_definition_from_json(definition, &mut style)
                });
                read.map_err(|error| {
                    make_error(
                        error.code,
                        error.message,
                        format!("label style={} {}", style.name, error.context),
                    )
                })?;
                contents.label_styles.push(style);
                Ok(())
            },
        )?;

        for_each_row(
            database,
            concat!(
                "SELECT name, label_style, layer, code, entity_type, label_layer, enabled",
                " FROM label_rules ORDER BY name"
            ),
            |row| {
                const TABLE: &str = "label_rules";
                let name = text_column(row, 0, TABLE)?;
                let label_style = text_column(row, 1, TABLE)?;
                let layer = text_column(row, 2, TABLE)?;
                let code = text_column(row, 3, TABLE)?;
                let entity_type = text_column(row, 4, TABLE)?;
                let label_layer = text_column(row, 5, TABLE)?;
                let enabled = row.column_int64(6) != 0;
                contents.label_rules.push(LabelRule {
                    name,
                    label_style,
                    layer,
                    code,
                    entity_type,
                    label_layer,
                    enabled,
                });
                Ok(())
            },
        )?;

        for_each_row(
            database,
            "SELECT name, description FROM linetypes ORDER BY name",
            |row| {
                const TABLE: &str = "linetypes";
                let name = text_column(row, 0, TABLE)?;
                let description = text_column(row, 1, TABLE)?;
                contents.linetypes.push(Linetype {
                    name,
                    description,
                    pattern: Vec::new(),
                });
                Ok(())
            },
        )?;
        // Elements in one pass, ordered by position, matched to the definition
        // by name. A pattern whose definition row is missing is dropped rather
        // than resurrected under an empty name.
        for_each_row(
            database,
            "SELECT linetype, length FROM linetype_elements ORDER BY linetype, position",
            |row| {
                let name = text_column(row, 0, "linetype_elements")?;
                let Some(found) = contents
                    .linetypes
                    .iter_mut()
                    .find(|linetype| linetype.name == name)
                else {
                    return Err(make_error(
                        ErrorCode::ParseFailure,
                        "a linetype element names a definition that is not there",
                        name,
                    ));
                };
                found.pattern.push(LinetypeElement {
                    length: row.column_double(1),
                });
                Ok(())
            },
        )?;

        for_each_row(
            database,
            "SELECT name, description, solid FROM hatch_patterns ORDER BY name",
            |row| {
                const TABLE: &str = "hatch_patterns";
                let name = text_column(row, 0, TABLE)?;
                let description = text_column(row, 1, TABLE)?;
                let solid = row.column_int64(2) != 0;
                contents.hatch_patterns.push(HatchPattern {
                    name,
                    description,
                    solid,
                    families: Vec::new(),
                });
                Ok(())
            },
        )?;

        // Ordered by position so the families come back in the order they
        // were written; a crosshatch is two families and their order is part
        // of the definition, not an accident of the table.
        for_each_row(
            database,
            concat!(
                "SELECT pattern, angle, spacing, line_offset FROM hatch_families",
                " ORDER BY pattern, position"
            ),
            |row| {
                let name = text_column(row, 0, "hatch_families")?;
                // The patterns were read ORDER BY name, SQLite's binary order,
                // which is the order of the names' bytes.
                let index = contents
                    .hatch_patterns
                    .partition_point(|pattern| pattern.name < name);
                let Some(found) = contents
                    .hatch_patterns
                    .get_mut(index)
                    .filter(|pattern| pattern.name == name)
                else {
                    // The foreign key makes this unreachable; reporting rather
                    // than dropping the row is what stops a future schema
                    // change from silently losing families (PLAN.MD section
                    // 36).
                    return Err(make_error(
                        ErrorCode::DatabaseFailure,
                        "a hatch family names a pattern that was not read",
                        name,
                    ));
                };
                found.families.push(HatchLineFamily {
                    angle: row.column_double(1),
                    spacing: row.column_double(2),
                    offset: row.column_double(3),
                });
                Ok(())
            },
        )?;

        for_each_row(
            database,
            "SELECT name, description, start_station FROM alignments ORDER BY name",
            |row| {
                const TABLE: &str = "alignments";
                let name = text_column(row, 0, TABLE)?;
                let description = text_column(row, 1, TABLE)?;
                let start_station = row.column_double(2);
                contents.alignments.push(Alignment {
                    name,
                    description,
                    horizontal: HorizontalAlignment {
                        pis: Vec::new(),
                        start_station,
                    },
                    vertical: None,
                });
                Ok(())
            },
        )?;

        // Ordered by position: the PI order IS the alignment.
        for_each_row(
            database,
            concat!(
                "SELECT alignment, x, y, radius, spiral_in, spiral_out FROM alignment_pis",
                " ORDER BY alignment, position"
            ),
            |row| {
                let name = text_column(row, 0, "alignment_pis")?;
                let index = contents
                    .alignments
                    .partition_point(|alignment| alignment.name < name);
                let Some(found) = contents
                    .alignments
                    .get_mut(index)
                    .filter(|alignment| alignment.name == name)
                else {
                    return Err(make_error(
                        ErrorCode::DatabaseFailure,
                        "an alignment PI names an alignment that was not read",
                        name,
                    ));
                };
                found.horizontal.pis.push(AlignmentPi {
                    point: Point2::new(row.column_double(1), row.column_double(2)),
                    radius: row.column_double(3),
                    spiral_in: row.column_double(4),
                    spiral_out: row.column_double(5),
                });
                Ok(())
            },
        )?;

        // The profile exists only when it has rows: the first row creates it,
        // so an alignment with none keeps `vertical` empty rather than
        // acquiring a profile of no PVIs that would then fail to solve.
        for_each_row(
            database,
            concat!(
                "SELECT alignment, station, elevation, curve_length FROM alignment_pvis",
                " ORDER BY alignment, position"
            ),
            |row| {
                let name = text_column(row, 0, "alignment_pvis")?;
                let index = contents
                    .alignments
                    .partition_point(|alignment| alignment.name < name);
                let Some(found) = contents
                    .alignments
                    .get_mut(index)
                    .filter(|alignment| alignment.name == name)
                else {
                    return Err(make_error(
                        ErrorCode::DatabaseFailure,
                        "a profile PVI names an alignment that was not read",
                        name,
                    ));
                };
                found
                    .vertical
                    .get_or_insert_default()
                    .pvis
                    .push(ProfilePvi {
                        station: row.column_double(1),
                        elevation: row.column_double(2),
                        curve_length: row.column_double(3),
                    });
                Ok(())
            },
        )?;

        for_each_row(
            database,
            concat!(
                "SELECT name, color, line_weight, linetype, hatch_pattern, description,",
                " symbol, symbol_size FROM styles ORDER BY name"
            ),
            |row| {
                const TABLE: &str = "styles";
                let name = text_column(row, 0, TABLE)?;
                let color = read_color(row, 1, TABLE)?;
                let line_weight = row.column_double(2);
                let linetype = text_column(row, 3, TABLE)?;
                let hatch_pattern = text_column(row, 4, TABLE)?;
                let description = text_column(row, 5, TABLE)?;
                let symbol = text_column(row, 6, TABLE)?;
                let symbol_size = row.column_double(7);
                contents.styles.push(Style {
                    name,
                    color,
                    line_weight,
                    linetype,
                    hatch_pattern,
                    description,
                    symbol,
                    symbol_size,
                });
                Ok(())
            },
        )?;

        for_each_row(
            database,
            "SELECT name, type, description, default_value FROM property_definitions ORDER BY name",
            |row| {
                const TABLE: &str = "property_definitions";
                let name = text_column(row, 0, TABLE)?;
                let r#type = property_type_from_string(text_column_view(row, 1, TABLE)?)?;
                let description = text_column(row, 2, TABLE)?;
                let mut default_value = None;
                if !row.column_is_null(3) {
                    let parsed = properties_from_json(text_column_view(row, 3, TABLE)?)?;
                    default_value = parsed.get("value").cloned();
                }
                contents.property_definitions.push(PropertyDefinition {
                    name,
                    r#type,
                    description,
                    default_value,
                });
                Ok(())
            },
        )?;

        // Sized up front: every reallocation of this vector moves every
        // Entity in it, and an Entity carries two maps. COUNT(*) is one extra
        // b-tree walk; contents.next_entity_id would be free but is only an
        // upper bound, and a drawing that has had most of its entities deleted
        // would reserve for the ids rather than for the rows.
        let mut entity_count: i64 = 0;
        for_each_row(database, "SELECT COUNT(*) FROM entities", |row| {
            entity_count = row.column_int64(0);
            Ok(())
        })?;
        if entity_count > 0 {
            contents.entities.reserve(entity_count as usize);
        }

        for_each_row(
            database,
            concat!(
                "SELECT id, layer, style, color, visible, geometry, properties, metadata, geometry_blob",
                " FROM entities ORDER BY id"
            ),
            |row| {
                const TABLE: &str = "entities";
                let id = row.column_int64(0) as EntityId;
                // Built only when something fails: on a sound project this
                // would run once per row - 50k string allocations - to
                // describe an error that never happened.
                let context = || format!("entity id={id}");
                // The text and blob columns below are BORROWED from SQLite's
                // row buffer and parsed before the statement steps on; see
                // SqliteStatement::column_text_view for the lifetime rule.
                let layer = text_column(row, 1, TABLE)?;
                let style = text_column(row, 2, TABLE)?;
                let color = read_color(row, 3, TABLE)?;
                let visible = row.column_int64(4) != 0;
                // The blob is the record from schema 3 onwards; the JSON column
                // is read only for rows written before it, which are converted
                // the next time the project is saved. Preferring the blob when
                // both are present means a half-migrated database still reads
                // the newer of the two rather than the staler.
                let decoded = if !row.column_is_null(8) {
                    geometry_from_blob(row.column_blob_span(8))
                } else {
                    text_column_view(row, 5, TABLE).and_then(geometry_from_json)
                };
                let geometry = decoded.map_err(|error| {
                    make_error(
                        error.code,
                        error.message,
                        format!("{} {}", context(), error.context),
                    )
                })?;
                let properties = text_column_view(row, 6, TABLE)
                    .and_then(properties_from_json)
                    .map_err(|error| make_error(error.code, error.message, context()))?;
                let metadata = text_column_view(row, 7, TABLE)
                    .and_then(properties_from_json)
                    .map_err(|error| make_error(error.code, error.message, context()))?;
                contents.entities.push(Entity {
                    id,
                    geometry,
                    layer,
                    style,
                    color,
                    visible,
                    properties,
                    metadata,
                });
                Ok(())
            },
        )?;

        for_each_row(
            database,
            "SELECT from_entity, to_entity, kind, data FROM relationships ORDER BY id",
            |row| {
                const TABLE: &str = "relationships";
                let from = row.column_int64(0) as EntityId;
                let to = row.column_int64(1) as EntityId;
                let kind = text_column(row, 2, TABLE)?;
                let data = text_column(row, 3, TABLE)?;
                contents.relationships.push(Relationship {
                    from,
                    to,
                    kind,
                    data,
                });
                Ok(())
            },
        )?;
        contents.survey_jobs = read_survey_jobs(database)?;
        contents.surfaces = read_surfaces(database)?;
        Ok(contents)
    }

    /// Snapshot into backups/, pruned to the newest `keep` files
    /// (DEFAULT_BACKUPS_TO_KEEP, the C++'s default). Returns the path.
    pub fn backup(&mut self, keep: usize) -> Result<PathBuf> {
        let directory = self.directory.join(BACKUP_DIRECTORY);
        if let Err(error) = std::fs::create_dir_all(directory.as_path()) {
            return Err(make_error(
                ErrorCode::DatabaseFailure,
                "could not create the backup directory",
                system_message(&error),
            ));
        }
        let mut target = directory.join(format!("project-{}.db", utc_timestamp(true)));
        let mut suffix = 1;
        while target.exists() {
            // The same millisecond.
            target = directory.join(format!("project-{}-{suffix}.db", utc_timestamp(true)));
            suffix += 1;
        }
        if let Err(error) = self.database.backup_to(&target) {
            let _ = std::fs::remove_file(target.as_path());
            return Err(error);
        }
        for stale in backups_newest_first(&self.directory).iter().skip(keep) {
            let _ = std::fs::remove_file(stale);
        }
        Ok(target)
    }

    /// Newest first.
    pub fn list_backups(&self) -> Vec<PathBuf> {
        backups_newest_first(&self.directory)
    }

    pub fn schema_version(&self) -> Result<i32> {
        self.database.user_version()
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn database_path(&self) -> PathBuf {
        self.directory.join(DATABASE_FILE)
    }
}
