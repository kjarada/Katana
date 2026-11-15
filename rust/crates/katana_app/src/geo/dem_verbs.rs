//! The DEM tools: RASTER MOSAIC, CLIP, FILL, FOOTPRINT, REPROJECT and
//! DIFFERENCE (docs/terrain.md, "The DEM tools"; docs/geoprocessing.md, "T7"),
//! the port of src/katana_app/geo/dem_verbs.cpp.
//!
//! ```text
//! RASTER MOSAIC <raster> [<raster>...] [resolution=same|highest|lowest|average|<x>,<y>]
//!               [SAVE <file>]
//! RASTER CLIP <raster> <scope>              to AREA's box, or to the closed
//!                                           boundaries any other scope takes
//! RASTER FILL <raster> [distance=<cells>] [smoothing=<n>] [strategy=invdist|nearest]
//! RASTER FOOTPRINT <raster>                 [TO LAYER <path> | TO FILE <path>]
//! RASTER REPROJECT <raster> [crs=<crs> | like=<raster id|name>] [from=<crs>]
//!                  [resampling=<method>] [cell=<m>]
//! RASTER DIFFERENCE <raster> [MINUS] <raster> [<scope>] [resampling=<method>]
//! and on each: [NAME <name>] [TO REFERENCE [<name>] | TO FILE <path> [FORMAT <driver>]]
//!              [OVERWRITE] [PREVIEW]
//!
//! <raster> := RASTER <id|name> | SURFACE <name> [CELL <m>] | FILE <path>
//! ```
//!
//! Each is a thin module over the one executor: its sources and targets are
//! read by the ONE source and target parsers (bindings.rs), a scope by the ONE
//! scope parser, and its result applied by apply_outputs - a raster as a
//! derived reference raster, features as one undo step. The rasters are GDAL's
//! algorithms (`raster mosaic`, `clip`, `fill-nodata`, `footprint`,
//! `reproject`, `calc`), pinned by tests/geo/test_dem_verbs.cpp's contract
//! test.
//!
//! Decisions (docs/terrain.md has the reasons): a mosaic is a VRT, light, its
//! tiles read where they are, and SAVE writes it out as a file of its own,
//! kept as the reference raster (a keyword, not save=: an option's value is
//! one unquoted word, and a path may hold blanks). CLIP's AREA is the box
//! itself; any other scope clips to the closed boundaries it takes (GDAL's
//! raster clip keeps every cell a boundary touches, not only those whose
//! centre is inside), and the cells outside them hold no value even when the
//! raster declares no no-data value (with_no_data). REPROJECT refuses a raster
//! with no coordinate system rather than reprojecting from nowhere: GDAL does
//! so without a word (measured: a plane with no CRS "reprojected" to
//! EPSG:28356 came back unchanged). DIFFERENCE aligns the second raster to the
//! first's grid (`raster reproject`, bilinear) only when their grids differ,
//! subtracts with `raster calc`'s builtin diff (this GDAL has neither muparser
//! nor ExprTk), and sums cut and fill with a compensated sum times the cell
//! area.
//!
//! Its cell= is read by dem_support's positive_option ("cell= is a positive
//! number"): dem_verbs.cpp sees both overloads, and the call's `const
//! std::string&` argument resolves to dem_support's. Its records are joined
//! by dem_support's join_records.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Keep the C++'s negated comparisons, which refuse NaN."
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use katana_cad::global_modify::matches_pattern;
use katana_cad::scope_verbs::{ScopeSource, ScopeWords, is_scope_word, parse_scope_words};
use katana_core::error::make_error;
use katana_core::text::{format_exact_real, parse_finite_double};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_gis::processing::{
    self as gp, ArgValue, DatasetPath, DatasetValue, FeatureSet, OutputTo, RunRequest, validate,
};
use katana_gis::{GdalDataset, GeometryKind, RasterInfo};
use katana_interop::geo::DrawingDatasetOptions;
use katana_interop::geo::drawing_dataset::ResultOptions;
use katana_interop::geo::raster_products::derived_raster_path;
use katana_interop::terrain_io::suggested_cell_size;
use katana_math::{CompensatedSum, cpp};

use super::bindings::{
    ApplyRequest, DeferredDataset, Source, SourceKind, Target, TargetKind, Tokens, apply_outputs,
    bind_drawing, bind_raster, derived_folder, input_record, parse_source, parse_target,
    project_crs, scope_record,
};
use super::dem_support::{
    BandValues, cell_area, holds_value, join_records, keep_in_place, move_file, path_of_utf8,
    positive_option, read_band_one, read_band_one_from_file, split_options, utf8_of_path,
};
use super::geo_verbs::{Context, Prepared, make_apply, make_work, stop_requested, trimmed_text};
use super::replies::{fixed3, value};
use super::terrain_verbs::raster_info_of;

const RASTER_SOURCES: &str = "RASTER <id|name>, SURFACE <name> [CELL <m>] or FILE <path>";

/// interop's suggested_cell_size's default: about 1000 cells along the
/// longer side (terrain_io.hpp).
const SUGGESTED_CELLS: i32 = 1000;

/// An algorithm's path, as the bridge takes it; tokens likewise.
fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| String::from(*item)).collect()
}

/// What one DEM verb takes.
#[derive(Clone, Copy, Debug)]
struct DemGrammar {
    /// "RASTER CLIP".
    verb: &'static str,
    /// Its key=value options.
    keys: &'static [&'static str],
    min_sources: usize,
    max_sources: usize,
    takes_scope: bool,
    takes_surface: bool,
    takes_save: bool,
    /// MINUS between the two rasters: DIFFERENCE's word for which is taken
    /// from which, optional as the design's contract gives it.
    takes_minus: bool,
}

/// A DEM verb's line, read.
#[derive(Clone, Debug, Default)]
struct DemLine {
    options: BTreeMap<String, String>,
    /// MINUS was given (and a second raster must follow).
    minus: bool,
    sources: Vec<Source>,
    scope: Option<ScopeWords>,
    target: Option<Target>,
    name: String,
    save: Option<String>,
    preview: bool,
    overwrite: bool,
}

impl DemLine {
    fn option(&self, key: &str) -> Option<String> {
        self.options.get(key).cloned()
    }
}

