//! What every geoprocessing verb shares (docs/geoprocessing.md, "Bindings"),
//! the port of src/katana_app/geo/bindings.hpp/.cpp: the ONE reader of a source
//! clause and the ONE reader of a target clause, drawing data bound through
//! the shared scope and filter, rasters and surfaces bound as datasets, and a
//! run's outputs applied to the drawing.
//!
//! ```text
//! <source> := <scope> | RASTER <id|name> | SURFACE <name> [CELL <m>]
//!           | FILE <path> [LAYER <name>]
//! <scope>  := what cad's parse_scope_words reads (scope_verbs.rs): SELECTION |
//!             DRAWING | VIEW [<id>] [EXTENTS] | AREA x0,y0,x1,y1 |
//!             LAYERS a,b [ONLY], then [WHERE key=value ...]
//! <target> := LAYER <path> | REFERENCE [<name>] | FILE <path> [FORMAT <driver>]
//!           | SURFACE <name> | SELECTION | REPORT
//! ```
//!
//! A curated verb (the lanes' SURFACE, CONTOUR, GIS BUFFER ...) is a thin
//! module over these: it reads its own words, builds RunRequests and chooses a
//! ResultMode; it never reads a scope or applies a result another way.
//!
//! Reserved blocks, one per package that adds to this file, so the lanes that
//! build on it edit only their own: T0 (TO SURFACE: a raster result kept as a
//! named surface) and V5 (TO SELECTION and TO REPORT: a query's rows selected
//! or reported).

use std::ops::Index;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use katana_cad::Document;
use katana_cad::scope_verbs::{
    ScopeMatch, ScopeWords, is_scope_word, match_scope, parse_scope_words,
    scope_record as cad_scope_record,
};
use katana_core::error::make_error;
use katana_core::text::{
    equals_ignoring_case, format_exact_real, is_ascii_space, parse_finite_double,
};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::entity_geometry::bounding_box;
use katana_entity::{Entity, EntityId};
use katana_geometry::Box2;
use katana_gis::gdal_adapter::{describe_crs, same_crs};
use katana_gis::processing::{DatasetPath, DatasetValue, RasterGrid, RunOutputs};
use katana_gis::{GdalDataset, RasterExportOptions};
use katana_interop::RasterOverlay;
use katana_interop::geo::drawing_dataset::{drawing_dataset, result_command};
use katana_interop::geo::raster_products::{derived_overlay, derived_raster_path, surface_grid};
use katana_interop::geo::{DrawingDataset, DrawingDatasetOptions, ResultOptions};
use katana_interop::terrain_io::suggested_cell_size;

use super::geo_verbs::{Context, system_message};
use super::replies::{text_record, value, warning_record, yes_no};
use super::terrain_verbs::{SURFACE_POINT_CAP, keep_surface, surface_from_raster_file};
use super::vector_support::{joined, report_records, select_features}; // V5: TO SELECTION and TO REPORT
use crate::import_records::{path_from_text, path_text};

/// A line split into words as the interpreter splits it (blanks, double
/// quotes grouping and removed), and which words were quoted: a quoted "FROM"
/// is a value, never the keyword.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tokens {
    pub words: Vec<String>,
    pub quoted: Vec<bool>,
}

impl Tokens {
    pub fn size(&self) -> usize {
        self.words.len()
    }

    /// `words[i]` is `keyword`, unquoted, in any case.
    pub fn is(&self, i: usize, keyword: &str) -> bool {
        i < self.words.len() && !self.quoted[i] && self.words[i].eq_ignore_ascii_case(keyword)
    }
}

impl Index<usize> for Tokens {
    type Output = String;

    fn index(&self, i: usize) -> &String {
        &self.words[i]
    }
}

/// ParseFailure for a quote never closed.
pub fn tokenize(line: &str) -> Result<Tokens> {
    // The interpreter's rules (cad's tokenize): blanks separate, double quotes
    // group and are removed, "" is an empty word. Kept here with the one thing
    // it drops - which words were quoted.
    let mut tokens = Tokens::default();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut has_token = false;
    let mut was_quoted = false;
    for ch in line.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
            has_token = true;
            was_quoted = true;
        } else if !in_quotes && ch.is_ascii() && is_ascii_space(ch as u8) {
            if has_token {
                tokens.words.push(std::mem::take(&mut current));
                tokens.quoted.push(was_quoted);
                has_token = false;
                was_quoted = false;
            }
        } else {
            current.push(ch);
            has_token = true;
        }
    }
    if in_quotes {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "unterminated quoted string",
        ));
    }
    if has_token {
        tokens.words.push(current);
        tokens.quoted.push(was_quoted);
    }
    Ok(tokens)
}

