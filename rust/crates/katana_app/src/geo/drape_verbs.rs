//! The RASTER SAMPLE and DRAPE verbs (docs/terrain.md, "Sampling and drape"),
//! the port of src/katana_app/geo/drape_verbs.cpp: the height of the ground at
//! points, reported, and given to the drawing's points and vertices in ONE
//! undo step - the same on katana_cli, katana_mcp and the window's command
//! line, where Terrain > Analysis > Drape and Sample Heights builds these
//! lines.
//!
//! ```text
//! RASTER SAMPLE SURFACE <name> | RASTER <id|name> | FILE <path> [AT x,y]...
//!               [<scope>] [method=bilinear|nearest|cubic|cubicspline] [PREVIEW]
//! DRAPE SURFACE <name> | RASTER <id|name> | FILE <path> [<scope>]
//!       [method=bilinear|nearest|cubic|cubicspline] [PREVIEW]
//! ```
//!
//! The ground is a surface read on its own triangles - exactly, never through
//! a grid of it - or a raster's file, which GDAL interpolates
//! (GDALRasterInterpolateAtPoint; bilinear unless method= says). A point off
//! the ground, or beside a no-data cell, has no height: a sample says
//! ground=no, and a drape leaves the vertex heightless and counts it. Absent is
//! not zero.
//!
//! RASTER SAMPLE reads the points given AT, and the points the scope takes -
//! the selection when no scope word and no AT is given. It changes nothing.
//! DRAPE sets the heights of the points, lines and polylines the scope takes
//! through cad's drape_command (entity's set_heights), after comparing them
//! with the copies taken when the line was prepared: an entity edited while
//! the job ran is not written over.

use std::collections::BTreeMap;
use std::time::Instant;

use katana_cad::geo::drape::{drape_command, drape_heights, height_vertices};
use katana_cad::scope_verbs::{
    ScopeMatch, ScopeWords, match_scope, scope_record as cad_scope_record,
};
use katana_core::error::make_error;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::{Entity, EntityId, EnumName, Geometry};
use katana_geometry::Point2;
use katana_gis::Resampling;
use katana_gis::raster_sampling::{resampling_named, to_string as resampling_text};

use super::analysis_support::{
    Ground, bind_ground, open_ground, point_of, point_text, take_keyword_values,
};
use super::bindings::{Tokens, unchanged_since};
use super::geo_verbs::{Context, Prepared, make_apply, make_work, stop_requested, trimmed_text};
use super::replies::warning_record;
use super::vector_support::{
    Applied, VerbWords, WordRules, answered, execute_step, gis_record, joined, not_run_record,
    output_record, read_verb_words,
};

const SAMPLE_USAGE: &str = "RASTER SAMPLE SURFACE <name> | RASTER <id|name> | FILE <path> [AT \
                            x,y]... [<scope>] [method=bilinear|nearest|cubic|cubicspline] \
                            [PREVIEW]";
const DRAPE_USAGE: &str = "DRAPE SURFACE <name> | RASTER <id|name> | FILE <path> [<scope>] \
                           [method=bilinear|nearest|cubic|cubicspline] [PREVIEW]";

fn cancelled() -> Error {
    Error::new(ErrorCode::InvalidState, "cancelled")
}

/// method=, which only a raster has: a surface is linear on its triangles.
fn method_of(words: &VerbWords, ground: &Ground) -> Result<Resampling> {
    let Some(method) = words.option("method") else {
        return Ok(Resampling::Bilinear);
    };
    if ground.surface.is_some() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a surface is read on its own triangles; method= is for a raster",
            format!("method={method}"),
        ));
    }
    resampling_named(method)
}

fn method_text(ground: &Ground, method: Resampling) -> &'static str {
    if ground.surface.is_some() {
        "triangles"
    } else {
        resampling_text(method)
    }
}

