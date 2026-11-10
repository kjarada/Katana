//! What the terrain verbs share (docs/terrain.md, "Surfaces on every front
//! end"; docs/geoprocessing.md, T0 to T3), the module of
//! src/katana_app/geo/terrain_verbs.hpp: the surface records, the one way a
//! surface is triangulated from a raster and kept in the store (which TO
//! SURFACE uses too), and the reading of their options. It holds
//! terrain_steps.cpp whole - the raster steps, sources and areas CONTOUR,
//! RASTER SHADE and RASTER SLOPE share - and the helpers of surface_verbs.cpp
//! that the other terrain verbs use; the SURFACE verb itself is
//! surface_verbs.rs (cad's precedent of a module per header).
//!
//! ```text
//! SURFACE LIST [JSON] | INFO <name> | REMOVE <name>
//! SURFACE FROM <source> [NAME <n>] [max=<points>] [AREA x0,y0,x1,y1] [classes=2,...]
//! SURFACE EXPORT <name> <file> [cell=<m>] [type=Float32|Float64] [cog] [co=K=V]...
//! CONTOUR, RASTER SHADE, RASTER SLOPE, RASTER ASPECT (contour_verbs.rs,
//! shade_verbs.rs, slope_verbs.rs)
//! ```
//!
//! The surfaces are the drawing's (Document::surfaces), staged for each phase
//! in `Context::surfaces` and committed by the executor as one undo step
//! (geo_verbs.rs); they are saved with the project (docs/terrain.md,
//! "Surfaces in the drawing").
//!
//! Two of these have a namesake elsewhere that answers differently, as in the
//! C++ (docs/rust.md "Not done"): [`positive_option`] here says "<key> is
//! greater than zero" where dem_support's says "<key>= is a positive number",
//! and [`joined_records`] here drops an empty record where gdal_verbs' keeps
//! it. Each call takes the one the C++ resolves it to.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Keep the C++'s negated comparisons, which refuse NaN."
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use katana_cad::scope_verbs::ScopeWords;
use katana_cad::terrain::surface_verbs::surface_record;
use katana_core::error::make_error;
use katana_core::text::{format_exact_real, parse_finite_double};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_geometry::{Box2, Point2, Polyline2};
use katana_gis::processing::{
    self as gp, ArgValue, DatasetPath, DatasetValue, Diagnostic, FeatureSet, OutputTo, RunRequest,
};
use katana_gis::{GdalDataset, RasterInfo};
use katana_interop::geo::contour_entities::boundaries_of;
use katana_interop::geo::{ContourBoundary, DrawingDatasetOptions};
use katana_math::cpp;
use katana_terrain::{
    DuplicatePointPolicy, NamedSurface, TinBuildOptions, TinInput, TinSurface, build_tin,
};

use super::bindings::{
    DeferredDataset, Source, SourceKind, Tokens, bind_drawing, bind_raster, input_record,
    parse_source, project_crs, scope_record,
};
use super::geo_verbs::{Context, Prepared, Progress, system_message};
use super::replies::warning_record;
use super::seams;
use crate::import_records::{path_from_text, path_text};

/// The most points a surface is triangulated from when nothing else is said:
/// the triangulation of 400 000 points took 1.7 s on the window's job
/// (docs/terrain.md, "Jobs"), and a ground surface does not carry two million
/// points of information. SURFACE FROM ... max= changes it.
pub const SURFACE_POINT_CAP: usize = 400_000;

// One surface's record is cad's surface_record (cad/terrain/surface_verbs.rs),
// written the same by every verb that names a surface.

/// A surface triangulated and not yet kept: made by a job's work, kept by its
/// apply. `records` say how it was made - how a raster was sampled, how many
/// coincident points were merged - and come before the surface's own record.
#[derive(Clone, Debug, Default)]
pub struct SurfaceBuild {
    pub surface: Arc<TinSurface>,
    pub records: Vec<String>,
}

// ---- shared with the other terrain verbs (surface_verbs.cpp) -----------------------------------

