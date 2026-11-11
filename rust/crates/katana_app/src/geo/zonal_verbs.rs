//! The RASTER ZONAL verb (docs/terrain.md, "Statistics by area"), the port of
//! src/katana_app/geo/zonal_verbs.cpp: the statistics of a raster inside each
//! closed shape a scope takes, written on the shapes themselves as properties
//! in ONE undo step - the same on katana_cli, katana_mcp and the window's
//! command line, where Terrain > Analysis > Statistics by Area builds these
//! lines.
//!
//! ```text
//! RASTER ZONAL SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path>
//!              [<scope>] [stats=mean,min,max,count,sum] [prefix=zone]
//!              [pixels=fractional|centre|all-touched] [csv=<file>] [OVERWRITE]
//!              [PREVIEW]
//! ```
//!
//! GDAL computes them (`raster zonal-stats`, exactextract's method): the
//! zones are the scope's closed shapes - closed polylines and circles, a
//! tagged hole joined to its area - handed over as one table carrying
//! katana_id, which is how each result finds its entity again. Open lines and
//! points bound no area: GDAL logs "Non-polygonal geometry" for one and the
//! other zones then came back with zero counts (the investigators' finding),
//! so they are left out before GDAL sees them, and counted.
//!
//! With pixels=fractional (the default) each cell counts by the part of it the
//! shape covers, so `count` is an area in cells: a 40 x 30 m box on 1.5 m
//! cells counts 1200 / 2.25 = 533.333 wherever it lies. centre takes a cell
//! whose centre is inside, all-touched every cell the shape touches.
//!
//! The results become <prefix>_<stat> properties of each zone (zone_mean,
//! zone_count ...) through the one result writer (ResultMode::SetProperties),
//! after the zones are compared with the copies taken when the line was
//! prepared: a zone edited while the job ran is not written over. A statistic
//! GDAL has no number for (the mean of a zone off the raster) is no property,
//! never 0.

use std::time::Instant;

use katana_commands::{CommandPtr, Transaction, remove_entity_property};
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};
use katana_gis::processing::{
    ArgValue, DatasetValue, Feature, FeatureSet, FeatureTable, FieldDef, FieldType, FieldValue,
    OutputTo, RunRequest, Scalar, run,
};
use katana_interop::geo::drawing_dataset::result_command;
use katana_interop::geo::{ResultMode, ResultOptions};

use super::bindings::{Tokens, scope_record, unchanged_since};
use super::geo_verbs::{Context, Prepared, make_apply, make_work, trimmed_text};
use super::replies::{value, warning_record};
use super::terrain_verbs::bind_terrain_source;
use super::vector_support::{
    Applied, Row, WordRules, answered, bind_scope, cell_text, choice_option, copies_of,
    execute_step, feature_id, field_index, gis_record, joined, list_of, not_run_record,
    output_record, property_of, read_verb_words, table_named, write_csv,
};
use crate::import_records::path_from_text;

const USAGE: &str = "RASTER ZONAL SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path> \
                     [<scope>] [stats=mean,min,max,count,sum] [prefix=zone] \
                     [pixels=fractional|centre|all-touched] [csv=<file>] [OVERWRITE] [PREVIEW]";

/// The statistics of GDAL's that are one number per zone. The rest of its
/// choices are lists (values, frac, unique, coverage) or need a weighting
/// raster (weighted_*), and have no single property to become.
const STATS: [&str; 17] = [
    "count",
    "sum",
    "mean",
    "min",
    "max",
    "median",
    "stdev",
    "variance",
    "mode",
    "minority",
    "variety",
    "center_x",
    "center_y",
    "min_center_x",
    "min_center_y",
    "max_center_x",
    "max_center_y",
];

const DEFAULT_STATS: [&str; 5] = ["mean", "min", "max", "count", "sum"];

fn refusal(why: &str, word: &str) -> Error {
    make_error(ErrorCode::InvalidArgument, why, word)
}