fn read_line(tokens: &Tokens, grammar: &DemGrammar) -> Result<DemLine> {
    let words = split_options(tokens, 2, grammar.keys, grammar.verb)?;
    let mut line = DemLine {
        options: words.options,
        ..DemLine::default()
    };
    let rest = &words.rest;
    let verb = grammar.verb;
    let mut at = 0;
    while at < rest.size() {
        if rest.is(at, "RASTER") || rest.is(at, "SURFACE") || rest.is(at, "FILE") {
            if line.sources.len() == grammar.max_sources {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!(
                        "{verb} takes {}{}",
                        grammar.max_sources,
                        if grammar.max_sources == 1 {
                            " raster"
                        } else {
                            " rasters"
                        }
                    ),
                    rest[at].clone(),
                ));
            }
            if rest.is(at, "SURFACE") && !grammar.takes_surface {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!(
                        "{verb} takes rasters and files; a surface is no tile - RASTER \
                         <id|name> or FILE <path|folder|pattern>"
                    ),
                    rest[at].clone(),
                ));
            }
            line.sources.push(parse_source(rest, &mut at)?);
        } else if rest.is(at, "NAME") {
            if at + 1 >= rest.size() || !line.name.is_empty() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "NAME takes the reference raster's name, once",
                    rest[at].clone(),
                ));
            }
            line.name = rest[at + 1].clone();
            at += 2;
        } else if grammar.takes_save && rest.is(at, "SAVE") {
            if at + 1 >= rest.size() || line.save.is_some() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "SAVE takes the file to write, once",
                    rest[at].clone(),
                ));
            }
            line.save = Some(rest[at + 1].clone());
            at += 2;
        } else if rest.is(at, "TO") {
            if line.target.is_some() {
                return Err(Error::new(ErrorCode::InvalidArgument, "one TO per line"));
            }
            at += 1;
            line.target = Some(parse_target(rest, &mut at)?);
        } else if grammar.takes_minus && rest.is(at, "MINUS") {
            // Only between the first raster and the second, once.
            if line.sources.len() != 1 || line.minus {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!("{verb} takes MINUS once, between the two rasters"),
                    rest[at].clone(),
                ));
            }
            line.minus = true;
            at += 1;
        } else if rest.is(at, "PREVIEW") {
            line.preview = true;
            at += 1;
        } else if rest.is(at, "OVERWRITE") {
            line.overwrite = true;
            at += 1;
        } else if grammar.takes_scope
            && line.scope.is_none()
            && !rest.quoted[at]
            && is_scope_word(&rest[at])
        {
            line.scope = Some(parse_scope_words(&rest.words, &mut at)?);
        } else {
            let mut takes = format!("{verb} takes {RASTER_SOURCES}");
            if grammar.takes_scope {
                takes += ", a scope";
            }
            for key in grammar.keys {
                takes += &format!(", {key}=");
            }
            if grammar.takes_save {
                takes += ", SAVE";
            }
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("{takes}, NAME, TO, OVERWRITE and PREVIEW"),
                rest[at].clone(),
            ));
        }
    }
    if line.sources.len() < grammar.min_sources {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "{verb} needs {}: {RASTER_SOURCES}",
                if grammar.min_sources == 1 {
                    String::from("a raster")
                } else {
                    format!("{} rasters", grammar.min_sources)
                }
            ),
        ));
    }
    Ok(line)
}

fn real(number: f64) -> String {
    format_exact_real(number)
}

/// A cell count option: a whole number from `least`.
fn whole_option(key: &str, text: &str, least: i64) -> Result<i64> {
    let refused = |number: f64| number != number.floor() || number < least as f64 || number > 1e9;
    match parse_finite_double(text) {
        Some(number) if !refused(number) => Ok(number as i64),
        _ => Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{key}= is a whole number from {least}"),
            format!("{key}={text}"),
        )),
    }
}

/// The rasters a line reads, bound as the datasets GDAL is given; a surface
/// takes the cell its extent suggests when the line gives none, so its input
/// record says what was used.
fn bind_sources(context: &Context<'_>, sources: &mut [Source]) -> Result<Vec<DeferredDataset>> {
    let mut bound = Vec::new();
    for source in sources.iter_mut() {
        if source.kind == SourceKind::Drawing {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("the drawing is no raster here; give {RASTER_SOURCES}"),
            ));
        }
        if source.kind == SourceKind::Surface
            && source.cell.is_none()
            && let Some(surface) = context
                .surfaces
                .find(&source.surface)
                .and_then(|named| named.surface.as_ref())
        {
            source.cell = Some(suggested_cell_size(surface.bounds(), SUGGESTED_CELLS));
        }
        bound.push(bind_raster(context, source)?);
    }
    Ok(bound)
}

fn input_records(sources: &[Source], args: &[&str]) -> Vec<String> {
    sources
        .iter()
        .enumerate()
        .map(|(i, source)| {
            let arg = if i < args.len() {
                args[i]
            } else {
                args[args.len() - 1]
            };
            input_record(arg, source)
        })
        .collect()
}

fn file_in_the_way(path: &str) -> bool {
    path_of_utf8(path).exists()
}

/// Where a raster result goes: a derived reference raster unless TO says
/// otherwise, written once where it is kept (the GDAL verb's rule). A layer is
/// refused, and so is a file in the way without OVERWRITE.
fn raster_target(
    context: &Context<'_>,
    line: &DemLine,
    verb: &str,
    default_name: &str,
    request: &mut RunRequest,
    text: &str,
) -> Result<ApplyRequest> {
    let to = line.target.clone().unwrap_or_default();
    if matches!(
        to.kind,
        TargetKind::Layer | TargetKind::Selection | TargetKind::Report
    ) {
        return Err(Error::new(
            ErrorCode::Unsupported,
            format!(
                "{verb} makes a raster: TO REFERENCE [<name>] keeps it as a reference raster, TO \
                 FILE <path> writes it"
            ),
        ));
    }
    if !line.name.is_empty() && to.kind == TargetKind::Reference && !to.name.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "NAME and TO REFERENCE both name the raster; give one",
        ));
    }
    if to.kind == TargetKind::File {
        if file_in_the_way(&to.name) && !line.overwrite {
            return Err(make_error(
                ErrorCode::AlreadyExists,
                "the file exists; add OVERWRITE to replace it",
                to.name.clone(),
            ));
        }
        request.output_to = OutputTo::File;
        request.output_path = to.name.clone();
        request.output_format = to.format.clone();
    } else {
        request.output_to = OutputTo::Memory;
        request.max_memory_cells = 0;
        request.spill_directory = utf8_of_path(&derived_folder(context));
    }
    request.overwrite = line.overwrite;
    let mut apply = ApplyRequest {
        target: to.clone(),
        default_name: String::from(default_name),
        result: ResultOptions {
            operation: gp::path_text(&request.path),
            command_name: String::from(trimmed_text(text)),
            ..ResultOptions::default()
        },
        ..ApplyRequest::default()
    };
    if to.kind == TargetKind::Default || to.kind == TargetKind::Reference {
        apply.target.kind = TargetKind::Reference;
        apply.target.name = if to.name.is_empty() {
            line.name.clone()
        } else {
            to.name.clone()
        };
    }
    Ok(apply)
}