/// Triangulated, coincident points averaged (a scan lands on one mark again
/// and again, millimetres apart in height). InvalidArgument for fewer than
/// three points; what the builder refused otherwise.
pub fn triangulate(input: TinInput, options: &TinBuildOptions) -> Result<SurfaceBuild> {
    if input.points.len() < 3 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "fewer than three points with a height to triangulate",
        ));
    }
    let built = build_tin(&input, options)?;
    let mut records = Vec::new();
    if built.report.duplicate_point_count > 0 {
        records.push(format!(
            "merged duplicates={} policy=average",
            built.report.duplicate_point_count
        ));
    }
    Ok(SurfaceBuild {
        surface: Arc::new(built.surface),
        records,
    })
}

/// The heights of the raster at `path`, sampled on the stride that keeps them
/// under `max_points` (the true band values), then triangulated. `area` cuts
/// the raster first.
pub fn surface_from_raster_file(
    path: &Path,
    max_points: usize,
    area: Option<Box2>,
) -> Result<SurfaceBuild> {
    // The band's TRUE values, read again from the file through GDAL - never
    // the 8-bit display copy a reference raster holds (audit QT-23) - on a
    // stride that keeps the whole extent under the cap (audit QT-24).
    let elevations = seams::raster_elevations(path, max_points, area)?;
    let mut records = vec![format!(
        "sampled stride={} pixels={} nodata={} points={}",
        elevations.stride,
        elevations.sampled,
        elevations.no_data,
        elevations.points.len()
    )];
    if elevations.points.len() < 3 {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "fewer than three pixels with an elevation to triangulate",
            records[0].clone(),
        ));
    }
    let input = TinInput {
        points: elevations.points,
        ..TinInput::default()
    };
    let build = TinBuildOptions {
        duplicate_points: DuplicatePointPolicy::Average,
        ..TinBuildOptions::default()
    };
    let mut made = triangulate(input, &build)?;
    records.append(&mut made.records);
    made.records = records;
    Ok(made)
}

/// `build` kept in the phase's staging store as `name`, which the executor
/// commits to the drawing as one undo step. When `unique` the name is made
/// unique ("name (2)"), else a name in use is AlreadyExists. The records of
/// the build, then the surface's.
pub fn keep_surface(
    context: &mut Context<'_>,
    name: String,
    source: String,
    unique: bool,
    build: SurfaceBuild,
) -> Result<String> {
    let name = if unique {
        context.surfaces.unique_name(name.as_bytes())
    } else {
        name
    };
    // Into the phase's staging copy, which the executor commits as one undo
    // step when the apply ends (geo_verbs.rs).
    context.surfaces.add(NamedSurface {
        name: name.clone(),
        surface: Some(build.surface),
        source,
        ..NamedSurface::default()
    })?;
    let kept = context
        .surfaces
        .find(&name)
        .expect("the surface just kept has its name");
    let mut records = build.records;
    records.push(surface_record(kept, context.document.surface_styles()));
    Ok(joined_records(&records))
}

// ---- the words the terrain verbs read ---------------------------------------------------------

/// `key=value` at `i`, unquoted, the key lower-cased; nothing for any other
/// word.
pub fn key_value(tokens: &Tokens, i: usize) -> Option<(String, String)> {
    if i >= tokens.size() {
        return None;
    }
    let word = &tokens[i];
    let equals = word.find('=')?;
    if equals == 0 || word.starts_with('-') {
        return None;
    }
    Some((
        word[..equals].to_ascii_lowercase(),
        String::from(&word[equals + 1..]),
    ))
}

/// A finite number, or InvalidArgument naming the option: "interval is a
/// number".
pub fn number_option(key: &str, value: &str) -> Result<f64> {
    parse_finite_double(value).ok_or_else(|| {
        make_error(
            ErrorCode::InvalidArgument,
            format!("{key} is a number"),
            value,
        )
    })
}