fn stat_list(text: Option<&String>) -> Result<Vec<String>> {
    let Some(text) = text else {
        return Ok(DEFAULT_STATS.map(String::from).to_vec());
    };
    let mut stats: Vec<String> = Vec::new();
    for item in list_of(text) {
        let stat = item.to_ascii_lowercase();
        if !STATS.contains(&stat.as_str()) {
            return Err(refusal(
                &format!("stats are among {}", STATS.join(",")),
                &item,
            ));
        }
        if stats.contains(&stat) {
            return Err(refusal("a statistic is named twice", &item));
        }
        stats.push(stat);
    }
    if stats.is_empty() {
        return Err(refusal(
            "stats= names at least one statistic: stats=mean,count",
            text,
        ));
    }
    Ok(stats)
}

/// A property key's first part: letters, digits, '_', '.' and '-', so that
/// zone_mean is a key a WHERE condition and a label can name. The C++'s
/// std::isalnum in the C locale: ASCII letters and digits alone.
fn valid_prefix(prefix: &str) -> bool {
    !prefix.is_empty()
        && prefix
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'.' || c == b'-')
}

/// What the work hands the apply.
#[derive(Default)]
struct Zonal {
    /// One table: katana_id, then <prefix>_<stat>.
    properties: FeatureSet,
    rows: Vec<Row>,
    warnings: Vec<String>,
    seconds: f64,
}

/// GDAL's result table as the properties table and the rows: a field per
/// statistic asked for, renamed <prefix>_<stat>; a non-finite number (the mean
/// of nothing) a null, so it becomes no property.
fn zonal_of(result: &FeatureSet, stats: &[String], prefix: &str) -> Result<Zonal> {
    let mut zonal = Zonal::default();
    let mut table = FeatureTable {
        name: String::from("zones"),
        fields: vec![FieldDef {
            name: String::from("katana_id"),
            r#type: FieldType::Integer64,
        }],
        ..FeatureTable::default()
    };
    let Some(from) = result.tables.first() else {
        zonal.properties.tables.push(table);
        return Ok(zonal);
    };
    let Some(id) = field_index(from, "katana_id") else {
        return Err(Error::new(
            ErrorCode::CommandRejected,
            "raster zonal-stats gave no katana_id to find the zones by",
        ));
    };
    let mut columns: Vec<Option<usize>> = Vec::new();
    for stat in stats {
        let column = field_index(from, stat);
        columns.push(column);
        match column {
            None => zonal
                .warnings
                .push(format!("raster zonal-stats gave no {stat} field")),
            Some(column) => table.fields.push(FieldDef {
                name: format!("{prefix}_{stat}"),
                r#type: from.fields[column].r#type,
            }),
        }
    }
    for feature in &from.features {
        let mut made = Feature::default();
        let entity = feature.values.get(id).cloned().unwrap_or(FieldValue::Null);
        let mut row = Row::default();
        row.cells.push((String::from("entity"), cell_text(&entity)));
        made.values.push(entity);
        for (stat, column) in stats.iter().zip(&columns) {
            let Some(column) = *column else {
                continue;
            };
            let mut held = feature
                .values
                .get(column)
                .cloned()
                .unwrap_or(FieldValue::Null);
            if let FieldValue::Real(number) = held
                && !number.is_finite()
            {
                held = FieldValue::Null;
            }
            if held != FieldValue::Null {
                row.cells.push((stat.clone(), cell_text(&held)));
            }
            made.values.push(held);
        }
        table.features.push(made);
        zonal.rows.push(row);
    }
    zonal.properties.tables.push(table);
    Ok(zonal)
}