fn answered(title: impl Into<String>, reply: impl Into<String>) -> Prepared {
    Prepared {
        title: title.into(),
        reply: Some(reply.into()),
        work: None,
    }
}

/// The datasets of a run, made on the worker, bound by argument: a list
/// argument takes all of its datasets.
#[derive(Clone)]
struct Bound {
    arg: String,
    list: bool,
    datasets: Vec<DeferredDataset>,
}

fn bound(arg: &str, list: bool, datasets: Vec<DeferredDataset>) -> Bound {
    Bound {
        arg: String::from(arg),
        list,
        datasets,
    }
}

fn materialise(bound: &[Bound], run: &mut RunRequest) -> Status {
    for arg in bound {
        let mut values = Vec::new();
        for make in &arg.datasets {
            values.push(make()?);
        }
        if arg.list {
            run.values
                .push((arg.arg.clone(), ArgValue::Datasets(values)));
        } else {
            let first = values
                .into_iter()
                .next()
                .expect("an argument is bound to a dataset");
            run.values.push((arg.arg.clone(), ArgValue::Dataset(first)));
        }
    }
    Ok(())
}

/// A check of the materialised request, on the worker, before the run.
type PreRun = Box<dyn FnOnce(&RunRequest) -> Status + Send>;
/// A change to the materialised request on the worker, before the run and
/// never for a PREVIEW: the intermediate files it made, removed after the run.
type BeforeRun =
    Box<dyn FnOnce(&mut RunRequest, Option<&AtomicBool>) -> Result<Vec<String>> + Send>;

/// A PreRun from a closure, its reference's lifetime left open.
fn pre_run<F>(check: F) -> PreRun
where
    F: FnOnce(&RunRequest) -> Status + Send + 'static,
{
    Box::new(check)
}

/// A BeforeRun from a closure, its references' lifetimes left open.
fn before_run<F>(before: F) -> BeforeRun
where
    F: FnOnce(&mut RunRequest, Option<&AtomicBool>) -> Result<Vec<String>> + Send + 'static,
{
    Box::new(before)
}

/// Removes a run's intermediate files.
fn remove_files(files: &[String]) {
    for file in files {
        let _ = std::fs::remove_file(path_of_utf8(file));
    }
}

/// One raster run on the worker whose raster result is wanted as a dataset:
/// kept in memory, or in a file in `folder` when it is large.
fn intermediate(
    mut run: RunRequest,
    folder: &Path,
    stop: Option<&AtomicBool>,
    files: &mut Vec<String>,
) -> Result<DatasetValue> {
    run.output_to = OutputTo::Memory;
    run.spill_directory = utf8_of_path(folder);
    let outputs = gp::run(&run, stop, None)?;
    if let Some(raster) = outputs.raster {
        return Ok(DatasetValue::Raster(raster));
    }
    if let Some(file) = outputs.file {
        files.push(file.clone());
        return Ok(DatasetValue::Path(DatasetPath {
            path: file,
            ..DatasetPath::default()
        }));
    }
    Err(Error::new(
        ErrorCode::CommandRejected,
        format!("{} made no raster", gp::path_text(&run.path)),
    ))
}

/// `dataset` as GDAL's cutline must be given it: with a no-data value. A
/// raster clipped to boundaries (`raster clip --like`) has the cells of its
/// box outside them set to the source's no-data value - and to 0 when it
/// declares none, which then read as heights of 0 m (a sample of 0, a
/// difference summed over all 441 cells of a box whose triangle touches 231).
/// One that declares none is copied with no-data NaN: `raster calc`'s builtin
/// sum of it alone (the value itself; this GDAL has no other dialect) into
/// Float64, which holds every value of every integer or float band exactly. A
/// grid is given NaN as it is. The files made are added to `files`.
fn with_no_data(
    mut dataset: DatasetValue,
    folder: &Path,
    stop: Option<&AtomicBool>,
    files: &mut Vec<String>,
) -> Result<DatasetValue> {
    let info = raster_info_of(&dataset)?;
    if info.no_data_value.is_some() {
        return Ok(dataset);
    }
    if let DatasetValue::Raster(grid) = &mut dataset {
        if grid.data_type != "Float32" && grid.data_type != "Float64" {
            grid.data_type = String::from("Float64");
        }
        grid.no_data = vec![Some(f64::NAN); grid.bands.len()];
        grid.info.no_data_value = Some(f64::NAN);
        return Ok(dataset);
    }
    let copy = RunRequest {
        path: strings(&["raster", "calc"]),
        tokens: strings(&[
            "--dialect=builtin",
            "--calc=sum",
            "--nodata=nan",
            "--output-data-type=Float64",
        ]),
        values: vec![(String::from("input"), ArgValue::Datasets(vec![dataset]))],
        ..RunRequest::default()
    };
    intermediate(copy, folder, stop, files)
}

