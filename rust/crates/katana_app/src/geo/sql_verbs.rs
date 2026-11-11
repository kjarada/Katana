//! GIS SQL (docs/geoprocessing.md, "V5"), the port of
//! src/katana_app/geo/sql_verbs.cpp: the drawing queried with SQL - SQLite with
//! Spatialite's functions by default, or OGR's own SQL - through GDAL's
//! `vector sql`, over the tables the scope becomes.
//!
//! ```text
//! GIS SQL "<select>" [<scope>] [dialect=sqlite|ogrsql]
//!         [AS REPORT | AS SELECT | AS LAYER <layer>] [csv=<file>] [OVERWRITE] [PREVIEW]
//! ```
//!
//! The tables are drawing_dataset's: points, lines and polygons, each with
//! katana_id, layer, style, colour and type, then the entities' properties as
//! typed columns. AS REPORT (the default) answers the rows; AS SELECT selects
//! the entities the katana_id column names; AS LAYER draws the geometry the
//! query returns, as one undo step. AS is TO: TO REPORT, TO SELECTION and TO
//! LAYER say the same, and are the GDAL verb's too (bindings.rs).
//!
//! A query reads; it never writes. Only a SELECT is run, one statement, and
//! SPATIALITE_SECURITY is never set, so Spatialite's file functions -
//! BlobToFile, ExportGeoJSON ... - stay disabled.

use std::time::Instant;

use katana_core::error::make_error;
use katana_core::text::is_ascii_space;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::layer_path::validate_layer_path;
use katana_gis::processing::RunOutputs;
use katana_interop::geo::ResultOptions;

use super::bindings::{ApplyRequest, Target, TargetKind, Tokens, apply_outputs, scope_record};
use super::geo_verbs::{Context, Prepared, make_apply, make_work, trimmed_text};
use super::replies::{value, warning_record};
use super::vector_support::{
    Row, RowOptions, WordRules, answered, bind_scope, choice_option, gis_record, joined,
    not_run_record, read_verb_words, row_key, rows_of, run_vector, write_csv,
};
use crate::import_records::path_from_text;

const USAGE: &str = concat!(
    "GIS SQL \"<select>\" [<scope>] [dialect=sqlite|ogrsql] [AS REPORT | AS SELECT | AS LAYER ",
    "<layer>] [csv=<file>] [OVERWRITE] [PREVIEW]",
);

/// Only one SELECT: a query reads, and a second statement after a ';' could
/// be anything.
fn check_statement(sql: &str) -> Status {
    let body = trimmed_text(sql)
        .trim_end_matches(|c: char| c == ';' || (c.is_ascii() && is_ascii_space(c as u8)));
    if body.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("GIS SQL needs a statement: {USAGE}"),
        ));
    }
    let blank = body
        .find([' ', '\t', '\r', '\n', '('])
        .unwrap_or(body.len());
    if !body[..blank].eq_ignore_ascii_case("select") {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "GIS SQL runs a SELECT and nothing else: a query reads the drawing, it never changes \
             it",
            &body[..blank],
        ));
    }
    // A ';' outside a quoted literal begins a second statement.
    let mut quote = 0u8;
    for c in body.bytes() {
        if quote != 0 {
            if c == quote {
                quote = 0;
            }
        } else if c == b'\'' || c == b'"' || c == b'`' {
            quote = c;
        } else if c == b';' {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "GIS SQL runs one statement",
                sql,
            ));
        }
    }
    Ok(())
}

