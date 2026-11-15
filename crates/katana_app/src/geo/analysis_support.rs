//! What the terrain analysis verbs of T4 and T5 share (docs/terrain.md,
//! "Sampling and drape", "Viewshed and line of sight"), the port of
//! src/katana_app/geo/analysis_support.hpp/.cpp (the C++'s namespace
//! `geo::analysis`): RASTER SAMPLE, DRAPE, RASTER VIEWSHED and LOS read
//! heights from the ground, and all of them write points as x,y and take
//! keyword-and-value words (AT x,y, OBSERVER x,y, NAME <n>) out of a line
//! before the rest goes to the ONE scope parser through vector_support's
//! read_verb_words.

use std::sync::{Arc, Mutex, PoisonError};

use katana_cad::geo::drape::HeightAt;
use katana_core::error::make_error;
use katana_core::text::{format_exact_real, parse_finite_double};
use katana_core::{ErrorCode, Result};
use katana_geometry::Point2;
use katana_gis::gdal_adapter::{describe_crs, same_crs};
use katana_gis::processing::DatasetValue;
use katana_gis::{GdalDataset, RasterSampler, Resampling};
use katana_terrain::TinSurface;

use super::bindings::{
    Source, SourceKind, Tokens, bind_raster, input_record, parse_source, project_crs,
};
use super::geo_verbs::{Context, trimmed_text};
use super::replies::value;
use crate::import_records::path_from_text;

/// The ground a verb reads heights from: a surface on its own triangles -
/// exactly, never through a grid of it - or a raster's file, sampled by GDAL
/// at full precision. Copied at prepare; opened on the worker.
#[derive(Clone, Debug, Default)]
pub struct Ground {
    pub source: Source,
    /// SURFACE.
    pub surface: Option<Arc<TinSurface>>,
    /// RASTER, FILE: the file.
    pub path: String,
    /// `input arg=input source=surface name=ground read=triangles`, or the
    /// shared input record of a raster or file.
    pub record: String,
    /// The project's coordinate system when the line was prepared: a raster
    /// known to be in another is refused when it is opened, since the
    /// drawing's points would be read at the wrong place in it.
    pub project_crs: String,
}

/// SURFACE <name> | RASTER <id|name> | FILE <path> at `at`, leaving `at`
/// after it. InvalidArgument naming the word for anything else (a scope of
/// the drawing comes after the ground) and for CELL, which only a grid has;
/// NotFound for a surface or raster of no such name; Unsupported for a
/// reference raster with no file behind it.
pub fn bind_ground(
    context: &Context<'_>,
    tokens: &Tokens,
    at: &mut usize,
    verb: &str,
) -> Result<Ground> {
    if *at >= tokens.size()
        || (!tokens.is(*at, "RASTER") && !tokens.is(*at, "SURFACE") && !tokens.is(*at, "FILE"))
    {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "{verb} reads heights from a SURFACE <name>, a RASTER <id|name> or a FILE \
                 <path>; a scope of the drawing comes after it"
            ),
            if *at < tokens.size() {
                tokens[*at].clone()
            } else {
                String::new()
            },
        ));
    }
    let source = parse_source(tokens, at)?;
    if source.kind == SourceKind::Surface {
        if source.cell.is_some() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a surface is read on its own triangles; CELL is for a grid of it",
                "CELL",
            ));
        }
        let Some(named) = context.surfaces.find(&source.surface) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "no surface has that name; SURFACE LIST names them",
                source.surface.clone(),
            ));
        };
        let record = format!(
            "input arg=input source=surface name={} read=triangles",
            value(&named.name)
        );
        let surface = named.surface.clone();
        return Ok(Ground {
            source,
            surface,
            record,
            ..Ground::default()
        });
    }
    // A raster's or a file's path, which the bindings read from the reference
    // raster (its source file, never its display copy): nothing is opened
    // here.
    let deferred = bind_raster(context, &source)?;
    let DatasetValue::Path(file) = deferred()? else {
        return Err(make_error(
            ErrorCode::Unsupported,
            "the raster has no file to sample",
            source.raster.clone(),
        ));
    };
    let record = input_record("input", &source);
    Ok(Ground {
        source,
        surface: None,
        path: file.path,
        record,
        project_crs: project_crs(context),
    })
}