/// The phases of a verb of one run: PREVIEW validates it here; otherwise the
/// work makes its datasets, runs it and hands the outputs to apply_outputs,
/// the head record first. The C++'s defaults for `check` and `before` are
/// none.
#[allow(
    clippy::too_many_arguments,
    reason = "The C++'s oneRun takes these nine, the line's whole run."
)]
fn one_run(
    title: &str,
    request: RunRequest,
    bound: Vec<Bound>,
    apply: ApplyRequest,
    head: String,
    mut records: Vec<String>,
    preview: bool,
    check: Option<PreRun>,
    before: Option<BeforeRun>,
) -> Result<Prepared> {
    if preview {
        let mut trial = request.clone();
        materialise(&bound, &mut trial)?;
        if let Some(check) = check {
            check(&trial)?;
        }
        validate(&trial)?;
        records.insert(0, format!("{head} preview=yes"));
        records.push(String::from("preview valid=yes changed=no"));
        return Ok(answered(title, join_records(&records)));
    }
    let work = make_work(move |stop, progress| {
        let mut run = request;
        materialise(&bound, &mut run)?;
        if let Some(check) = check {
            check(&run)?;
        }
        let mut intermediates = Vec::new();
        if let Some(before) = before {
            intermediates = before(&mut run, stop)?;
        }
        let outputs = gp::run(&run, stop, progress);
        remove_files(&intermediates);
        let outputs = outputs?;
        Ok(make_apply(move |context| {
            let seconds = outputs.seconds;
            let applied = apply_outputs(context, &apply, outputs)?;
            let mut reply = vec![format!("{head} seconds={}", fixed3(seconds))];
            reply.extend(records);
            reply.push(applied);
            Ok(join_records(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from(title),
        reply: None,
        work: Some(work),
    })
}

fn cancelled() -> Error {
    Error::new(ErrorCode::InvalidState, "cancelled")
}

// ---- MOSAIC ---------------------------------------------------------------------------------

static MOSAIC_OPTIONS: [&str; 1] = ["resolution"];

fn has_wildcard(text: &str) -> bool {
    text.contains(['*', '?'])
}

fn passes_through(path: &str) -> bool {
    path.starts_with("/vsi") || path.contains("://")
}

/// The last element of a line's path, as std::filesystem's filename() gives
/// it: nothing for a path that ends in a separator, where Rust's file_name()
/// takes the folder before it.
fn file_name_of(given: &Path, text: &str) -> String {
    if text.ends_with('/') || (cfg!(windows) && text.ends_with('\\')) {
        return String::new();
    }
    given
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The files a FILE source of a mosaic names, on the worker: a folder is the
/// rasters in it (what GDAL opens as one; a sidecar or a .prj is not), a
/// pattern the files its name matches ('*', '?'), anything else itself. In
/// name order, so a mosaic of a folder is the same whatever order the file
/// system lists it in. NotFound for a folder or pattern that names none.
fn tiles_of(path: &str) -> Result<Vec<String>> {
    if passes_through(path) {
        return Ok(vec![String::from(path)]);
    }
    let given = path_of_utf8(path);
    let name = file_name_of(&given, path);
    let mut found: Vec<PathBuf> = Vec::new();
    if given.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&given) {
            for entry in entries.flatten() {
                let file = entry.path();
                if !file.is_file() {
                    continue;
                }
                if GdalDataset::open(&file).is_ok_and(|opened| opened.has_raster()) {
                    found.push(file);
                }
            }
        }
    } else if has_wildcard(&name) {
        let folder = match given.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
            _ => PathBuf::from("."),
        };
        if let Ok(entries) = std::fs::read_dir(folder) {
            for entry in entries.flatten() {
                let file = entry.path();
                if file.is_file()
                    && matches_pattern(&utf8_of_path(Path::new(&entry.file_name())), &name)
                {
                    found.push(file);
                }
            }
        }
    } else {
        return Ok(vec![String::from(path)]);
    }
    if found.is_empty() {
        return Err(make_error(
            ErrorCode::NotFound,
            "no raster there to mosaic",
            path,
        ));
    }
    found.sort();
    Ok(found
        .iter()
        .map(|file| utf8_of_path(file.as_path()))
        .collect())
}

/// A name no scratch file of another run has: the derived folder is shared by
/// every job of the front end, and by other processes on the same project.
/// The C++ stamps it with steady_clock's count, which Rust's Instant does not
/// show; the system clock's nanoseconds serve the same end (own_scratch
/// stamps its folder so too).
fn scratch_name(stem: &str, extension: &str) -> String {
    static MADE: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let made = MADE.fetch_add(1, Ordering::Relaxed) + 1;
    format!("katana-{stem}-{now}-{made}{extension}")
}

fn prepare_mosaic(context: &mut Context<'_>, tokens: &Tokens, text: &str) -> Result<Prepared> {
    let grammar = DemGrammar {
        verb: "RASTER MOSAIC",
        keys: &MOSAIC_OPTIONS,
        min_sources: 1,
        max_sources: 4096,
        takes_scope: false,
        takes_surface: false,
        takes_save: true,
        takes_minus: false,
    };
    let mut line = read_line(tokens, &grammar)?;
    let save = line.save.clone();
    let to = line.target.clone().unwrap_or_default();
    if save.is_some() && line.target.is_some() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "SAVE writes the mosaic and keeps it as a reference raster; TO says where else it \
             goes - give one",
        ));
    }
    if !matches!(
        to.kind,
        TargetKind::Default | TargetKind::Reference | TargetKind::File
    ) {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "a mosaic is a reference raster (TO REFERENCE [<name>], or SAVE <file> to keep it in \
             a file of its own) or a file (TO FILE <path>)",
        ));
    }
    if !line.name.is_empty() && to.kind == TargetKind::Reference && !to.name.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "NAME and TO REFERENCE both name the raster; give one",
        ));
    }
    let written = if save.is_some() {
        save.clone()
    } else if to.kind == TargetKind::File {
        Some(to.name.clone())
    } else {
        None
    };
    if let Some(written) = &written
        && file_in_the_way(written)
        && !line.overwrite
    {
        return Err(make_error(
            ErrorCode::AlreadyExists,
            "the file exists; add OVERWRITE to replace it",
            written.clone(),
        ));
    }

    let datasets = bind_sources(context, &mut line.sources)?;
    let mut records = input_records(&line.sources, &["input"]);

    let mut request = RunRequest {
        path: strings(&["raster", "mosaic"]),
        overwrite: line.overwrite,
        ..RunRequest::default()
    };
    if let Some(resolution) = line.option("resolution") {
        request.tokens.push(format!("--resolution={resolution}"));
    }
    let virtual_mosaic = written.is_none();
    let mut vrt = PathBuf::new();
    match &written {
        None => {
            // A VRT names its tiles; with absolute paths it reads them
            // wherever the VRT itself is kept.
            request.tokens.push(String::from("--absolute-path"));
            vrt = derived_folder(context).join(scratch_name("mosaic", ".vrt"));
            request.output_to = OutputTo::File;
            request.output_path = utf8_of_path(&vrt);
            request.output_format = String::from("VRT");
        }
        Some(written) => {
            request.output_to = OutputTo::File;
            request.output_path = written.clone();
            request.output_format = if to.kind == TargetKind::File {
                to.format.clone()
            } else {
                String::new()
            };
        }
    }
    let name = if !line.name.is_empty() {
        line.name.clone()
    } else if !to.name.is_empty() && save.is_none() {
        to.name.clone()
    } else {
        String::from("mosaic")
    };
    let derivation = String::from(trimmed_text(text));
    let project = context.document.project_directory();
    let sources = line.sources.clone();
    let head = String::from("mosaic algorithm=\"raster mosaic\"");

    if line.preview {
        records.insert(0, format!("{head} preview=yes"));
        records.push(String::from("preview valid=yes changed=no"));
        return Ok(answered("RASTER MOSAIC", join_records(&records)));
    }

    let kept_as_file = save.is_some();
    let work = make_work(move |stop, progress| {
        // The folder a VRT is written to is made when the first is.
        if virtual_mosaic && let Some(parent) = vrt.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // The tiles: rasters as their files, folders and patterns expanded.
        let mut tiles: Vec<DatasetValue> = Vec::new();
        for (i, source) in sources.iter().enumerate() {
            if stop_requested(stop) {
                return Err(cancelled());
            }
            if source.kind == SourceKind::File {
                for file in tiles_of(&source.path)? {
                    tiles.push(DatasetValue::Path(DatasetPath {
                        path: file,
                        ..DatasetPath::default()
                    }));
                }
                continue;
            }
            tiles.push(datasets[i]()?);
        }
        let mut run = request;
        let count = tiles.len();
        run.values
            .push((String::from("input"), ArgValue::Datasets(tiles)));
        let outputs = gp::run(&run, stop, progress)?;
        let first = format!(
            "{head} tiles={count} virtual={} seconds={}",
            if virtual_mosaic { "yes" } else { "no" },
            fixed3(outputs.seconds)
        );
        let file = run.output_path.clone();
        Ok(make_apply(move |ctx| {
            let mut reply = vec![first];
            reply.extend(records);
            if !virtual_mosaic && !kept_as_file {
                reply.push(format!(
                    "output arg=output kind=file target=file file={}",
                    value(&file)
                ));
                return Ok(join_records(&reply));
            }
            let mut kept = path_of_utf8(&file);
            let mut chosen = name.clone();
            if virtual_mosaic {
                // Into the derived folder under a name no raster has, as a
                // derived raster is kept.
                let mut copy = 2;
                loop {
                    let taken = ctx
                        .reference
                        .rasters()
                        .iter()
                        .any(|raster| raster.name.eq_ignore_ascii_case(&chosen));
                    kept = derived_raster_path(project.as_deref(), ctx.scratch, &chosen)
                        .with_extension("vrt");
                    if !taken && !kept.exists() {
                        break;
                    }
                    chosen = format!("{name}-{copy}");
                    copy += 1;
                }
                move_file(&vrt, &kept)?;
            }
            let record = keep_in_place(
                ctx,
                &kept,
                &chosen,
                &derivation,
                kept_as_file || project.is_some(),
                "output",
            )?;
            reply.push(record);
            Ok(join_records(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from("RASTER MOSAIC"),
        reply: None,
        work: Some(work),
    })
}

// ---- CLIP -----------------------------------------------------------------------------------

fn prepare_clip(context: &mut Context<'_>, tokens: &Tokens, text: &str) -> Result<Prepared> {
    let grammar = DemGrammar {
        verb: "RASTER CLIP",
        keys: &[],
        min_sources: 1,
        max_sources: 1,
        takes_scope: true,
        takes_surface: true,
        takes_save: false,
        takes_minus: false,
    };
    let mut line = read_line(tokens, &grammar)?;
    let Some(scope) = line.scope.clone() else {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "RASTER CLIP needs what to clip to: AREA x0,y0,x1,y1 for a box, or a scope whose \
             closed boundaries clip it (SELECTION, LAYERS a,b, DRAWING WHERE ...)",
        ));
    };
    let mut request = RunRequest {
        path: strings(&["raster", "clip"]),
        ..RunRequest::default()
    };
    let apply = raster_target(context, &line, "RASTER CLIP", "clip", &mut request, text)?;
    let datasets = bind_sources(context, &mut line.sources)?;
    let mut args = vec![bound("input", true, datasets)];
    let mut records = input_records(&line.sources, &["input"]);
    let mut head = String::from("clip algorithm=\"raster clip\"");

    let mut before: Option<BeforeRun> = None;
    if scope.source == ScopeSource::Area {
        // AREA is the box itself; a filter would pick boundaries, which a box
        // has none of.
        if !scope.filter.empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "RASTER CLIP clips to AREA's box itself; a WHERE filter picks boundaries - give \
                 DRAWING WHERE ... or LAYERS ... instead",
            ));
        }
        let area = format!(
            "{},{},{},{}",
            real(scope.area.min.x),
            real(scope.area.min.y),
            real(scope.area.max.x),
            real(scope.area.max.y)
        );
        request.tokens.push(format!("--bbox={area}"));
        head += &format!(" by=area area={area}");
    } else {
        let options = DrawingDatasetOptions {
            crs_wkt: project_crs(context),
            ..DrawingDatasetOptions::default()
        };
        let mut drawing = bind_drawing(context, &scope, &options)?;
        // The closed boundaries alone: points and open lines bound nothing.
        let mut boundaries = FeatureSet::default();
        for table in std::mem::take(&mut drawing.dataset.set.tables) {
            if table.kind == GeometryKind::Polygon {
                boundaries.tables.push(table);
            }
        }
        records.push(input_record("like", &Source::default()));
        records.push(scope_record("like", &drawing));
        let count = drawing.dataset.stats.polygons;
        head += &format!(" by=boundaries boundaries={count}");
        if boundaries.tables.is_empty() {
            records.insert(0, format!("{head} ran=no"));
            return Ok(answered("RASTER CLIP", join_records(&records)));
        }
        let set = Arc::new(boundaries);
        let like: DeferredDataset = Arc::new(move || -> Result<DatasetValue> {
            Ok(DatasetValue::Features((*set).clone()))
        });
        args.push(bound("like", false, vec![like]));
        // Outside the boundaries is no value, whether or not the raster
        // declares one (with_no_data).
        let scratch = context.scratch.to_path_buf();
        before = Some(before_run(move |run, stop| {
            let mut files = Vec::new();
            for (name, held) in &mut run.values {
                let ArgValue::Datasets(inputs) = held else {
                    continue;
                };
                if name.as_str() != "input" {
                    continue;
                }
                for input in inputs.iter_mut() {
                    match with_no_data(std::mem::take(input), &scratch, stop, &mut files) {
                        Ok(given) => *input = given,
                        Err(error) => {
                            remove_files(&files);
                            return Err(error);
                        }
                    }
                }
            }
            Ok(files)
        }));
    }
    one_run(
        "RASTER CLIP",
        request,
        args,
        apply,
        head,
        records,
        line.preview,
        None,
        before,
    )
}