/// The matched entities, as the scope parser and matcher give them.
fn match_of(context: &Context<'_>, scope: &ScopeWords) -> Result<ScopeMatch> {
    match_scope(context.document, scope, context.interpreter.scope_context())
}

/// An entity's type as the counts name it: the C++ toString, lowered.
fn type_word(entity: &Entity) -> String {
    entity.r#type().name().to_ascii_lowercase()
}

fn words_rules(usage: &str) -> WordRules {
    WordRules {
        options: vec![String::from("method")],
        flags: vec![String::from("PREVIEW")],
        target: false,
        usage: String::from(usage),
        ..WordRules::default()
    }
}

// ---- RASTER SAMPLE ------------------------------------------------------------------------------

/// The verb, its row in the verb table's T4 block (verb_table.rs).
pub fn prepare_raster_sample(
    context: &mut Context<'_>,
    tokens: &Tokens,
    _line: &str,
) -> Result<Prepared> {
    let mut at = 2;
    let ground = bind_ground(context, tokens, &mut at, "RASTER SAMPLE")?;
    let split = take_keyword_values(tokens, at, &["AT"])?;
    let words = read_verb_words(
        &split.rest,
        0,
        split.rest.size(),
        &words_rules(SAMPLE_USAGE),
    )?;
    let method = method_of(&words, &ground)?;
    let mut points: Vec<Point2> = Vec::new();
    for (_, text) in &split.taken {
        let Some(point) = point_of(text) else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "AT is a point: AT 12.5,40",
                text.clone(),
            ));
        };
        points.push(point);
    }
    let mut records = vec![ground.record.clone()];
    // The points the scope takes: the selection's when neither a scope nor an
    // AT is given, as every verb's missing scope is.
    let mut entities: Vec<(EntityId, Point2)> = Vec::new();
    if words.scope_given || points.is_empty() {
        let found = match_of(context, &words.scope)?;
        let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
        for &id in &found.matched {
            let Some(entity) = context.document.model().entities.find(id) else {
                continue;
            };
            if let Geometry::Point(point) = &entity.geometry {
                entities.push((id, point.position));
            } else {
                *skipped.entry(type_word(entity)).or_default() += 1;
            }
        }
        let mut record = format!(
            "scope arg=input {} points={}",
            cad_scope_record(&found),
            entities.len()
        );
        for (kind, count) in &skipped {
            record += &format!(" skipped.{kind}={count}");
        }
        records.push(record);
    }
    let settings = format!("samples method={}", method_text(&ground, method));
    if points.is_empty() && entities.is_empty() {
        records.insert(0, not_run_record("sample", "points=0"));
        records.push(format!("{settings} count=0"));
        return Ok(answered("RASTER SAMPLE", joined(&records)));
    }
    if words.has("PREVIEW") {
        records.insert(0, String::from("gis op=sample preview=yes"));
        records.push(format!(
            "{settings} count={}",
            points.len() + entities.len()
        ));
        records.push(String::from("preview valid=yes changed=no"));
        return Ok(answered("RASTER SAMPLE", joined(&records)));
    }

    let work = make_work(move |stop, _progress| {
        let start = Instant::now();
        let open = open_ground(&ground, method)?;
        let mut reply: Vec<String> = Vec::new();
        let mut on = 0;
        let mut sampled = |head: String, place: Point2| {
            let z = (open.at)(place);
            on += usize::from(z.is_some());
            let height = match z {
                Some(z) => format!(" z={}", format_exact_real(z)),
                None => String::from(" ground=no"),
            };
            reply.push(format!("{head}at={}{height}", point_text(&place)));
        };
        for &point in &points {
            sampled(String::from("sample "), point);
        }
        for &(id, point) in &entities {
            if stop_requested(stop) {
                return Err(cancelled());
            }
            sampled(format!("sample entity={id} "), point);
        }
        let count = points.len() + entities.len();
        let summary = format!("{settings} count={count} on={on} off={}", count - on);
        let seconds = start.elapsed().as_secs_f64();
        Ok(make_apply(move |_context| {
            let mut out = vec![gis_record("sample", seconds)];
            out.extend(records);
            out.extend(reply);
            out.push(summary);
            if on != count {
                out.push(warning_record(&format!(
                    "{} points lie off the ground and have no height",
                    count - on
                )));
            }
            Ok(joined(&out))
        }))
    });
    Ok(Prepared {
        title: String::from("Sample Heights"),
        reply: None,
        work: Some(work),
    })
}