/// The same, greater than zero: "<key> is greater than zero" (the C++'s
/// `positiveOption(std::string_view, std::string_view)`).
pub fn positive_option(key: &str, value: &str) -> Result<f64> {
    let number = number_option(key, value)?;
    if !(number > 0.0) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{key} is greater than zero"),
            value,
        ));
    }
    Ok(number)
}

/// The records joined a line each, an empty one left out.
pub fn joined_records(records: &[String]) -> String {
    let mut text = String::new();
    for record in records {
        if !record.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text += record;
        }
    }
    text
}

/// A Prepared with nothing to run: the reply is the answer.
pub fn answered_with(title: impl Into<String>, reply: impl Into<String>) -> Prepared {
    Prepared {
        title: title.into(),
        reply: Some(reply.into()),
        work: None,
    }
}

// ---- the raster steps (terrain_steps.cpp) -------------------------------------------------------

fn chain_folder(scratch: &Path) -> PathBuf {
    // One folder per chain, so two jobs of one process never share a file.
    static MADE: AtomicU64 = AtomicU64::new(0);
    let made = MADE.fetch_add(1, Ordering::Relaxed) + 1;
    scratch.join(format!("katana-chain-{made}"))
}

/// A raster handed from one GDAL step to the next - clip, smooth, contour;
/// hillshade, colour, blend; slope, reclassify, polygonize. Each step's raster
/// is written to a tiled GeoTIFF in the chain's own folder, never held whole
/// in memory, its bands, colours and no-data as GDAL wrote them; the folder is
/// removed with the chain, so a failed or cancelled run leaves nothing. Made
/// and used on one thread, the job's: it shares no dataset with any other run.
#[derive(Debug)]
pub struct RasterChain {
    /// GDAL's warnings of every step, in order.
    pub warnings: Vec<Diagnostic>,
    pub seconds: f64,
    current: DatasetValue,
    folder: PathBuf,
    stepped: bool,
}

impl RasterChain {
    /// `input` is the first step's; `scratch` the folder the chain's own is
    /// made in.
    pub fn new(input: DatasetValue, scratch: &Path) -> Self {
        Self {
            warnings: Vec::new(),
            seconds: 0.0,
            current: input,
            folder: chain_folder(scratch),
            stepped: false,
        }
    }

    /// The algorithm at `path` on the current raster (its `input`), `values`
    /// bound before GDAL reads `tokens`; the raster it writes is the current
    /// one after. What the bridge's run refuses, including "cancelled". The
    /// C++'s defaults are no progress and no values.
    pub fn step(
        &mut self,
        path: &[String],
        tokens: Vec<String>,
        stop: Option<&AtomicBool>,
        progress: Progress<'_>,
        mut values: Vec<(String, ArgValue)>,
    ) -> Status {
        let _ = std::fs::create_dir_all(&self.folder);
        values.push((
            String::from("input"),
            ArgValue::Dataset(self.current.clone()),
        ));
        // Every raster to a file of the chain's: a DEM is never held whole in
        // memory, and an RGBA picture keeps its colours, which a grid of
        // doubles would not.
        let request = RunRequest {
            path: path.to_vec(),
            values,
            tokens,
            output_to: OutputTo::Memory,
            max_memory_cells: 0,
            spill_directory: path_text(&self.folder),
            ..RunRequest::default()
        };
        let outputs = gp::run(&request, stop, progress)?;
        self.seconds += outputs.seconds;
        self.warnings.extend(outputs.diagnostics.iter().cloned());
        let Some(file) = outputs.file else {
            return Err(make_error(
                ErrorCode::CommandRejected,
                format!("{} wrote no raster", gp::path_text(path)),
                gp::path_text(path),
            ));
        };
        self.current = DatasetValue::Path(DatasetPath {
            path: file,
            ..DatasetPath::default()
        });
        self.stepped = true;
        Ok(())
    }