// ---- FILL -----------------------------------------------------------------------------------

static FILL_OPTIONS: [&str; 3] = ["distance", "smoothing", "strategy"];

fn empty_cells(band: &BandValues) -> usize {
    band.values
        .iter()
        .filter(|&&cell| !holds_value(band, cell))
        .count()
}

fn prepare_fill(context: &mut Context<'_>, tokens: &Tokens, text: &str) -> Result<Prepared> {
    let grammar = DemGrammar {
        verb: "RASTER FILL",
        keys: &FILL_OPTIONS,
        min_sources: 1,
        max_sources: 1,
        takes_scope: false,
        takes_surface: true,
        takes_save: false,
        takes_minus: false,
    };
    let mut line = read_line(tokens, &grammar)?;
    let mut request = RunRequest {
        path: strings(&["raster", "fill-nodata"]),
        ..RunRequest::default()
    };
    if let Some(distance) = line.option("distance") {
        let cells = whole_option("distance", &distance, 1)?;
        request.tokens.push(format!("--max-distance={cells}"));
    }
    if let Some(smoothing) = line.option("smoothing") {
        let passes = whole_option("smoothing", &smoothing, 0)?;
        request
            .tokens
            .push(format!("--smoothing-iterations={passes}"));
    }
    if let Some(strategy) = line.option("strategy") {
        // GDAL's own choices; it refuses any other in its own words.
        request.tokens.push(format!("--strategy={strategy}"));
    }
    let apply = raster_target(context, &line, "RASTER FILL", "filled", &mut request, text)?;
    let datasets = bind_sources(context, &mut line.sources)?;
    let args = vec![bound("input", true, datasets)];
    let records = input_records(&line.sources, &["input"]);
    let head = String::from("fill algorithm=\"raster fill-nodata\"");
    if line.preview {
        return one_run(
            "RASTER FILL",
            request,
            args,
            apply,
            head,
            records,
            true,
            None,
            None,
        );
    }
    let applied = apply;
    let work = make_work(move |stop, progress| {
        let mut run = request;
        materialise(&args, &mut run)?;
        // What there is to fill, counted from the input as GDAL reads it.
        let ArgValue::Datasets(inputs) = &run.values[0].1 else {
            unreachable!("materialise binds the input as a list");
        };
        let before = read_band_one(&inputs[0])?;
        let empty = empty_cells(&before);
        if empty == 0 {
            let first = format!("{head} empty=0 filled=0 remaining=0 ran=no");
            return Ok(make_apply(move |_context| {
                let mut reply = vec![first];
                reply.extend(records);
                Ok(join_records(&reply))
            }));
        }
        let outputs = gp::run(&run, stop, progress)?;
        let mut remaining = 0;
        if let Some(file) = &outputs.file {
            let after = read_band_one_from_file(&path_of_utf8(file))?;
            remaining = empty_cells(&after);
        }
        let first = format!(
            "{head} empty={empty} filled={} remaining={remaining} seconds={}",
            empty.saturating_sub(remaining),
            fixed3(outputs.seconds)
        );
        Ok(make_apply(move |ctx| {
            let output = apply_outputs(ctx, &applied, outputs)?;
            let mut reply = vec![first];
            reply.extend(records);
            reply.push(output);
            Ok(join_records(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from("RASTER FILL"),
        reply: None,
        work: Some(work),
    })
}

// ---- FOOTPRINT ------------------------------------------------------------------------------

fn prepare_footprint(context: &mut Context<'_>, tokens: &Tokens, text: &str) -> Result<Prepared> {
    let grammar = DemGrammar {
        verb: "RASTER FOOTPRINT",
        keys: &[],
        min_sources: 1,
        max_sources: 1,
        takes_scope: false,
        takes_surface: true,
        takes_save: false,
        takes_minus: false,
    };
    let mut line = read_line(tokens, &grammar)?;
    let to = line.target.clone().unwrap_or_default();
    if !matches!(
        to.kind,
        TargetKind::Default | TargetKind::Layer | TargetKind::File
    ) {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "a footprint is areas: TO LAYER <path> draws them, TO FILE <path> writes them",
        ));
    }
    if !line.name.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a footprint is drawn on a layer, not named: TO LAYER <path>",
        ));
    }
    let mut request = RunRequest {
        path: strings(&["raster", "footprint"]),
        overwrite: line.overwrite,
        ..RunRequest::default()
    };
    if to.kind == TargetKind::File {
        if file_in_the_way(&to.name) && !line.overwrite {
            return Err(make_error(
                ErrorCode::AlreadyExists,
                "the file exists; add OVERWRITE to replace it",
                to.name.clone(),
            ));
        }
        request.output_to = OutputTo::File;
        request.output_path = to.name.clone();
        request.output_format = to.format.clone();
    }
    let datasets = bind_sources(context, &mut line.sources)?;
    let apply = ApplyRequest {
        target: to,
        default_name: String::from("footprint"),
        result: ResultOptions {
            operation: gp::path_text(&request.path),
            command_name: String::from(trimmed_text(text)),
            ..ResultOptions::default()
        },
        ..ApplyRequest::default()
    };
    let records = input_records(&line.sources, &["input"]);
    one_run(
        "RASTER FOOTPRINT",
        request,
        vec![bound("input", true, datasets)],
        apply,
        String::from("footprint algorithm=\"raster footprint\""),
        records,
        line.preview,
        None,
        None,
    )
}

