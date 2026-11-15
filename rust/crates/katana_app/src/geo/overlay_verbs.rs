//! GIS OVERLAY (docs/geoprocessing.md, "V2"), the port of
//! src/katana_app/geo/overlay_verbs.cpp: the polygon booleans Katana's
//! geometry did not have (geometry's polygon module) - easement area per lot,
//! net developable area, lots split by zone or flood extent, pipe length per
//! lot - through GDAL's `vector layer-algebra`, between the scope and a second
//! scope or a file.
//!
//! ```text
//! GIS OVERLAY intersection|difference|union|symdifference|identity|update|clip
//!     <scope> WITH (<scope> | FILE <path> [LAYER <name>] [where="<sql>"])
//!     [keep=a,b|all|none] [keepwith=a,b|all|none] [TO LAYER <path>]
//!     [csv=<file>] [OVERWRITE] [PREVIEW]
//! ```
//!
//! The second clause is read by the same scope parser as the first: the
//! subject's words end at WITH, which is no scope word, and the overlay's
//! begin after it (vector_support's reading of a clause).
//!
//! What was measured, and so is relied on here:
//!
//! ```text
//! - layer-algebra reads ONE input layer and ONE method layer ("Cannot get
//!   input layer ''" for a dataset of several), so each of the subject's
//!   tables is its own run, and the overlay is its areas;
//! - it names the fields it carries input_<field> and method_<field>;
//! - a line against areas comes back cut at their edges, which is how a
//!   pipe's length per lot is measured.
//! ```

use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::layer_path::validate_layer_path;
use katana_gis::GeometryKind;
use katana_gis::processing::{
    self as gp, ArgValue, DatasetPath, DatasetValue, Feature, FeatureSet, FeatureTable, FieldDef,
    FieldValue, RunRequest,
};
use katana_interop::geo::ResultOptions;
use katana_interop::geo::drawing_dataset::{is_bookkeeping_field, result_command};

use super::bindings::{Tokens, scope_record};
use super::geo_verbs::{Context, Prepared, Progress, make_apply, make_work, trimmed_text};
use super::replies::{fixed3, value, warning_record};
use super::vector_support::{
    Applied, Row, RowOptions, WordRules, answered, bind_scope, execute_step, gis_record, joined,
    layer_target, list_of, measure, merged_table, not_run_record, output_record, progress_span,
    read_verb_words, row_record, rows_of, run_vector, table_named, without_empty, write_csv,
};
use crate::import_records::path_from_text;

const USAGE: &str = concat!(
    "GIS OVERLAY intersection|difference|union|symdifference|identity|update|clip <scope> WITH ",
    "(<scope> | FILE <path> [LAYER <name>] [where=\"<sql>\"]) [keep=a,b|all|none] ",
    "[keepwith=a,b|all|none] [TO LAYER <path>] [csv=<file>] [OVERWRITE] [PREVIEW]",
);

/// Katana's word for an operation, and GDAL's. GDAL's own are taken too.
const OPERATIONS: [(&str, &str); 9] = [
    ("intersection", "intersection"),
    ("difference", "erase"),
    ("union", "union"),
    ("symdifference", "sym-difference"),
    ("identity", "identity"),
    ("update", "update"),
    ("clip", "clip"),
    ("erase", "erase"),
    ("sym-difference", "sym-difference"),
];

/// The operations whose result holds pieces of the overlay the subject does
/// not reach: run once per subject table they would repeat those pieces, so
/// they take the subject's areas only.
fn areas_only(gdal: &str) -> bool {
    matches!(gdal, "union" | "sym-difference" | "update")
}

/// --input-field / --method-field words for keep= / keepwith=. katana_id is
/// always carried: it is how a piece names the entities it came from.
fn field_words(keep: Option<&str>, side: &str) -> Result<Vec<String>> {
    let Some(keep) = keep else {
        return Ok(Vec::new());
    };
    if keep.eq_ignore_ascii_case("all") {
        return Ok(Vec::new());
    }
    let none = keep.eq_ignore_ascii_case("none");
    let mut fields = if none { Vec::new() } else { list_of(keep) };
    if fields.is_empty() && !none {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "keep= and keepwith= name the properties to carry, or all or none",
            keep,
        ));
    }
    if !fields.contains(&String::from("katana_id")) {
        fields.insert(0, String::from("katana_id"));
    }
    Ok(fields
        .iter()
        .map(|field| format!("--{side}-field={field}"))
        .collect())
}