    /// The algorithm at `path` on the current raster for the features it
    /// writes (raster contour, raster polygonize); the current raster stays.
    pub fn features(
        &mut self,
        path: &[String],
        tokens: Vec<String>,
        stop: Option<&AtomicBool>,
        progress: Progress<'_>,
    ) -> Result<FeatureSet> {
        let request = RunRequest {
            path: path.to_vec(),
            values: vec![(
                String::from("input"),
                ArgValue::Dataset(self.current.clone()),
            )],
            tokens,
            output_to: OutputTo::Memory,
            ..RunRequest::default()
        };
        let outputs = gp::run(&request, stop, progress)?;
        self.seconds += outputs.seconds;
        self.warnings.extend(outputs.diagnostics.iter().cloned());
        Ok(outputs.features.unwrap_or_default())
    }

    pub fn current(&self) -> &DatasetValue {
        &self.current
    }

    /// The current raster's facts, read from its file or its grid.
    pub fn info(&self) -> Result<RasterInfo> {
        raster_info_of(&self.current)
    }

    /// The file the last step wrote, moved out of the chain's folder to
    /// `destination` (made unique if a file is there): kept when the chain
    /// goes. InvalidState before any step.
    pub fn keep(&mut self, destination: &Path) -> Result<PathBuf> {
        let file = match &self.current {
            DatasetValue::Path(file) if self.stepped => file.path.clone(),
            _ => {
                return Err(Error::new(
                    ErrorCode::InvalidState,
                    "no step of the chain has written a raster",
                ));
            }
        };
        let parent = destination.parent().unwrap_or_else(|| Path::new(""));
        let _ = std::fs::create_dir_all(parent);
        let stem = destination
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let extension = destination
            .extension()
            .map(|extension| format!(".{}", extension.to_string_lossy()))
            .unwrap_or_default();
        let mut target = destination.to_path_buf();
        let mut copy = 2;
        while target.exists() {
            target = parent.join(format!("{stem}-{copy}{extension}"));
            copy += 1;
        }
        let from = path_from_text(&file);
        if std::fs::rename(&from, &target).is_err() {
            // Another volume: copied, and the chain's copy goes with its
            // folder.
            if let Err(error) = std::fs::copy(from.as_path(), &target) {
                return Err(make_error(
                    ErrorCode::FileExportFailure,
                    format!("could not keep the raster: {}", system_message(&error)),
                    path_text(&target),
                ));
            }
        }
        self.current = DatasetValue::Path(DatasetPath {
            path: path_text(&target),
            ..DatasetPath::default()
        });
        Ok(target)
    }

    /// `text` written to a file `name` of the chain's own, removed with it: a
    /// colour map a step reads by name.
    pub fn write(&self, name: &str, text: &str) -> Result<PathBuf> {
        let _ = std::fs::create_dir_all(&self.folder);
        let file = self.folder.join(name);
        if std::fs::write(&file, text).is_err() {
            return Err(make_error(
                ErrorCode::FileExportFailure,
                "could not write the chain's file",
                path_text(&file),
            ));
        }
        Ok(file)
    }
}

impl Drop for RasterChain {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

// ---- facts, sources, areas ----------------------------------------------------------------------

/// A raster dataset's facts: a file's read through GDAL, a grid's as it is.
pub fn raster_info_of(dataset: &DatasetValue) -> Result<RasterInfo> {
    match dataset {
        DatasetValue::Raster(grid) => {
            let mut info = grid.info.clone();
            if let Some(Some(no_data)) = grid.no_data.first() {
                info.no_data_value = Some(*no_data);
            }
            Ok(info)
        }
        DatasetValue::Path(file) => GdalDataset::open(&path_from_text(&file.path))?.raster_info(),
        DatasetValue::Features(_) => Err(Error::new(
            ErrorCode::InvalidArgument,
            "features are no raster",
        )),
    }
}

/// The side of a square, unrotated cell; nothing for any other.
pub fn square_cell_of(info: &RasterInfo) -> Option<f64> {
    let gt = &info.geotransform;
    if !info.has_geotransform || gt[2] != 0.0 || gt[4] != 0.0 || gt[1].abs() != gt[5].abs() {
        return None;
    }
    Some(gt[1].abs())
}

/// The ground a raster covers: the box of its four corners.
pub fn raster_extent(info: &RasterInfo) -> Box2 {
    let gt = &info.geotransform;
    let mut extent = Box2::default();
    for (px, py) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
        let column = px * f64::from(info.width);
        let row = py * f64::from(info.height);
        extent.expand(Point2::new(
            gt[0] + column * gt[1] + row * gt[2],
            gt[3] + column * gt[4] + row * gt[5],
        ));
    }
    extent
}