/// Which source a FROM gives (the C++'s `Source::Kind`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SourceKind {
    #[default]
    Drawing,
    Raster,
    Surface,
    File,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Source {
    pub kind: SourceKind,
    /// Drawing.
    pub scope: ScopeWords,
    /// Raster: an id or a name.
    pub raster: String,
    /// Surface.
    pub surface: String,
    /// File.
    pub path: String,
    pub layer: String,
    /// Surface: CELL.
    pub cell: Option<f64>,
}

/// Where a TO sends a result (the C++'s `Target::Kind`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TargetKind {
    #[default]
    Default,
    Layer,
    Reference,
    File,
    Surface,
    Selection,
    Report,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Target {
    pub kind: TargetKind,
    /// Layer: the layer; Reference, Surface: the name; File: the path.
    pub name: String,
    /// File: FORMAT.
    pub format: String,
}

fn clause_word(tokens: &Tokens, i: usize) -> bool {
    ["FROM", "TO", "CONFIRM", "OVERWRITE", "PREVIEW"]
        .iter()
        .any(|word| tokens.is(i, word))
}

/// The word at `at`, or nothing past the end: what a refusal names.
fn word_at(tokens: &Tokens, at: usize) -> String {
    if at < tokens.size() {
        tokens[at].clone()
    } else {
        String::new()
    }
}

/// The reference raster a RASTER source names: by its id (digits) first, else
/// by its name in any case.
fn find_raster<'r>(rasters: &'r [RasterOverlay], word: &str) -> Option<&'r RasterOverlay> {
    let digits = !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_digit());
    if digits
        && let Some(raster) = rasters
            .iter()
            .find(|raster| format!("{}", raster.id) == word)
    {
        return Some(raster);
    }
    rasters
        .iter()
        .find(|raster| equals_ignoring_case(raster.name.as_bytes(), word.as_bytes()))
}

/// Whether a reference raster has `name`, in any case.
fn reference_name_taken(rasters: &[RasterOverlay], name: &str) -> bool {
    rasters
        .iter()
        .any(|raster| equals_ignoring_case(raster.name.as_bytes(), name.as_bytes()))
}

/// The words of a clause keyword, for the errors that name what was expected.
pub fn is_source_keyword(tokens: &Tokens, i: usize) -> bool {
    tokens.is(i, "RASTER")
        || tokens.is(i, "SURFACE")
        || tokens.is(i, "FILE")
        || (i < tokens.size() && !tokens.quoted[i] && is_scope_word(&tokens[i]))
}

/// The value after the keyword at `at`, leaving `at` after both; `what` names
/// the keyword's value when there is none.
fn needs(tokens: &Tokens, at: &mut usize, what: &str) -> Result<String> {
    if *at + 1 >= tokens.size() || clause_word(tokens, *at + 1) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            what,
            tokens[*at].clone(),
        ));
    }
    *at += 2;
    Ok(tokens[*at - 1].clone())
}