/// A field's name as layer-algebra wrote it: the side it came from ("input",
/// "method" or none) and its own name.
fn split_name(name: &str) -> (&str, &str) {
    if let Some(base) = name.strip_prefix("input_") {
        ("input", base)
    } else if let Some(base) = name.strip_prefix("method_") {
        ("method", base)
    } else {
        ("", name)
    }
}

/// The fields a piece carries, as the drawing's properties: input_owner is
/// owner where only one side has an owner; katana_id names the subject
/// (gis.source when drawn) and the overlay's becomes gis.with. Dropped from
/// either side, prefixed or not (difference, update and clip give the
/// subject's fields unprefixed): the drawing's bookkeeping (layer, style,
/// colour, type), and gis.* provenance - an overlay made by another verb
/// carries gis.op and gis.source, which on a row read as the piece's own, and
/// the piece drawn is given its own.
fn renamed(table: &FeatureTable) -> FeatureTable {
    // Base name -> the sides it appears on.
    let mut sides: BTreeMap<&str, i32> = BTreeMap::new();
    for field in &table.fields {
        *sides.entry(split_name(&field.name).1).or_insert(0) += 1;
    }
    let mut out = FeatureTable {
        name: table.name.clone(),
        kind: table.kind,
        has_z: table.has_z,
        crs_wkt: table.crs_wkt.clone(),
        ..FeatureTable::default()
    };
    let mut kept_from = Vec::new();
    for (f, field) in table.fields.iter().enumerate() {
        let (side, base) = split_name(&field.name);
        let name = if base == "katana_id" {
            if side == "method" {
                "gis.with"
            } else {
                "katana_id"
            }
        } else if is_bookkeeping_field(base) || base.starts_with("gis.") {
            continue;
        } else if !side.is_empty() && sides.get(base) == Some(&1) {
            base
        } else {
            field.name.as_str()
        };
        out.fields.push(FieldDef {
            name: String::from(name),
            r#type: field.r#type,
        });
        kept_from.push(f);
    }
    for feature in &table.features {
        let values = kept_from
            .iter()
            .map(|&f| feature.values.get(f).cloned().unwrap_or(FieldValue::Null))
            .collect();
        out.features.push(Feature {
            parts: feature.parts.clone(),
            values,
        });
    }
    out
}

/// Where the overlay comes from: a second scope, or a file read on the
/// worker.
#[derive(Clone, Debug, Default, PartialEq)]
struct Overlay {
    /// The second scope's areas.
    drawn: Option<FeatureSet>,
    /// FILE: its path, layer and where= clause.
    path: String,
    layer: String,
    r#where: String,
}

/// The overlay's features, read where they are: the second scope's areas as
/// they were copied, or the file through GDAL's own filter.
fn method_set(
    overlay: &Overlay,
    stop: Option<&AtomicBool>,
    progress: Progress<'_>,
    warnings: &mut Vec<String>,
) -> Result<FeatureSet> {
    if let Some(drawn) = &overlay.drawn {
        return Ok(drawn.clone());
    }
    // The file read here, on the worker, through GDAL's own filter: one
    // layer, the where= clause GDAL's SQL reads.
    let mut read = RunRequest {
        path: ["vector", "filter"].map(String::from).to_vec(),
        ..RunRequest::default()
    };
    let file = DatasetPath {
        path: overlay.path.clone(),
        layer: overlay.layer.clone(),
        ..DatasetPath::default()
    };
    read.values.push((
        String::from("input"),
        ArgValue::Dataset(DatasetValue::Path(file)),
    ));
    if !overlay.r#where.is_empty() {
        read.tokens.push(format!("--where={}", overlay.r#where));
    }
    let outputs = gp::run(&read, stop, progress_span(progress, 0.0, 0.2).as_deref())?;
    let mut from_file = outputs.features.unwrap_or_default();
    if from_file.tables.len() > 1 {
        warnings.push(format!(
            "the file has {} layers; the first, {}, is overlaid - LAYER <name> names another",
            from_file.tables.len(),
            from_file.tables[0].name
        ));
        from_file.tables.truncate(1);
    }
    if let Some(first) = from_file.tables.first()
        && first.kind != GeometryKind::Polygon
    {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the file's layer holds no areas to overlay",
            first.name.clone(),
        ));
    }
    Ok(from_file)
}