/// The verb, its row in the verb table's T4 block (verb_table.rs).
pub fn prepare_raster_zonal(
    context: &mut Context<'_>,
    tokens: &Tokens,
    line: &str,
) -> Result<Prepared> {
    let mut at = 2;
    let source = bind_terrain_source(context, tokens, &mut at, "RASTER ZONAL")?;
    let rules = WordRules {
        options: ["stats", "prefix", "pixels", "csv"]
            .map(String::from)
            .to_vec(),
        flags: ["OVERWRITE", "PREVIEW"].map(String::from).to_vec(),
        target: false,
        usage: String::from(USAGE),
        ..WordRules::default()
    };
    let words = read_verb_words(tokens, at, tokens.size(), &rules)?;
    let stats = stat_list(words.option("stats"))?;
    let prefix = words
        .option("prefix")
        .cloned()
        .unwrap_or_else(|| String::from("zone"));
    if !valid_prefix(&prefix) {
        return Err(refusal(
            "prefix is letters, digits, '_', '.' and '-': prefix=zone",
            &prefix,
        ));
    }
    let pixels = choice_option(
        &words,
        "pixels",
        &["fractional", "centre", "all-touched"],
        "fractional",
    )?;
    // GDAL's own word for a cell whose centre is inside.
    let gdal_pixels = if pixels == "centre" {
        String::from("default")
    } else {
        pixels.clone()
    };
    let mut csv = String::new();
    if let Some(file) = words.option("csv") {
        csv.clone_from(file);
        if path_from_text(&csv).exists() && !words.has("OVERWRITE") {
            return Err(make_error(
                ErrorCode::AlreadyExists,
                "the file exists; add OVERWRITE to replace it",
                csv,
            ));
        }
    }

    let bound = bind_scope(context, &words.scope)?;
    let mut records = vec![source.record.clone(), scope_record("zones", &bound)];
    let taken = &bound.dataset.stats;
    let mut zones = FeatureSet::default();
    let mut ids = Vec::new();
    if let Some(polygons) = table_named(&bound.dataset.set, "polygons") {
        // katana_id alone: GDAL copies the fields it is asked to include, and
        // the zones' other properties are theirs already.
        let mut table = FeatureTable {
            name: polygons.name.clone(),
            kind: polygons.kind,
            has_z: false,
            crs_wkt: polygons.crs_wkt.clone(),
            fields: vec![FieldDef {
                name: String::from("katana_id"),
                r#type: FieldType::Integer64,
            }],
            features: Vec::new(),
        };
        for feature in &polygons.features {
            let Some(id) = feature_id(polygons, feature, "katana_id") else {
                continue;
            };
            let mut zone = Feature {
                parts: feature.parts.clone(),
                values: Vec::new(),
            };
            for part in &mut zone.parts {
                part.has_z = false;
            }
            zone.values.push(FieldValue::Integer64(id as i64));
            table.features.push(zone);
            ids.push(id);
        }
        if !table.features.is_empty() {
            zones.tables.push(table);
        }
    }
    records.push(format!(
        "zones used={} skipped.open={} skipped.points={}",
        ids.len(),
        taken.lines,
        taken.points
    ));
    if taken.lines + taken.points != 0 {
        records.push(warning_record(&format!(
            "{} of what the scope took are not closed shapes, so bound no zone, and were left out",
            taken.lines + taken.points
        )));
    }
    for warning in &taken.warnings {
        records.push(warning_record(warning));
    }
    let settings = format!(
        "zonal stats={} prefix={} pixels={pixels}",
        stats.join(","),
        value(&prefix)
    );
    if ids.is_empty() {
        records.insert(0, not_run_record("zonal", "zones=0"));
        records.push(settings);
        return Ok(answered("RASTER ZONAL", joined(&records)));
    }
    if words.has("PREVIEW") {
        records.insert(0, String::from("gis op=zonal preview=yes"));
        records.push(settings);
        records.push(String::from("preview valid=yes changed=no"));
        return Ok(answered("RASTER ZONAL", joined(&records)));
    }

    let dataset = source.dataset;
    let copies = copies_of(context.document, &ids);
    let command_name = String::from(trimmed_text(line));
    let work = make_work(move |stop, progress| {
        let start = Instant::now();
        let input = dataset()?;
        let request = RunRequest {
            path: vec![String::from("raster"), String::from("zonal-stats")],
            values: vec![
                (String::from("input"), ArgValue::Dataset(input)),
                (
                    String::from("zones"),
                    ArgValue::Dataset(DatasetValue::Features(zones)),
                ),
                (
                    String::from("stat"),
                    ArgValue::Scalar(Scalar::TextList(stats.clone())),
                ),
                (
                    String::from("include-field"),
                    ArgValue::Scalar(Scalar::TextList(vec![String::from("katana_id")])),
                ),
                (
                    String::from("pixels"),
                    ArgValue::Scalar(Scalar::Text(gdal_pixels)),
                ),
            ],
            output_to: OutputTo::Memory,
            ..RunRequest::default()
        };
        let outputs = run(&request, stop, progress)?;
        let none = FeatureSet::default();
        let mut zonal = zonal_of(outputs.features.as_ref().unwrap_or(&none), &stats, &prefix)?;
        for warning in &outputs.diagnostics {
            zonal.warnings.push(warning.message.clone());
        }
        zonal.seconds = start.elapsed().as_secs_f64();
        Ok(make_apply(move |context| {
            // In place: the zones as they were when the line was read.
            unchanged_since(context, &copies)?;
            let options = ResultOptions {
                mode: ResultMode::SetProperties,
                command_name: command_name.clone(),
                ..ResultOptions::default()
            };
            let mut plan = result_command(context.document.model(), &zonal.properties, &options)?;
            // A statistic with no number this time takes away the number an
            // earlier run left, in the same step: a zone moved off the raster
            // must not keep its old mean.
            let mut step = Transaction::new(command_name.clone());
            if let Some(command) = plan.command.take() {
                step.add(command);
            }
            let mut changed = 0;
            for table in &zonal.properties.tables {
                for feature in &table.features {
                    let Some(id) = feature_id(table, feature, "katana_id") else {
                        continue;
                    };
                    let Some(entity) = context.document.model().entities.find(id) else {
                        continue;
                    };
                    // Whether this zone changes at all, for the output record.
                    let mut any = false;
                    for (field, held) in table.fields.iter().zip(&feature.values).skip(1) {
                        let key = &field.name;
                        let current = entity.properties.get(key);
                        if *held == FieldValue::Null {
                            if current.is_some() {
                                step.add(remove_entity_property(vec![entity.id], key.clone()));
                                any = true;
                            }
                        } else if let Some(now) = property_of(held) {
                            any = any || current.is_none_or(|was| *was != now);
                        }
                    }
                    changed += usize::from(any);
                }
            }
            let applied = Applied {
                updated: changed,
                skipped: plan.skipped,
                ..Applied::default()
            };
            let command: Option<CommandPtr> = if step.size() != 0 {
                Some(Box::new(step))
            } else {
                None
            };
            // The file before the drawing, as GIS OVERLAY and GIS SQL write
            // theirs: a CSV that cannot be written is a failed reply, and a
            // failed reply must leave the drawing as it was.
            if !csv.is_empty() {
                write_csv(&csv, &zonal.rows, &[])?;
            }
            execute_step(context, command)?;
            let mut reply = vec![gis_record("zonal", zonal.seconds)];
            reply.extend(records);
            reply.push(output_record("", &applied));
            for row in &zonal.rows {
                let mut record = String::from("zone");
                for (key, text) in &row.cells {
                    record += &format!(" {key}={}", value(text));
                }
                reply.push(record);
            }
            reply.push(format!(
                "{settings} zones={}{}",
                zonal.rows.len(),
                if csv.is_empty() {
                    String::new()
                } else {
                    format!(" csv={}", value(&csv))
                }
            ));
            for warning in &plan.warnings {
                reply.push(warning_record(warning));
            }
            for warning in &zonal.warnings {
                reply.push(warning_record(warning));
            }
            Ok(joined(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from("Statistics by Area"),
        reply: None,
        work: Some(work),
    })
}