/// Reads one source from `at`, leaving `at` after it. InvalidArgument (or the
/// scope parser's ParseFailure) naming the word, for a clause that is none.
pub fn parse_source(tokens: &Tokens, at: &mut usize) -> Result<Source> {
    let refuse =
        |why: &str, at: usize| make_error(ErrorCode::InvalidArgument, why, word_at(tokens, at));
    if *at >= tokens.size() || clause_word(tokens, *at) {
        return Err(refuse(
            "FROM needs a source: SELECTION, DRAWING, VIEW, AREA, LAYERS or WHERE for the drawing; \
             RASTER <id|name>; SURFACE <name> [CELL <m>]; FILE <path>",
            *at,
        ));
    }
    let mut source = Source::default();
    if tokens.is(*at, "RASTER") {
        source.raster = needs(tokens, at, "RASTER needs the reference raster's id or name")?;
        source.kind = SourceKind::Raster;
        return Ok(source);
    }
    if tokens.is(*at, "SURFACE") {
        source.surface = needs(tokens, at, "SURFACE needs the surface's name")?;
        source.kind = SourceKind::Surface;
        if tokens.is(*at, "CELL") {
            let cell = needs(tokens, at, "CELL needs a length")?;
            let length = parse_finite_double(&cell);
            match length {
                Some(length) if length > 0.0 => source.cell = Some(length),
                _ => {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        "CELL is a positive length",
                        cell,
                    ));
                }
            }
        }
        return Ok(source);
    }
    if tokens.is(*at, "FILE") {
        source.path = needs(tokens, at, "FILE needs a path")?;
        source.kind = SourceKind::File;
        if tokens.is(*at, "LAYER") {
            source.layer = needs(tokens, at, "LAYER needs the file's layer name")?;
        }
        return Ok(source);
    }
    if tokens.quoted[*at] || !is_scope_word(&tokens[*at]) {
        return Err(refuse(
            "not a source; FROM takes SELECTION, DRAWING, VIEW, AREA, LAYERS or WHERE for the \
             drawing, RASTER, SURFACE or FILE",
            *at,
        ));
    }
    // The ONE scope parser: it stops at the first word that is neither a
    // scope word nor a WHERE condition, which is where the line goes on.
    let start = *at;
    let scope = parse_scope_words(&tokens.words, at)?;
    if *at == start {
        return Err(refuse("not a source", *at));
    }
    source.kind = SourceKind::Drawing;
    source.scope = scope;
    Ok(source)
}

/// The name after a target's keyword at `at`, leaving `at` after both.
fn named(tokens: &Tokens, at: &mut usize, kind: TargetKind, why: &str) -> Result<Target> {
    if *at + 1 >= tokens.size() || clause_word(tokens, *at + 1) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            why,
            tokens[*at].clone(),
        ));
    }
    let target = Target {
        kind,
        name: tokens[*at + 1].clone(),
        format: String::new(),
    };
    *at += 2;
    Ok(target)
}

/// Reads one target from `at`, leaving `at` after it.
pub fn parse_target(tokens: &Tokens, at: &mut usize) -> Result<Target> {
    let refuse = |at: usize| {
        make_error(
            ErrorCode::InvalidArgument,
            "TO takes LAYER <path>, REFERENCE [<name>], FILE <path> [FORMAT <driver>], SURFACE \
             <name>, SELECTION or REPORT",
            word_at(tokens, at),
        )
    };
    if *at >= tokens.size() {
        return Err(refuse(*at));
    }
    if tokens.is(*at, "LAYER") || tokens.is(*at, "LAYERS") {
        return named(
            tokens,
            at,
            TargetKind::Layer,
            "TO LAYER needs the layer's path",
        );
    }
    if tokens.is(*at, "SURFACE") {
        return named(tokens, at, TargetKind::Surface, "TO SURFACE needs a name");
    }
    if tokens.is(*at, "FILE") {
        let mut target = named(tokens, at, TargetKind::File, "TO FILE needs a path")?;
        if tokens.is(*at, "FORMAT") {
            if *at + 1 >= tokens.size() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "FORMAT needs a GDAL driver's name",
                ));
            }
            target.format = tokens[*at + 1].clone();
            *at += 2;
        }
        return Ok(target);
    }
    if tokens.is(*at, "REFERENCE") {
        let mut target = Target {
            kind: TargetKind::Reference,
            ..Target::default()
        };
        *at += 1;
        if *at < tokens.size() && !clause_word(tokens, *at) {
            target.name = tokens[*at].clone();
            *at += 1;
        }
        return Ok(target);
    }
    if tokens.is(*at, "SELECTION") {
        *at += 1;
        return Ok(Target {
            kind: TargetKind::Selection,
            ..Target::default()
        });
    }
    if tokens.is(*at, "REPORT") {
        *at += 1;
        return Ok(Target {
            kind: TargetKind::Report,
            ..Target::default()
        });
    }
    Err(refuse(*at))
}

/// What a scope took, as the feature set an algorithm reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoundDrawing {
    pub r#match: ScopeMatch,
    pub dataset: DrawingDataset,
}

/// The scope resolved and matched by the shared matcher (VIEW through the
/// interpreter's scope context, refused headless naming AREA), then
/// converted. A scope that takes nothing is not a failure: the dataset is
/// empty and the record says matched=0.
pub fn bind_drawing(
    context: &Context<'_>,
    scope: &ScopeWords,
    options: &DrawingDatasetOptions,
) -> Result<BoundDrawing> {
    let found = match_scope(context.document, scope, context.interpreter.scope_context())?;
    let dataset = drawing_dataset(context.document.model(), &found.matched, options)?;
    Ok(BoundDrawing {
        r#match: found,
        dataset,
    })
}