fn prepare_overlay(context: &mut Context<'_>, tokens: &Tokens, line: &str) -> Result<Prepared> {
    if tokens.size() < 3 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("usage: {USAGE}"),
        ));
    }
    let asked = tokens[2].to_ascii_lowercase();
    let operation = OPERATIONS.iter().find(|(katana, _)| *katana == asked);
    let Some(&(_, gdal_operation)) = operation.filter(|_| !tokens.quoted[2]) else {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "GIS OVERLAY takes intersection, difference, union, symdifference, identity, update \
             or clip first",
            tokens[2].clone(),
        ));
    };
    let with = (3..tokens.size())
        .find(|&i| tokens.is(i, "WITH"))
        .unwrap_or(tokens.size());
    if with == tokens.size() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("GIS OVERLAY needs WITH and what to overlay: {USAGE}"),
        ));
    }
    let rules = WordRules {
        options: Vec::from(["keep", "keepwith", "csv", "where"].map(String::from)),
        flags: Vec::from(["OVERWRITE", "PREVIEW"].map(String::from)),
        target: true,
        usage: String::from(USAGE),
        ..WordRules::default()
    };
    let subject_words = read_verb_words(tokens, 3, with, &rules)?;
    // After WITH: FILE <path> [LAYER <name>], or a scope; options anywhere.
    let mut from = with + 1;
    let mut overlay = Overlay::default();
    if tokens.is(from, "FILE") {
        if from + 1 >= tokens.size() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "FILE needs a path",
                "FILE",
            ));
        }
        overlay.path = tokens[from + 1].clone();
        from += 2;
        if tokens.is(from, "LAYER") {
            if from + 1 >= tokens.size() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "LAYER needs the file's layer name",
                    "LAYER",
                ));
            }
            overlay.layer = tokens[from + 1].clone();
            from += 2;
        }
    }
    let with_words = read_verb_words(tokens, from, tokens.size(), &rules)?;
    if !overlay.path.is_empty() && with_words.scope_given {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "WITH takes a scope or FILE <path>, not both",
        ));
    }
    if overlay.path.is_empty() && !with_words.scope_given {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("WITH needs what to overlay: a scope or FILE <path>: {USAGE}"),
        ));
    }
    // One set of options for the line, wherever they stood.
    let mut words = subject_words;
    for (key, text) in &with_words.options {
        if words.options.insert(key.clone(), text.clone()).is_some() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("{key}= is given twice"),
                key.clone(),
            ));
        }
    }
    for flag in &with_words.flags {
        if !words.flags.insert(flag.clone()) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("{flag} is given twice"),
                flag.clone(),
            ));
        }
    }
    if let Some(target) = &with_words.target {
        if words.target.is_some() {
            return Err(Error::new(ErrorCode::InvalidArgument, "one TO per line"));
        }
        words.target = Some(target.clone());
    }
    if let Some(filter) = words.option("where") {
        if overlay.path.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "where= filters a FILE overlay; a drawn one takes WHERE conditions",
                format!("where={filter}"),
            ));
        }
        overlay.r#where = filter.clone();
    }
    let input = field_words(words.option("keep").map(String::as_str), "input")?;
    let method = field_words(words.option("keepwith").map(String::as_str), "method")?;
    let layer = layer_target(&words, "GIS OVERLAY", "gis/overlay")?;
    if let Err(valid) = validate_layer_path(&layer) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("TO LAYER needs a layer path: {}", valid.message),
            layer,
        ));
    }
    let mut csv = String::new();
    if let Some(file) = words.option("csv") {
        csv = file.clone();
        if path_from_text(&csv).exists() && !words.has("OVERWRITE") {
            return Err(make_error(
                ErrorCode::AlreadyExists,
                "the file exists; add OVERWRITE to replace it",
                csv,
            ));
        }
    }

    let subject = bind_scope(context, &words.scope)?;
    let mut records = vec![scope_record("input", &subject)];
    for warning in &subject.dataset.stats.warnings {
        records.push(warning_record(warning));
    }
    let mut took_nothing = subject.dataset.set.tables.is_empty();
    if overlay.path.is_empty() {
        let drawn = bind_scope(context, &with_words.scope)?;
        records.push(scope_record("method", &drawn));
        let mut areas = FeatureSet::default();
        if let Some(polygons) = table_named(&drawn.dataset.set, "polygons") {
            areas.tables.push(polygons.clone());
        }
        let not_areas = drawn.dataset.stats.points + drawn.dataset.stats.lines;
        if not_areas != 0 {
            records.push(warning_record(&format!(
                "{not_areas} points and lines WITH takes are not areas and are not overlaid"
            )));
        }
        took_nothing = took_nothing || areas.tables.is_empty();
        overlay.drawn = Some(areas);
    } else {
        let mut record = format!("input arg=method source=file file={}", value(&overlay.path));
        if !overlay.layer.is_empty() {
            record += &format!(" layer={}", value(&overlay.layer));
        }
        if !overlay.r#where.is_empty() {
            record += &format!(" where={}", value(&overlay.r#where));
        }
        records.push(record);
    }
    // The subject's tables, one run each; the area operations take areas.
    let mut subjects = FeatureSet::default();
    let mut left_out = 0;
    for table in subject.dataset.set.tables {
        if areas_only(gdal_operation) && table.kind != GeometryKind::Polygon {
            left_out += table.features.len();
            continue;
        }
        subjects.tables.push(table);
    }
    if left_out != 0 {
        records.push(warning_record(&format!(
            "{left_out} points and lines are left out: {asked} is of areas"
        )));
    }
    took_nothing = took_nothing || subjects.tables.is_empty();
    let settings = format!("overlay operation={asked}");
    if took_nothing {
        records.insert(0, not_run_record("overlay", ""));
        records.push(format!("{settings} features=0"));
        return Ok(answered("GIS OVERLAY", joined(&records)));
    }
    if words.has("PREVIEW") {
        records.insert(0, String::from("gis op=overlay preview=yes"));
        records.push(settings);
        records.push(String::from("preview valid=yes changed=no"));
        return Ok(answered("GIS OVERLAY", joined(&records)));
    }

    let mut gdal_words = vec![format!("--operation={gdal_operation}")];
    gdal_words.extend(input);
    gdal_words.extend(method);
    let target = layer;
    let command_name = String::from(trimmed_text(line));
    let work = make_work(move |stop, progress| {
        let start = Instant::now();
        let mut warnings: Vec<String> = Vec::new();
        let methods = method_set(&overlay, stop, progress, &mut warnings)?;
        let mut pieces: Vec<FeatureTable> = Vec::new();
        if methods
            .tables
            .first()
            .is_some_and(|first| !first.features.is_empty())
        {
            let span = 0.8 / subjects.tables.len() as f64;
            for (done, table) in subjects.tables.iter().enumerate() {
                let one = FeatureSet {
                    tables: vec![table.clone()],
                };
                let out = run_vector(
                    &["vector", "layer-algebra"].map(String::from),
                    &one,
                    &gdal_words,
                    stop,
                    progress_span(
                        progress,
                        0.2 + span * done as f64,
                        0.2 + span * (done + 1) as f64,
                    )
                    .as_deref(),
                    &mut warnings,
                    &[(String::from("method"), methods.clone())],
                )?;
                for result in &out.tables {
                    pieces.push(renamed(result));
                }
            }
        } else {
            warnings.push(String::from(
                "nothing to overlay: the file's layer holds no feature",
            ));
        }
        let tables: Vec<&FeatureTable> = pieces.iter().collect();
        let mut result = FeatureSet::default();
        if !tables.is_empty() {
            result
                .tables
                .push(merged_table(&tables, String::from("overlay")));
        }
        let result = without_empty(result);
        let measures = measure(&result);
        let rows: Vec<Row> = match result.tables.first() {
            Some(first) => rows_of(
                first,
                &RowOptions {
                    entity_keys: true,
                    measures: true,
                },
            ),
            None => Vec::new(),
        };
        let summary = format!(
            "{settings} features={} area={} length={}{}",
            measures.features,
            fixed3(measures.area),
            fixed3(measures.length),
            if csv.is_empty() {
                String::new()
            } else {
                format!(" csv={}", value(&csv))
            }
        );
        let seconds = start.elapsed().as_secs_f64();
        Ok(make_apply(move |context| {
            if !csv.is_empty() {
                write_csv(&csv, &rows, &[])?;
            }
            let options = ResultOptions {
                target_layer: target.clone(),
                operation: String::from("vector layer-algebra"),
                command_name,
                ..ResultOptions::default()
            };
            let mut made = result_command(context.document.model(), &result, &options)?;
            let applied = Applied {
                created: made.created,
                skipped: made.skipped,
                ..Applied::default()
            };
            execute_step(context, made.command.take())?;
            let mut reply = vec![gis_record("overlay", seconds)];
            reply.extend(records);
            reply.push(output_record(&target, &applied));
            for row in &rows {
                reply.push(row_record(row));
            }
            reply.push(summary);
            for warning in &made.warnings {
                reply.push(warning_record(warning));
            }
            for warning in &warnings {
                reply.push(warning_record(warning));
            }
            Ok(joined(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from("GIS OVERLAY"),
        reply: None,
        work: Some(work),
    })
}

/// GIS OVERLAY, its row in the verb table's V2 block (verb_table.rs).
pub fn prepare_gis_overlay(
    context: &mut Context<'_>,
    tokens: &Tokens,
    line: &str,
) -> Result<Prepared> {
    prepare_overlay(context, tokens, line)
}