// ---- REPROJECT ------------------------------------------------------------------------------

static REPROJECT_OPTIONS: [&str; 5] = ["crs", "from", "like", "resampling", "cell"];

fn prepare_reproject(context: &mut Context<'_>, tokens: &Tokens, text: &str) -> Result<Prepared> {
    let grammar = DemGrammar {
        verb: "RASTER REPROJECT",
        keys: &REPROJECT_OPTIONS,
        min_sources: 1,
        max_sources: 1,
        takes_scope: false,
        takes_surface: true,
        takes_save: false,
        takes_minus: false,
    };
    let mut line = read_line(tokens, &grammar)?;
    let crs = line.option("crs");
    let like = line.option("like");
    let from = line.option("from");
    if crs.is_some() && like.is_some() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "crs= and like= both say where the raster goes; give one",
        ));
    }
    let project = project_crs(context);
    if crs.is_none() && like.is_none() && project.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidCrs,
            "the drawing has no coordinate system to reproject to; give crs=<code> or \
             like=<raster>",
        ));
    }
    let mut request = RunRequest {
        path: strings(&["raster", "reproject"]),
        ..RunRequest::default()
    };
    let mut head = String::from("reproject algorithm=\"raster reproject\"");
    if like.is_none() {
        request.tokens.push(format!(
            "--output-crs={}",
            crs.as_deref().unwrap_or(&project)
        ));
        head += &format!(
            " crs={}",
            match &crs {
                Some(crs) => value(crs),
                None => String::from("project"),
            }
        );
    }
    if let Some(from) = &from {
        request.tokens.push(format!("--input-crs={from}"));
        head += &format!(" from={}", value(from));
    }
    if let Some(resampling) = line.option("resampling") {
        request.tokens.push(format!("--resampling={resampling}"));
        head += &format!(" resampling={}", value(&resampling));
    }
    if let Some(cell) = line.option("cell") {
        let length = positive_option("cell", &cell)?;
        request
            .tokens
            .push(format!("--resolution={},{}", real(length), real(length)));
        head += &format!(" cell={}", real(length));
    }
    let apply = raster_target(
        context,
        &line,
        "RASTER REPROJECT",
        "reprojected",
        &mut request,
        text,
    )?;
    let datasets = bind_sources(context, &mut line.sources)?;
    let mut args = vec![bound("input", true, datasets)];
    let mut records = input_records(&line.sources, &["input"]);
    if let Some(like) = &like {
        let like_source = Source {
            kind: SourceKind::Raster,
            raster: like.clone(),
            ..Source::default()
        };
        let like_dataset = bind_raster(context, &like_source)?;
        args.push(bound("like", false, vec![like_dataset]));
        records.push(input_record("like", &like_source));
        head += &format!(" like={}", value(like));
    }
    // A raster that says nothing of where it is cannot be reprojected: GDAL
    // would hand it back unchanged and say nothing.
    let check = if from.is_none() {
        Some(pre_run(|run| {
            let inputs = run
                .values
                .iter()
                .find(|(name, _)| name == "input")
                .map(|(_, held)| held);
            let Some(ArgValue::Datasets(inputs)) = inputs else {
                unreachable!("materialise binds the input as a list");
            };
            let info = raster_info_of(&inputs[0])?;
            if info.projection_wkt.is_empty() {
                return Err(Error::new(
                    ErrorCode::InvalidCrs,
                    "the raster has no coordinate system to reproject from; from=<crs> says what \
                     it is",
                ));
            }
            Ok(())
        }))
    } else {
        None
    };
    one_run(
        "RASTER REPROJECT",
        request,
        args,
        apply,
        head,
        records,
        line.preview,
        check,
        None,
    )
}