/// A dataset made on the worker, from what was copied at prepare: a surface
/// is sampled there, since sampling is work and the surface is shared and
/// immutable. Copied and called again as the C++'s std::function is (a verb
/// may materialise its inputs for a preview and again for the run), so it is
/// shared, and Sync because a job's thread calls it.
pub type DeferredDataset = Arc<dyn Fn() -> Result<DatasetValue> + Send + Sync>;

/// A raster, surface or file source as the dataset GDAL reads. RASTER binds
/// the reference raster's file, read at full precision - never its display
/// copy; SURFACE the surface sampled at cell centres at CELL, or at a cell
/// suggested for its extent; FILE the path as it is (a /vsi path or a URL
/// passes through). NotFound for a raster or surface of no such id or name;
/// Unsupported for a reference raster with no file behind it.
pub fn bind_raster(context: &Context<'_>, source: &Source) -> Result<DeferredDataset> {
    match source.kind {
        SourceKind::Raster => {
            let Some(raster) = find_raster(context.reference.rasters(), &source.raster) else {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "no reference raster has that id or name; REFS lists them",
                    source.raster.clone(),
                ));
            };
            if raster.source.as_os_str().is_empty() {
                return Err(make_error(
                    ErrorCode::Unsupported,
                    "the reference raster has no file to read at full precision",
                    source.raster.clone(),
                ));
            }
            let path = DatasetPath {
                path: path_text(&raster.source),
                ..DatasetPath::default()
            };
            let deferred: DeferredDataset =
                Arc::new(move || -> Result<DatasetValue> { Ok(DatasetValue::Path(path.clone())) });
            Ok(deferred)
        }
        SourceKind::Surface => {
            let Some(named) = context.surfaces.find(&source.surface) else {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "no surface has that name",
                    source.surface.clone(),
                ));
            };
            let surface = Arc::clone(
                named
                    .surface
                    .as_ref()
                    .expect("a drawing's surface has a triangulation"),
            );
            // The C++ suggests a cell whether or not CELL gave one; it reads
            // nothing else, so asking only when there is none is the same.
            // 1000 cells along the longer side is the C++'s default
            // (terrain_io.hpp, suggestedCellSize).
            let cell = match source.cell {
                Some(cell) => cell,
                None => suggested_cell_size(surface.bounds(), 1000),
            };
            let crs = project_crs(context);
            let deferred: DeferredDataset = Arc::new(move || -> Result<DatasetValue> {
                // At most 25 000 000 cells: the C++'s default
                // (raster_products.hpp, surfaceGrid).
                let mut grid = surface_grid(&surface, cell, 25_000_000)?;
                grid.info.projection_wkt = crs.clone();
                Ok(DatasetValue::Raster(grid))
            });
            Ok(deferred)
        }
        SourceKind::File => {
            let path = DatasetPath {
                path: source.path.clone(),
                layer: source.layer.clone(),
                ..DatasetPath::default()
            };
            let deferred: DeferredDataset =
                Arc::new(move || -> Result<DatasetValue> { Ok(DatasetValue::Path(path.clone())) });
            Ok(deferred)
        }
        SourceKind::Drawing => Err(Error::new(
            ErrorCode::InvalidArgument,
            "the drawing is bound by its scope, not as a raster",
        )),
    }
}

/// The record of one input: `input arg=input source=surface name=ground
/// cell=1` (docs/geoprocessing.md, "Replies").
pub fn input_record(arg: &str, source: &Source) -> String {
    let mut record = format!("input arg={}", value(arg));
    match source.kind {
        SourceKind::Drawing => record += " source=drawing",
        SourceKind::Raster => record += &format!(" source=raster name={}", value(&source.raster)),
        SourceKind::Surface => {
            record += &format!(
                " source=surface name={} cell={}",
                value(&source.surface),
                source
                    .cell
                    .map_or_else(|| String::from("auto"), format_exact_real)
            );
        }
        SourceKind::File => {
            record += &format!(" source=file file={}", value(&source.path));
            if !source.layer.is_empty() {
                record += &format!(" layer={}", value(&source.layer));
            }
        }
    }
    record
}