// ---- DRAPE --------------------------------------------------------------------------------------

/// The verb, its row in the verb table's T4 block (verb_table.rs).
pub fn prepare_drape(context: &mut Context<'_>, tokens: &Tokens, line: &str) -> Result<Prepared> {
    let mut at = 1;
    let ground = bind_ground(context, tokens, &mut at, "DRAPE")?;
    let words = read_verb_words(tokens, at, tokens.size(), &words_rules(DRAPE_USAGE))?;
    let method = method_of(&words, &ground)?;
    let found = match_of(context, &words.scope)?;
    // What has vertices a height belongs to; the rest is counted and left.
    let mut targets: Vec<Entity> = Vec::new();
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
    let mut vertices = 0;
    for &id in &found.matched {
        let Some(entity) = context.document.model().entities.find(id) else {
            continue;
        };
        let count = height_vertices(entity).len();
        if count == 0 {
            *skipped.entry(type_word(entity)).or_default() += 1;
            continue;
        }
        vertices += count;
        targets.push(entity.clone());
    }
    let mut skipped_text = String::new();
    for (kind, count) in &skipped {
        skipped_text += &format!(" skipped.{kind}={count}");
    }
    let mut records = vec![
        ground.record.clone(),
        format!(
            "scope arg=input {} used={}{skipped_text}",
            cad_scope_record(&found),
            targets.len()
        ),
    ];
    let settings = format!("drape method={}", method_text(&ground, method));
    if targets.is_empty() {
        records.insert(0, not_run_record("drape", "entities=0"));
        records.push(format!("{settings} entities=0 vertices=0"));
        return Ok(answered("DRAPE", joined(&records)));
    }
    if words.has("PREVIEW") {
        records.insert(0, String::from("gis op=drape preview=yes"));
        records.push(format!(
            "{settings} entities={} vertices={vertices}",
            targets.len()
        ));
        records.push(String::from("preview valid=yes changed=no"));
        return Ok(answered("DRAPE", joined(&records)));
    }

    let copies = targets;
    let command_name = String::from(trimmed_text(line));
    let work = make_work(move |stop, _progress| {
        let start = Instant::now();
        let open = open_ground(&ground, method)?;
        let heights = drape_heights(&copies, |place| (open.at)(place), stop);
        if heights.stopped || stop_requested(stop) {
            return Err(cancelled());
        }
        let seconds = start.elapsed().as_secs_f64();
        Ok(make_apply(move |context| {
            // In place: the entities as they were when the line was read.
            unchanged_since(context, &copies)?;
            let made = drape_command(context.document.model(), &heights.entities, &command_name);
            let applied = Applied {
                updated: made.changed,
                skipped: made.missing,
                ..Applied::default()
            };
            let unchanged = made.unchanged;
            execute_step(context, made.command)?;
            let mut reply = vec![gis_record("drape", seconds)];
            reply.extend(records);
            reply.push(output_record("", &applied));
            reply.push(format!(
                "{settings} entities={} vertices={} off={} entities_off={} unchanged={unchanged}",
                heights.entities.len(),
                heights.vertices,
                heights.vertices_off,
                heights.entities_off
            ));
            if heights.vertices_off != 0 {
                reply.push(warning_record(&format!(
                    "{} vertices lie off the ground and were left without a height",
                    heights.vertices_off
                )));
            }
            Ok(joined(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from("Drape"),
        reply: None,
        work: Some(work),
    })
}