fn prepare_sql(context: &mut Context<'_>, tokens: &Tokens, line: &str) -> Result<Prepared> {
    if tokens.size() < 3 || !tokens.quoted[2] {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("the statement is one quoted word: {USAGE}"),
            if tokens.size() > 2 {
                tokens[2].clone()
            } else {
                String::new()
            },
        ));
    }
    let sql = tokens[2].clone();
    check_statement(&sql)?;
    let rules = WordRules {
        options: Vec::from(["dialect", "csv"].map(String::from)),
        flags: Vec::from(["OVERWRITE", "PREVIEW"].map(String::from)),
        target: true,
        usage: String::from(USAGE),
        target_words: Vec::from(["AS", "TO"].map(String::from)),
    };
    // AS SELECT is the target parser's SELECTION: the word SQL speaks.
    let mut read = tokens.clone();
    for i in 4..read.size() {
        if read.is(i, "SELECT") && read.is(i - 1, "AS") {
            read.words[i] = String::from("SELECTION");
        }
    }
    let words = read_verb_words(&read, 3, read.size(), &rules)?;
    let dialect = choice_option(&words, "dialect", &["sqlite", "ogrsql"], "sqlite")?;
    let mut target = Target {
        kind: TargetKind::Report,
        ..Target::default()
    };
    if let Some(given) = &words.target {
        target = given.clone();
        if !matches!(
            target.kind,
            TargetKind::Report | TargetKind::Selection | TargetKind::Layer
        ) {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "GIS SQL answers AS REPORT, AS SELECT or AS LAYER <layer>",
            ));
        }
        if target.kind == TargetKind::Layer
            && let Err(valid) = validate_layer_path(&target.name)
        {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("AS LAYER needs a layer path: {}", valid.message),
                target.name.clone(),
            ));
        }
    }
    let mut csv = String::new();
    if let Some(file) = words.option("csv") {
        if target.kind != TargetKind::Report {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "csv= writes a report's rows: AS REPORT",
                format!("csv={file}"),
            ));
        }
        csv = file.clone();
        if path_from_text(&csv).exists() && !words.has("OVERWRITE") {
            return Err(make_error(
                ErrorCode::AlreadyExists,
                "the file exists; add OVERWRITE to replace it",
                csv,
            ));
        }
    }
    let bound = bind_scope(context, &words.scope)?;
    let mut records = vec![scope_record("input", &bound)];
    for warning in &bound.dataset.stats.warnings {
        records.push(warning_record(warning));
    }
    let names: Vec<&str> = bound
        .dataset
        .set
        .tables
        .iter()
        .map(|table| table.name.as_str())
        .collect();
    let tables = super::replies::joined(&names, ",");
    let settings = format!("sql dialect={dialect} tables={}", value(&tables));
    if bound.dataset.set.tables.is_empty() {
        records.insert(0, not_run_record("sql", ""));
        records.push(format!("{settings} rows=0"));
        return Ok(answered("GIS SQL", joined(&records)));
    }
    if words.has("PREVIEW") {
        records.insert(0, String::from("gis op=sql preview=yes"));
        records.push(settings);
        records.push(String::from("preview valid=yes changed=no"));
        return Ok(answered("GIS SQL", joined(&records)));
    }

    let input = bound.dataset.set;
    let gdal_words = [
        format!("--sql={sql}"),
        format!(
            "--dialect={}",
            if dialect == "sqlite" {
                "SQLITE"
            } else {
                "OGRSQL"
            }
        ),
    ];
    let command_name = String::from(trimmed_text(line));
    let work = make_work(move |stop, progress| {
        let start = Instant::now();
        let mut warnings: Vec<String> = Vec::new();
        let result = run_vector(
            &["vector", "sql"].map(String::from),
            &input,
            &gdal_words,
            stop,
            progress,
            &mut warnings,
            &[],
        )?;
        let rows: usize = result.tables.iter().map(|table| table.features.len()).sum();
        let summary = format!(
            "{settings} rows={rows}{}",
            if csv.is_empty() {
                String::new()
            } else {
                format!(" csv={}", value(&csv))
            }
        );
        let seconds = start.elapsed().as_secs_f64();
        Ok(make_apply(move |context| {
            if !csv.is_empty() {
                let mut lines: Vec<Row> = Vec::new();
                let mut columns: Vec<String> = Vec::new();
                for table in &result.tables {
                    columns.extend(table.fields.iter().map(|field| row_key(&field.name)));
                    lines.extend(rows_of(table, &RowOptions::default()));
                }
                write_csv(&csv, &lines, &columns)?;
            }
            // A query of the drawing holds its katana_id: a row drawn back
            // names the entity it came from, not a copy of it.
            let target_layer = if target.kind == TargetKind::Layer {
                target.name.clone()
            } else {
                String::new()
            };
            let request = ApplyRequest {
                target,
                default_name: String::from("sql"),
                result: ResultOptions {
                    target_layer,
                    operation: String::from("vector sql"),
                    command_name,
                    ..ResultOptions::default()
                },
                ..ApplyRequest::default()
            };
            let outputs = RunOutputs {
                features: Some(result),
                ..RunOutputs::default()
            };
            let applied = apply_outputs(context, &request, outputs)?;
            let mut reply = vec![gis_record("sql", seconds)];
            reply.extend(records);
            if !applied.is_empty() {
                reply.push(applied);
            }
            reply.push(summary);
            for warning in &warnings {
                reply.push(warning_record(warning));
            }
            Ok(joined(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from("GIS SQL"),
        reply: None,
        work: Some(work),
    })
}

/// GIS SQL, its row in the verb table's V5 block (verb_table.rs).
pub fn prepare_gis_sql(context: &mut Context<'_>, tokens: &Tokens, line: &str) -> Result<Prepared> {
    prepare_sql(context, tokens, line)
}