/// The record of what a scope took, the shared scope record in the middle:
///
/// ```text
/// scope arg=input scope=drawing matched=12 used=10 points=0 lines=4
/// polygons=6 skipped.text=2
/// ```
pub fn scope_record(arg: &str, bound: &BoundDrawing) -> String {
    let stats = &bound.dataset.stats;
    let mut record = format!(
        "scope arg={} {} used={} points={} lines={} polygons={}",
        value(arg),
        cad_scope_record(&bound.r#match),
        stats.used,
        stats.points,
        stats.lines,
        stats.polygons
    );
    for (reason, count) in &stats.skipped {
        record += &format!(" skipped.{reason}={count}");
    }
    record
}

/// The project's coordinate system, for the tables a scope becomes.
pub fn project_crs(context: &Context<'_>) -> String {
    context.document.metadata().coordinate_system.clone()
}

/// When `data`'s coordinate system is known to differ from the project's
/// (gis's same_crs; nothing when either is unknown), the text a warning says:
/// "<what> is in <X>, not the project's <Y>: <consequence>". Coordinates drawn
/// as they come would not register against the rest of the drawing.
pub fn crs_differs(data: &str, project: &str, what: &str, consequence: &str) -> Option<String> {
    if same_crs(data, project) != Some(false) {
        return None;
    }
    Some(format!(
        "{what} is in {}, not the project's {}: {consequence}",
        describe_crs(data),
        describe_crs(project)
    ))
}

/// Where a derived raster goes: the project's cache, or the scratch folder
/// (persisted=no) when the drawing has no project.
pub fn derived_folder(context: &Context<'_>) -> PathBuf {
    let project = context.document.project_directory();
    derived_raster_path(project.as_deref(), context.scratch, "x")
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

/// What an apply needs to know besides the outputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplyRequest {
    pub target: Target,
    pub arg: String,
    /// The layer a vector result goes to when no TO says: gis/<algorithm's
    /// last word>. The reference name a raster result takes likewise.
    pub default_name: String,
    pub result: ResultOptions,
}

impl Default for ApplyRequest {
    fn default() -> Self {
        Self {
            target: Target::default(),
            arg: String::from("output"),
            default_name: String::new(),
            result: ResultOptions::default(),
        }
    }
}

/// A derived raster's file into its place: renamed, or copied and removed
/// when the scratch folder and the project are on different volumes.
fn move_file(from: &Path, to: &Path) -> Status {
    if let Some(parent) = to.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    if let Err(error) = std::fs::copy(from, to) {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            format!(
                "could not keep the derived raster: {}",
                system_message(&error)
            ),
            path_text(to),
        ));
    }
    let _ = std::fs::remove_file(from);
    Ok(())
}

fn bounds_of(document: &Document, ids: &[EntityId]) -> Box2 {
    let mut found = Box2::default();
    for &id in ids {
        if let Some(entity) = document.model().entities.find(id) {
            let one = bounding_box(&entity.geometry);
            if !one.empty() {
                found.expand(one.min);
                found.expand(one.max);
            }
        }
    }
    found
}

/// A raster result to the reference data, under a name no raster has.
fn keep_derived(context: &mut Context<'_>, request: &ApplyRequest, file: &Path) -> Result<String> {
    let base = if request.target.name.is_empty() {
        request.default_name.clone()
    } else {
        request.target.name.clone()
    };
    let project = context.document.project_directory();
    let mut name = base.clone();
    let mut destination;
    let mut copy = 2;
    loop {
        destination = derived_raster_path(project.as_deref(), context.scratch, &name);
        if !reference_name_taken(context.reference.rasters(), &name) && !destination.exists() {
            break;
        }
        name = format!("{base}-{copy}");
        copy += 1;
    }
    move_file(file, &destination)?;
    let overlay = derived_overlay(
        &destination,
        name.clone(),
        request.result.command_name.clone(),
    )?;
    let width = overlay.width;
    let height = overlay.height;
    let crs = overlay.projection_wkt.clone();
    let bounds = overlay.world_bounds();
    let id = context.reference.add(overlay);
    if let Some(changed) = &context.front_end.changed {
        changed();
    }
    if let Some(frame) = &context.front_end.frame
        && !bounds.empty()
    {
        frame(&bounds);
    }
    let mut reply = format!(
        "output arg={} kind=raster target=reference id={id} name={} raster={width}x{height} \
         file={} persisted={}",
        value(&request.arg),
        value(&name),
        value(&path_text(&destination)),
        yes_no(project.is_some())
    );
    if let Some(differs) = crs_differs(
        &crs,
        &project_crs(context),
        "the raster",
        "it is drawn at its own coordinates, where the drawing's are not; RASTER REPROJECT moves \
         it",
    ) {
        reply.push('\n');
        reply += &warning_record(&differs);
    }
    Ok(reply)
}