// ---- DIFFERENCE -----------------------------------------------------------------------------

static DIFFERENCE_OPTIONS: [&str; 1] = ["resampling"];

fn same_grid(a: &RasterInfo, b: &RasterInfo) -> bool {
    a.width == b.width
        && a.height == b.height
        && a.geotransform == b.geotransform
        && a.projection_wkt == b.projection_wkt
}

#[derive(Clone, Copy, Debug, Default)]
struct Volumes {
    cells: usize,
    cut: f64,
    fill: f64,
    area: f64,
}

/// Cut and fill of a difference (first minus second): a positive cell is the
/// first above the second - fill - and a negative one cut, each its depth
/// times the cell's area. Summed compensated, so a million cells of 0.3 add up
/// to what they are.
fn volumes_of(band: &BandValues) -> Volumes {
    let mut cut = CompensatedSum::default();
    let mut fill = CompensatedSum::default();
    let mut cells = 0;
    for &cell in &band.values {
        if !holds_value(band, cell) {
            continue;
        }
        cells += 1;
        if cell > 0.0 {
            fill.add(cell);
        } else if cell < 0.0 {
            cut.add(-cell);
        }
    }
    let area = cell_area(&band.geotransform);
    Volumes {
        cells,
        cut: cut.value() * area,
        fill: fill.value() * area,
        area: cells as f64 * area,
    }
}

fn cell_text(geotransform: &[f64; 6]) -> String {
    let x = geotransform[1].abs();
    let y = geotransform[5].abs();
    if x == y {
        real(x)
    } else {
        format!("{},{}", real(x), real(y))
    }
}

/// Both rasters must say where they are, or both say nothing.
fn check_crs(a: &RasterInfo, b: &RasterInfo) -> Status {
    if a.projection_wkt.is_empty() != b.projection_wkt.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidCrs,
            "one raster has a coordinate system and the other none, so they cannot be aligned; \
             reproject the other first (RASTER REPROJECT ... from=<crs>)",
        ));
    }
    Ok(())
}