/// The least and greatest values of band 1, no-data and non-finite cells
/// left out; nothing when no cell has a value. Every cell is read when
/// `max_samples` is 0 (one row at a time from a file); else a stride keeps the
/// read to about that many, and the range is the sample's - within the
/// whole's. The C++'s default is 0.
pub fn value_range(dataset: &DatasetValue, max_samples: usize) -> Result<Option<(f64, f64)>> {
    let mut low = f64::INFINITY;
    let mut high = -low;
    let mut take = |values: &[f64], no_data: Option<f64>| {
        for &value in values {
            if !value.is_finite() || no_data.is_some_and(|no_data| value == no_data) {
                continue;
            }
            low = cpp::min(low, value);
            high = cpp::max(high, value);
        }
    };
    match dataset {
        DatasetValue::Raster(grid) => {
            if let Some(band) = grid.bands.first() {
                take(band.as_slice(), grid.no_data.first().copied().flatten());
            }
        }
        DatasetValue::Path(file) => {
            let opened = GdalDataset::open(&path_from_text(&file.path))?;
            let info = opened.raster_info()?;
            let cells = f64::from(info.width) * f64::from(info.height);
            let stride = if max_samples == 0 {
                1
            } else {
                ((cells / max_samples as f64).sqrt().ceil() as i32).max(1)
            };
            let samples = opened.read_band_sampled(1, stride)?;
            take(samples.values.as_slice(), samples.no_data_value);
        }
        DatasetValue::Features(_) => {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "features are no raster",
            ));
        }
    }
    if !(low <= high) {
        return Ok(None);
    }
    Ok(Some((low, high)))
}

/// The raster an analysis verb reads (CONTOUR, RASTER SHADE, RASTER SLOPE),
/// read by the one source parser: RASTER <id|name> or FILE <path> as FROM
/// binds them, or SURFACE <name> [CELL <m>] - sampled at CELL, or at the cell
/// suggested for its extent (the cell chosen now, so the reply can say it).
#[derive(Clone)]
pub struct TerrainSource {
    pub source: Source,
    /// The dataset GDAL reads, made on the job's thread.
    pub dataset: DeferredDataset,
    /// SURFACE: the surface itself, shared and immutable.
    pub surface: Option<Arc<TinSurface>>,
    /// SURFACE: whether the line gave its CELL.
    pub cell_given: bool,
    /// `input arg=input source=...`: what was read.
    pub record: String,
}

/// Reads the source at `at`, leaving `at` after it. InvalidArgument naming
/// the word for a scope of the drawing (`verb` has no drawing source);
/// NotFound for a raster or surface of no such id or name; Unsupported for a
/// reference raster with no file behind it.
pub fn bind_terrain_source(
    context: &Context<'_>,
    tokens: &Tokens,
    at: &mut usize,
    verb: &str,
) -> Result<TerrainSource> {
    if *at >= tokens.size()
        || (!tokens.is(*at, "RASTER") && !tokens.is(*at, "SURFACE") && !tokens.is(*at, "FILE"))
    {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "{verb} reads a SURFACE <name>, a RASTER <id|name> or a FILE <path>; a scope of \
                 the drawing comes after it"
            ),
            if *at < tokens.size() {
                tokens[*at].clone()
            } else {
                String::new()
            },
        ));
    }
    let mut source = parse_source(tokens, at)?;
    let mut surface = None;
    let mut cell_given = false;
    if source.kind == SourceKind::Surface {
        let Some(named) = context.surfaces.find(&source.surface) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "no surface has that name; SURFACE LIST names them",
                source.surface.clone(),
            ));
        };
        let tin = Arc::clone(
            named
                .surface
                .as_ref()
                .expect("a drawing's surface has a triangulation"),
        );
        cell_given = source.cell.is_some();
        if source.cell.is_none() {
            // Chosen now, so the reply says the cell the surface was read at.
            source.cell = Some(seams::suggested_cell_size(tin.bounds())?);
        }
        surface = Some(tin);
    }
    let dataset = bind_raster(context, &source)?;
    let record = input_record("input", &source);
    Ok(TerrainSource {
        source,
        dataset,
        surface,
        cell_given,
        record,
    })
}