/// A grid a run kept in memory, written where a derived raster is kept.
fn write_grid(context: &Context<'_>, grid: &RasterGrid) -> Result<PathBuf> {
    if grid.bands.len() != 1 {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "a result of several bands in memory cannot be kept as a reference raster",
        ));
    }
    let options = RasterExportOptions {
        width: grid.info.width,
        height: grid.info.height,
        driver: String::from("GTiff"),
        projection_wkt: grid.info.projection_wkt.clone(),
        geotransform: grid.info.geotransform,
        no_data_value: grid.no_data.first().copied().flatten(),
    };
    static WRITTEN: AtomicU64 = AtomicU64::new(0);
    let written = WRITTEN.fetch_add(1, Ordering::Relaxed) + 1;
    let file = context.scratch.join(format!("katana-grid-{written}.tif"));
    let _ = std::fs::create_dir_all(context.scratch);
    GdalDataset::write_raster(&file, &options, &grid.bands[0])?;
    Ok(file)
}

fn unsupported(why: &str) -> Error {
    Error::new(ErrorCode::Unsupported, why)
}

/// The outputs of a run, applied: a vector result as ONE command on the
/// drawing (result_command), a raster as a derived reference raster, a file
/// named, text printed. The records, one per line. Unsupported for an output
/// the target cannot take (a vector into REFERENCE, a raster into a LAYER),
/// and for the targets whose packages are not built yet.
pub fn apply_outputs(
    context: &mut Context<'_>,
    request: &ApplyRequest,
    mut outputs: RunOutputs,
) -> Result<String> {
    let target = &request.target;
    let mut records: Vec<String> = Vec::new();
    match target.kind {
        TargetKind::Surface => {
            // ---- T0: Terrain session: TO SURFACE ----
            // A raster result triangulated as SURFACE FROM RASTER triangulates
            // a DEM - its true values on the stride that keeps it under the
            // cap - and kept under the name TO gives, which must be free. The
            // raster was spilled to a file of the run's (max_memory_cells 0),
            // removed once read; a grid kept in memory is written first, as
            // for REFERENCE.
            if outputs.features.is_some() {
                return Err(unsupported(
                    "features cannot be a surface; SURFACE FROM LAYERS <layer> triangulates what \
                     TO LAYER draws",
                ));
            }
            if let Some(grid) = outputs.raster.take() {
                let file = write_grid(context, &grid)?;
                outputs.file = Some(path_text(&file));
            }
            let raster = match &outputs.file {
                Some(file) => path_from_text(file),
                None => return Err(unsupported("the run made no raster to triangulate")),
            };
            if target.name.is_empty() || context.surfaces.find(&target.name).is_some() {
                let _ = std::fs::remove_file(raster.as_path());
                return Err(make_error(
                    ErrorCode::AlreadyExists,
                    "TO SURFACE needs a name no surface has; SURFACE LIST names them",
                    target.name.clone(),
                ));
            }
            let built = surface_from_raster_file(&raster, SURFACE_POINT_CAP, None);
            let _ = std::fs::remove_file(raster.as_path());
            let kept = keep_surface(
                context,
                target.name.clone(),
                request.result.command_name.clone(),
                false,
                built?,
            )?;
            let mut reply = format!(
                "output arg={} kind=raster target=surface name={}\n{kept}",
                value(&request.arg),
                value(&target.name)
            );
            for diagnostic in &outputs.diagnostics {
                if !diagnostic.failure {
                    reply.push('\n');
                    reply += &warning_record(&diagnostic.message);
                }
            }
            return Ok(reply);
        }
        TargetKind::Selection | TargetKind::Report => {
            // ---- V5: GIS SQL: TO SELECTION and TO REPORT ----
            // A query's rows as records, or its katana_id column as the
            // selection (vector_support.rs): GIS SQL's AS REPORT and AS SELECT
            // are these, and so is the GDAL verb's TO REPORT, TO SELECTION.
            let Some(features) = &outputs.features else {
                return Err(unsupported(
                    "TO SELECTION and TO REPORT take features, and this result has none",
                ));
            };
            if target.kind == TargetKind::Selection {
                records.push(select_features(context, features, &request.arg)?);
            } else {
                records = report_records(features, &request.arg);
            }
            for diagnostic in &outputs.diagnostics {
                if !diagnostic.failure {
                    records.push(warning_record(&diagnostic.message));
                }
            }
            return Ok(joined(&records));
        }
        TargetKind::Default | TargetKind::Layer | TargetKind::Reference | TargetKind::File => {}
    }

    // A grid kept in memory goes where a raster result is kept.
    if let Some(grid) = outputs.raster.take() {
        if target.kind == TargetKind::Layer || target.kind == TargetKind::File {
            return Err(unsupported(
                "a raster result cannot go to a layer; TO REFERENCE keeps it as a reference raster",
            ));
        }
        let file = write_grid(context, &grid)?;
        outputs.file = Some(path_text(&file));
    }
    if let Some(file) = &outputs.file {
        if target.kind == TargetKind::File {
            let driver = if target.format.is_empty() {
                String::new()
            } else {
                format!(" driver={}", value(&target.format))
            };
            records.push(format!(
                "output arg={} kind=file target=file file={}{driver}",
                value(&request.arg),
                value(file)
            ));
        } else if target.kind == TargetKind::Layer {
            return Err(unsupported(
                "a raster result cannot go to a layer; TO REFERENCE keeps it as a reference raster",
            ));
        } else {
            records.push(keep_derived(context, request, &path_from_text(file))?);
        }
    }
    if let Some(features) = &outputs.features {
        if target.kind == TargetKind::Reference {
            return Err(unsupported(
                "a vector result cannot be a reference raster; TO LAYER <path> draws it",
            ));
        }
        let mut options = request.result.clone();
        if options.target_layer.is_empty() {
            options.target_layer = if target.kind == TargetKind::Layer {
                target.name.clone()
            } else {
                format!("gis/{}", request.default_name)
            };
        }
        let mut plan = result_command(context.document.model(), features, &options)?;
        let mut created = Vec::new();
        if let Some(command) = plan.command.take() {
            context.document.execute(command)?;
            created = context.document.last_created_entities();
        }
        // A result in another coordinate system is drawn as it comes (GDAL
        // vector reproject to EPSG:4326 makes degrees): said, once per system,
        // rather than drawn silently off the drawing's.
        let mut crs_warnings: Vec<String> = Vec::new();
        for table in &features.tables {
            if let Some(differs) = crs_differs(
                &table.crs_wkt,
                &project_crs(context),
                "the result",
                "its coordinates were drawn as they are, not moved into the project's",
            ) && !crs_warnings.contains(&differs)
            {
                crs_warnings.push(differs);
            }
        }
        records.push(format!(
            "output arg={} kind=vector target=layer layer={} created={} updated={} deleted={} \
             skipped={}",
            value(&request.arg),
            value(&options.target_layer),
            plan.created,
            plan.updated,
            plan.deleted,
            plan.skipped
        ));
        for warning in &plan.warnings {
            records.push(warning_record(warning));
        }
        for warning in &crs_warnings {
            records.push(warning_record(warning));
        }
        if let Some(frame) = &context.front_end.frame
            && !created.is_empty()
        {
            let bounds = bounds_of(context.document, &created);
            if !bounds.empty() {
                frame(&bounds);
            }
        }
    }
    if let Some(text) = &outputs.text {
        records.push(text_record(text));
    }
    if let Some(code) = outputs.return_code {
        records.push(format!("return code={code}"));
    }
    for diagnostic in &outputs.diagnostics {
        if !diagnostic.failure {
            records.push(warning_record(&diagnostic.message));
        }
    }
    // One per line, as the C++ joins them: an empty record still takes its
    // place.
    Ok(super::replies::joined(&records, "\n"))
}

/// For an in-place apply (UpdateGeometry, SetProperties, REPLACE): the
/// entities as they were when the line was prepared, compared with the
/// drawing now. InvalidState "the drawing changed while the job ran" when any
/// differs or is gone: overwriting a concurrent edit silently would be worse
/// than refusing.
pub fn unchanged_since(context: &Context<'_>, copies: &[Entity]) -> Status {
    for copy in copies {
        match context.document.model().entities.find(copy.id) {
            Some(now) if now == copy => {}
            _ => {
                return Err(make_error(
                    ErrorCode::InvalidState,
                    "the drawing changed while the job ran",
                    format!("entity {}", copy.id),
                ));
            }
        }
    }
    Ok(())
}