/// The ground opened on the worker: the height at a plan point, and a
/// natural spacing to walk it at - half a raster cell, or none for a surface,
/// whose caller decides.
#[derive(Clone)]
pub struct OpenGround {
    pub at: HeightAt,
    pub spacing: Option<f64>,
}

/// A surface read on its triangles (TinSurface's elevation_at); a raster
/// through gis's RasterSampler with `method`. What RasterSampler::open
/// refuses, and InvalidCRS for a raster in another coordinate system than the
/// project's (gis's same_crs).
pub fn open_ground(ground: &Ground, method: Resampling) -> Result<OpenGround> {
    if let Some(surface) = &ground.surface {
        let surface = Arc::clone(surface);
        let at: HeightAt = Arc::new(move |at: Point2| surface.elevation_at(at));
        return Ok(OpenGround { at, spacing: None });
    }
    if !ground.project_crs.is_empty() {
        let info = GdalDataset::open(&path_from_text(&ground.path))
            .and_then(|opened| opened.raster_info());
        if let Ok(info) = info
            && same_crs(&info.projection_wkt, &ground.project_crs) == Some(false)
        {
            return Err(make_error(
                ErrorCode::InvalidCrs,
                format!(
                    "the raster is in {} and the drawing in {}: the drawing's points would be \
                     read at the wrong place; RASTER REPROJECT the raster into the project's \
                     first",
                    describe_crs(&info.projection_wkt),
                    describe_crs(&ground.project_crs)
                ),
                ground.path.clone(),
            ));
        }
    }
    let sampler = RasterSampler::open(&path_from_text(&ground.path), 1)?;
    let spacing = Some(sampler.cell_size() / 2.0);
    // Shared, so the height can be copied, and behind a lock because a GDAL
    // dataset is used by one thread at a time (RasterSampler is Send, not
    // Sync); the C++ shares it unlocked and uses it on the one thread.
    let shared = Arc::new(Mutex::new(sampler));
    let at: HeightAt = Arc::new(move |at: Point2| {
        shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .at(at.x, at.y, method)
    });
    Ok(OpenGround { at, spacing })
}

/// "x,y" as a plan point: two finite numbers, locale-independent; None
/// otherwise.
pub fn point_of(text: &str) -> Option<Point2> {
    let comma = text.find(',')?;
    if text[comma + 1..].contains(',') {
        return None;
    }
    let x = parse_finite_double(trimmed_text(&text[..comma]))?;
    let y = parse_finite_double(trimmed_text(&text[comma + 1..]))?;
    Some(Point2::new(x, y))
}

/// A point as a line and a record write it: "12.5,-3".
pub fn point_text(point: &Point2) -> String {
    format!(
        "{},{}",
        format_exact_real(point.x),
        format_exact_real(point.y)
    )
}

/// A line's words from `begin`, with every `<keyword> <value>` of `keywords`
/// (upper case) taken out wherever it stands, in order - an unquoted keyword
/// in any case; a quoted "AT" is a value. What is left, for
/// vector_support's read_verb_words.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeywordValues {
    pub rest: Tokens,
    /// The keyword in upper case, and its value.
    pub taken: Vec<(String, String)>,
}

/// InvalidArgument naming the keyword when no value follows it.
pub fn take_keyword_values(
    tokens: &Tokens,
    begin: usize,
    keywords: &[&str],
) -> Result<KeywordValues> {
    let mut out = KeywordValues::default();
    let mut i = begin;
    while i < tokens.size() {
        let word = if tokens.quoted[i] {
            String::new()
        } else {
            tokens[i].to_ascii_uppercase()
        };
        if !word.is_empty() && keywords.contains(&word.as_str()) {
            if i + 1 >= tokens.size() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!("{word} needs a value after it"),
                    tokens[i].clone(),
                ));
            }
            out.taken.push((word, tokens[i + 1].clone()));
            i += 2;
            continue;
        }
        out.rest.words.push(tokens.words[i].clone());
        out.rest.quoted.push(tokens.quoted[i]);
        i += 1;
    }
    Ok(out)
}