fn prepare_difference(context: &mut Context<'_>, tokens: &Tokens, text: &str) -> Result<Prepared> {
    let grammar = DemGrammar {
        verb: "RASTER DIFFERENCE",
        keys: &DIFFERENCE_OPTIONS,
        min_sources: 2,
        max_sources: 2,
        takes_scope: true,
        takes_surface: true,
        takes_save: false,
        takes_minus: true,
    };
    let mut line = read_line(tokens, &grammar)?;
    let mut request = RunRequest {
        path: strings(&["raster", "calc"]),
        tokens: strings(&["--dialect=builtin", "--calc=diff"]),
        ..RunRequest::default()
    };
    let apply = raster_target(
        context,
        &line,
        "RASTER DIFFERENCE",
        "difference",
        &mut request,
        text,
    )?;
    let datasets = bind_sources(context, &mut line.sources)?;
    let mut records = input_records(&line.sources, &["first", "second"]);
    let resampling = line
        .option("resampling")
        .unwrap_or_else(|| String::from("bilinear"));

    // An optional scope limits the area: its closed boundaries clip the
    // difference before it is summed.
    let mut boundaries: Option<FeatureSet> = None;
    if let Some(scope) = &line.scope {
        let options = DrawingDatasetOptions {
            crs_wkt: project_crs(context),
            ..DrawingDatasetOptions::default()
        };
        let mut drawing = bind_drawing(context, scope, &options)?;
        let mut polygons = FeatureSet::default();
        for table in std::mem::take(&mut drawing.dataset.set.tables) {
            if table.kind == GeometryKind::Polygon {
                polygons.tables.push(table);
            }
        }
        records.push(scope_record("within", &drawing));
        if polygons.tables.is_empty() {
            records.insert(
                0,
                String::from(
                    "difference algorithm=\"raster calc\" within=boundaries boundaries=0 ran=no",
                ),
            );
            return Ok(answered("RASTER DIFFERENCE", join_records(&records)));
        }
        boundaries = Some(polygons);
    }
    let head = String::from("difference algorithm=\"raster calc\"");

    if line.preview {
        let first = datasets[0]()?;
        let second = datasets[1]()?;
        let a = raster_info_of(&first)?;
        let b = raster_info_of(&second)?;
        check_crs(&a, &b)?;
        records.insert(
            0,
            format!(
                "{head} aligned={} preview=yes",
                if same_grid(&a, &b) { "yes" } else { "no" }
            ),
        );
        records.push(String::from("preview valid=yes changed=no"));
        return Ok(answered("RASTER DIFFERENCE", join_records(&records)));
    }

    let applied = apply;
    let scratch = context.scratch.to_path_buf();
    let work = make_work(move |stop, progress| {
        let mut intermediates: Vec<String> = Vec::new();
        let first = datasets[0]()?;
        let second = datasets[1]()?;
        let a = raster_info_of(&first)?;
        let b = raster_info_of(&second)?;
        check_crs(&a, &b)?;
        let aligned = same_grid(&a, &b);
        let mut under = second;
        if !aligned {
            // The second onto the first's grid: its extent, its cells and its
            // CRS, said outright. GDAL's --like says the same in one word but
            // is ignored, without a word, when the rasters have no CRS
            // (measured: a 2 m grid "aligned" to a 1 m one stayed 2 m).
            let gt = a.geotransform;
            let x0 = gt[0];
            let x1 = gt[0] + f64::from(a.width) * gt[1];
            let y0 = gt[3] + f64::from(a.height) * gt[5];
            let y1 = gt[3];
            let mut align_tokens = vec![
                format!("--resampling={resampling}"),
                format!(
                    "--bbox={},{},{},{}",
                    real(cpp::min(x0, x1)),
                    real(cpp::min(y0, y1)),
                    real(cpp::max(x0, x1)),
                    real(cpp::max(y0, y1))
                ),
                format!("--size={},{}", a.width, a.height),
            ];
            if !a.projection_wkt.is_empty() {
                align_tokens.push(format!("--output-crs={}", a.projection_wkt));
            }
            let align = RunRequest {
                path: strings(&["raster", "reproject"]),
                tokens: align_tokens,
                values: vec![(String::from("input"), ArgValue::Datasets(vec![under]))],
                ..RunRequest::default()
            };
            match intermediate(align, &scratch, stop, &mut intermediates) {
                Ok(moved) => under = moved,
                Err(error) => {
                    remove_files(&intermediates);
                    return Err(error);
                }
            }
        }
        if stop_requested(stop) {
            remove_files(&intermediates);
            return Err(cancelled());
        }
        let mut run = request.clone();
        let kept_spill = run.spill_directory.clone();
        let clip_after = boundaries.is_some();
        if clip_after {
            // The whole difference is an intermediate: the clipped one is
            // kept.
            run.output_to = OutputTo::Memory;
            run.max_memory_cells = 0;
            run.spill_directory = utf8_of_path(&scratch);
        }
        run.values.push((
            String::from("input"),
            ArgValue::Datasets(vec![first, under]),
        ));
        let mut outputs = match gp::run(&run, stop, progress) {
            Ok(outputs) => outputs,
            Err(error) => {
                remove_files(&intermediates);
                return Err(error);
            }
        };
        if let Some(boundaries) = &boundaries {
            let Some(file) = outputs.file.clone() else {
                remove_files(&intermediates);
                return Err(Error::new(
                    ErrorCode::CommandRejected,
                    "raster calc made no raster",
                ));
            };
            intermediates.push(file.clone());
            // Outside the boundaries is no value, whether or not the
            // difference declares one (with_no_data).
            let whole = match with_no_data(
                DatasetValue::Path(DatasetPath {
                    path: file,
                    ..DatasetPath::default()
                }),
                &scratch,
                stop,
                &mut intermediates,
            ) {
                Ok(whole) => whole,
                Err(error) => {
                    remove_files(&intermediates);
                    return Err(error);
                }
            };
            let clip = RunRequest {
                path: strings(&["raster", "clip"]),
                output_to: request.output_to,
                output_path: request.output_path.clone(),
                output_format: request.output_format.clone(),
                overwrite: request.overwrite,
                max_memory_cells: 0,
                spill_directory: kept_spill,
                values: vec![
                    (String::from("input"), ArgValue::Datasets(vec![whole])),
                    (
                        String::from("like"),
                        ArgValue::Dataset(DatasetValue::Features(boundaries.clone())),
                    ),
                ],
                ..RunRequest::default()
            };
            let seconds = outputs.seconds;
            outputs = match gp::run(&clip, stop, None) {
                Ok(outputs) => outputs,
                Err(error) => {
                    remove_files(&intermediates);
                    return Err(error);
                }
            };
            outputs.seconds += seconds;
        }
        remove_files(&intermediates);
        let Some(file) = &outputs.file else {
            return Err(Error::new(
                ErrorCode::CommandRejected,
                "the difference made no raster",
            ));
        };
        let band = read_band_one_from_file(&path_of_utf8(file))?;
        let volumes = volumes_of(&band);
        let cell = cell_text(&band.geotransform);
        let first = format!(
            "{head} aligned={}{} cell={cell} cells={} area={} cut={} fill={} net={} method=grid \
             label={} seconds={}",
            if aligned { "yes" } else { "no" },
            if aligned {
                String::new()
            } else {
                format!(" resampling={}", value(&resampling))
            },
            volumes.cells,
            fixed3(volumes.area),
            fixed3(volumes.cut),
            fixed3(volumes.fill),
            fixed3(volumes.fill - volumes.cut),
            value(&format!("grid method, cell {cell} m")),
            fixed3(outputs.seconds)
        );
        Ok(make_apply(move |ctx| {
            let output = apply_outputs(ctx, &applied, outputs)?;
            let mut reply = vec![first];
            reply.extend(records);
            reply.push(output);
            Ok(join_records(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from("RASTER DIFFERENCE"),
        reply: None,
        work: Some(work),
    })
}

pub fn prepare_raster_mosaic(
    context: &mut Context<'_>,
    tokens: &Tokens,
    line: &str,
) -> Result<Prepared> {
    prepare_mosaic(context, tokens, line)
}

pub fn prepare_raster_clip(
    context: &mut Context<'_>,
    tokens: &Tokens,
    line: &str,
) -> Result<Prepared> {
    prepare_clip(context, tokens, line)
}

pub fn prepare_raster_fill(
    context: &mut Context<'_>,
    tokens: &Tokens,
    line: &str,
) -> Result<Prepared> {
    prepare_fill(context, tokens, line)
}

pub fn prepare_raster_footprint(
    context: &mut Context<'_>,
    tokens: &Tokens,
    line: &str,
) -> Result<Prepared> {
    prepare_footprint(context, tokens, line)
}

pub fn prepare_raster_reproject(
    context: &mut Context<'_>,
    tokens: &Tokens,
    line: &str,
) -> Result<Prepared> {
    prepare_reproject(context, tokens, line)
}

pub fn prepare_raster_difference(
    context: &mut Context<'_>,
    tokens: &Tokens,
    line: &str,
) -> Result<Prepared> {
    prepare_difference(context, tokens, line)
}