/// The closed shapes a scope took, as the areas an analysis is kept inside
/// (CONTOUR's boundaries, RASTER SLOPE's clip): closed polylines and circles
/// (drawing_dataset's polygons, a tagged hole joining its area). Open lines
/// and points are no area: counted and said, never an error, as a scope that
/// takes nothing is not.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScopeAreas {
    pub areas: Vec<ContourBoundary>,
    /// Of every area, empty for none.
    pub r#box: Box2,
    /// `scope arg=<arg> ...`, then `areas used=<n> skipped.open=<n>
    /// skipped.points=<n>`, then a warning for what was left out.
    pub records: Vec<String>,
}

pub fn bind_areas(context: &Context<'_>, scope: &ScopeWords, arg: &str) -> Result<ScopeAreas> {
    let options = DrawingDatasetOptions {
        crs_wkt: project_crs(context),
        ..DrawingDatasetOptions::default()
    };
    let bound = bind_drawing(context, scope, &options)?;
    let mut areas = ScopeAreas {
        areas: boundaries_of(&bound.dataset.set),
        ..ScopeAreas::default()
    };
    for area in &areas.areas {
        for ring in &area.rings {
            for vertex in &ring.vertices {
                areas.r#box.expand(*vertex);
            }
        }
    }
    let stats = &bound.dataset.stats;
    areas.records.push(scope_record(arg, &bound));
    areas.records.push(format!(
        "areas used={} skipped.open={} skipped.points={}",
        areas.areas.len(),
        stats.lines,
        stats.points
    ));
    if stats.lines + stats.points != 0 {
        areas.records.push(warning_record(&format!(
            "{} of what the scope took are not closed shapes, so bound no area, and were left out",
            stats.lines + stats.points
        )));
    }
    for warning in &stats.warnings {
        areas.records.push(warning_record(warning));
    }
    Ok(areas)
}

/// A ring as WKT's coordinate list, closed as WKT closes it.
fn ring_wkt(ring: &Polyline2) -> String {
    let mut text = String::from("(");
    for vertex in &ring.vertices {
        text += &format!("{} {},", gdal_number(vertex.x), gdal_number(vertex.y));
    }
    let first = ring.vertices[0];
    text + &format!("{} {})", gdal_number(first.x), gdal_number(first.y))
}

/// The areas as one WKT MULTIPOLYGON, for GDAL's --geometry.
pub fn areas_wkt(areas: &[ContourBoundary]) -> String {
    let mut text = String::from("MULTIPOLYGON(");
    for (a, area) in areas.iter().enumerate() {
        text += if a == 0 { "(" } else { ",(" };
        for (r, ring) in area.rings.iter().enumerate() {
            if r != 0 {
                text.push(',');
            }
            text += &ring_wkt(ring);
        }
        text.push(')');
    }
    text + ")"
}

/// What a derived raster is named after: a surface's name, a file's stem, a
/// reference raster's name ("terrain" -> "terrain-slope", "terrain-viewshed").
pub fn source_name(context: &Context<'_>, source: &Source) -> String {
    match source.kind {
        SourceKind::Surface => source.surface.clone(),
        SourceKind::File => path_from_text(&source.path)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default(),
        SourceKind::Raster => seams::source_raster_name(context, &source.raster),
        SourceKind::Drawing => String::from("raster"),
    }
}

/// A number as GDAL's command line takes it, exactly: "0.5", "-12.25".
pub fn gdal_number(value: f64) -> String {
    format_exact_real(value)
}
