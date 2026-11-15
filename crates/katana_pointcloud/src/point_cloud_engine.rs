//! LAS / LAZ point-cloud access through PDAL, behind a Katana interface (Rule
//! 4). The port of include/katana/pointcloud/point_cloud_engine.hpp and
//! src/katana_io/point_cloud_engine.cpp. No PDAL type appears here: PDAL is
//! reached through native/pdal_shim.cpp (crate::ffi), which only does what
//! this module tells it - every decision of the C++ engine is made here.
//!
//! PLAN.MD Phase 17 requires this to scale to datasets that do not fit in
//! memory. What is implemented is the decimating, filtering, bounded read
//! that makes a large file usable for display and analysis: the caller
//! states a point budget and PDAL discards the rest during the pipeline
//! rather than after it. A full out-of-core spatial hierarchy with level of
//! detail is NOT implemented, and read_header() exists so a caller can size
//! its budget before committing to a read.
//!
//! No lock is taken around PDAL. PDAL guards its process-wide registries
//! itself (the plugin manager's and the stage extensions' mutexes; the stage
//! factory documents stage creation as thread-safe), every call here builds
//! and owns its own stages, and the C++ engine is run from the window's job
//! threads without one too.

use std::path::Path;

use katana_core::error::make_error;
use katana_core::path_text::{path_exists, path_to_utf8};
use katana_core::text::{format_exact_real, format_general, lowered};
use katana_core::text_encoding::utf8_or_windows1252;
use katana_core::{Error, ErrorCode, Result, Status};

use crate::ffi::{self, KatanaPdalPoint, PointFields, Stage, StageOption, StageRole, Thrown};
use crate::proj_search_paths::point_gdal_at_proj_data;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointCloudPoint {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub intensity: f64,
    pub classification: u8,
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub has_color: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointCloudBounds {
    pub min_x: f64,
    pub min_y: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

/// Inverted infinities, matching geometry's Box2: a default bounds is EMPTY
/// and can be grown by comparison. Defaulting to zeros would instead describe
/// a degenerate box at the origin, which is not empty by the test below - so
/// an empty cloud would drag Zoom Extents out to 0,0.
impl Default for PointCloudBounds {
    fn default() -> Self {
        Self {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            min_z: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
            max_z: f64::NEG_INFINITY,
        }
    }
}

impl PointCloudBounds {
    pub fn empty(&self) -> bool {
        self.min_x > self.max_x || self.min_y > self.max_y || self.min_z > self.max_z
    }
}

/// What a file declares before any points are read. Cheap: the header only.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointCloudHeader {
    pub point_count: u64,
    pub bounds: PointCloudBounds,
    pub projection_wkt: String,
    pub has_color: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointCloud {
    pub points: Vec<PointCloudPoint>,
    pub bounds: PointCloudBounds,
    pub projection_wkt: String,
    /// Points the file held before decimation and filtering, so a caller can
    /// say "showing 2 000 000 of 418 000 000" rather than implying it has them
    /// all.
    pub source_point_count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointCloudReadOptions {
    pub classification: Option<u8>,
    pub clip: Option<PointCloudBounds>,
    /// Keep one point in every `decimation_step`. Must be >= 1.
    pub decimation_step: u32,
    /// Hard ceiling on returned points; 0 means no ceiling. Applied after
    /// decimation, as a last resort - prefer a decimation step, which samples
    /// the whole extent instead of truncating it to whatever PDAL emits first.
    pub max_points: u64,
    /// COPC only: the coarsest point spacing acceptable, in the cloud's units.
    /// A Cloud Optimised Point Cloud is a LAZ whose chunks are already an
    /// octree, and readers.copc returns only the levels whose spacing is at or
    /// above this, so level of detail is a query parameter rather than a
    /// structure Katana owns (PLAN.MD Phase 17). Refused with InvalidArgument
    /// on a file that is not COPC, rather than silently returning everything.
    pub resolution: Option<f64>,
}

impl Default for PointCloudReadOptions {
    fn default() -> Self {
        Self {
            classification: None,
            clip: None,
            decimation_step: 1,
            max_points: 0,
            resolution: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointCloudEngine;

// ---- what the C++ engine's anonymous namespace held -----------------------------------------

/// What a guarded body gives: Err for an exception PDAL threw (caught in the
/// shim), Ok with the body's own answer otherwise.
type Guarded<T> = std::result::Result<Result<T>, Thrown>;

// PDAL reports failures by throwing. The rest of Katana reports them with
// Result (PLAN.MD section 36), so every entry point into PDAL is wrapped here
// and nothing throws across the interface: the shim catches the exception,
// and its what() becomes the context of the error `code` / `what` names.
fn guarded<T>(code: ErrorCode, what: &str, body: impl FnOnce() -> Guarded<T>) -> Result<T> {
    point_gdal_at_proj_data();
    match body() {
        Ok(answer) => answer,
        Err(Thrown(text)) => Err(make_error(code, what, text)),
    }
}

// A path as the C++ passes it to PDAL (path::string()): the path's own bytes
// on POSIX, UTF-8 on Windows.
fn native_text(path: &Path) -> Vec<u8> {
    path_to_utf8(path)
}

// A path as an error's context names it.
fn path_text(path: &Path) -> String {
    utf8_or_windows1252(path_to_utf8(path))
}

// std::filesystem::exists(path, error): false when it cannot tell.
fn exists(path: &Path) -> bool {
    path_exists(path).unwrap_or(false)
}

// path::filename(): what follows the last separator, empty for a path that
// ends in one. Path::file_name would pass over a trailing separator and
// answer the last component instead.
fn file_name_bytes(path: &Path) -> Vec<u8> {
    let text = path_to_utf8(path);
    let start = text
        .iter()
        .rposition(|&byte| byte == b'/' || (cfg!(windows) && byte == b'\\'))
        .map_or(0, |separator| separator + 1);
    text[start..].to_vec()
}

fn filename_option(path: &Path) -> StageOption {
    ("filename", native_text(path))
}

// The driver PDAL infers, outside any guard: the C++ calls inferDriver
// unguarded in isCopc, convertToCopc and write, so an exception PDAL threw
// there would leave the engine uncaught (a defect, docs/rust.md "Not done").
// Ported as it is: such an exception is a panic carrying its what().
fn infer_unguarded(path: &Path, for_writing: bool) -> String {
    match ffi::infer_driver(&native_text(path), for_writing) {
        Ok(driver) => driver,
        Err(Thrown(text)) => panic!("PDAL threw while inferring a driver: {text}"),
    }
}

// ASPRS LAS 1.4 R15 stores X, Y and Z as 32-bit integers, each multiplied by
// the public header's scale factor and added to its offset, so the scale IS
// the resolution of every coordinate in the file. PDAL's writers default it
// to 0.01 - a centimetre - which quantised every point written here by up to
// 5 mm (audit IO-17). 0.001 is the millimetre survey coordinates are quoted
// to.
//
// With that resolution an int32 spans +/-2 147 km, less than a projected
// northing of 7 410 km, so the offset must move the origin to the data:
// "auto" has PDAL take it from the points' minimum.
const LAS_SCALE: &str = "0.001";
const LAS_OFFSET: &str = "auto";

fn add_millimetre_scale(options: &mut Vec<StageOption>) {
    for (scale, offset) in [
        ("scale_x", "offset_x"),
        ("scale_y", "offset_y"),
        ("scale_z", "offset_z"),
    ] {
        options.push((scale, Vec::from(LAS_SCALE)));
        options.push((offset, Vec::from(LAS_OFFSET)));
    }
}

// A double as PDAL's Options::add writes it (Utils::toString(value, 15)):
// NaN and the infinities by their JSON property names, anything else as a
// stream with precision 15 writes it.
fn option_real(value: f64) -> String {
    if value.is_nan() {
        return String::from("NaN");
    }
    if value.is_infinite() {
        return String::from(if value < 0.0 { "-Infinity" } else { "Infinity" });
    }
    format_general(value, 15)
}

// std::min and std::max as the C++ library answers them (the second only
// when it is strictly less, or the first strictly less than it): the rule of
// katana_math::cpp, which this layer may not see (it sees core only).
fn cpp_min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

fn cpp_max(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

fn grow_bounds(bounds: &mut PointCloudBounds, x: f64, y: f64, z: f64) {
    bounds.min_x = cpp_min(bounds.min_x, x);
    bounds.min_y = cpp_min(bounds.min_y, y);
    bounds.min_z = cpp_min(bounds.min_z, z);
    bounds.max_x = cpp_max(bounds.max_x, x);
    bounds.max_y = cpp_max(bounds.max_y, y);
    bounds.max_z = cpp_max(bounds.max_z, z);
}

fn inverted_bounds() -> PointCloudBounds {
    PointCloudBounds {
        min_x: f64::MAX,
        min_y: f64::MAX,
        min_z: f64::MAX,
        max_x: f64::MIN,
        max_y: f64::MIN,
        max_z: f64::MIN,
    }
}

// LAS stores colour as 16-bit; scale to 8-bit for display.
fn eight_bits(value: u16) -> u8 {
    (if value > 255 { value >> 8 } else { value }) as u8
}

// How many points the shim is asked for at a time: the C++ engine reads a
// point at a time from the view; the batch only bounds the buffer.
const READ_BATCH: u64 = 65_536;

fn no_reader(path: &Path) -> Error {
    make_error(
        ErrorCode::Unsupported,
        "no PDAL reader is registered for this extension",
        path_text(path),
    )
}

impl PointCloudEngine {
    pub fn read_header(&self, path: &Path) -> Result<PointCloudHeader> {
        if !exists(path) {
            return Err(make_error(
                ErrorCode::NotFound,
                "file does not exist",
                path_text(path),
            ));
        }
        guarded(
            ErrorCode::FileImportFailure,
            "PDAL could not read the point-cloud header",
            || {
                let driver = ffi::infer_driver(&native_text(path), false)?;
                if driver.is_empty() {
                    return Ok(Err(no_reader(path)));
                }
                // preview() reads the header only, so this stays cheap no
                // matter how large the file is - which is what lets the
                // importer size a decimation step before committing to a read.
                let Some(info) = ffi::preview(&driver, &[filename_option(path)], true)? else {
                    return Ok(Err(make_error(
                        ErrorCode::Unsupported,
                        "PDAL could not create reader",
                        driver,
                    )));
                };
                let bounds = match info.bounds {
                    Some([min_x, min_y, min_z, max_x, max_y, max_z]) => PointCloudBounds {
                        min_x,
                        min_y,
                        min_z,
                        max_x,
                        max_y,
                        max_z,
                    },
                    None => PointCloudBounds::default(),
                };
                Ok(Ok(PointCloudHeader {
                    point_count: info.point_count,
                    bounds,
                    projection_wkt: info.srs_wkt,
                    has_color: info.has_red,
                }))
            },
        )
    }

    pub fn read(&self, path: &Path, options: &PointCloudReadOptions) -> Result<PointCloud> {
        if options.decimation_step == 0 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "decimation step must be at least 1",
            ));
        }
        if !exists(path) {
            return Err(make_error(
                ErrorCode::NotFound,
                "file does not exist",
                path_text(path),
            ));
        }

        guarded(
            ErrorCode::FileImportFailure,
            "PDAL could not read the point cloud",
            || {
                let driver = ffi::infer_driver(&native_text(path), false)?;
                if driver.is_empty() {
                    return Ok(Err(no_reader(path)));
                }

                let mut reader_options = Vec::new();
                if let Some(resolution) = options.resolution {
                    // Only readers.copc understands a resolution. Any other
                    // reader would ignore the option and return the whole
                    // file, which is exactly the silent failure a caller
                    // asking for a coarse level of detail cannot afford
                    // (PLAN.MD section 36).
                    if driver != "readers.copc" {
                        return Ok(Err(make_error(
                            ErrorCode::InvalidArgument,
                            "a resolution can only be asked of a COPC file; convert it with \
                             convertToCopc first",
                            path_text(path),
                        )));
                    }
                    if !(resolution > 0.0) {
                        return Ok(Err(Error::new(
                            ErrorCode::InvalidArgument,
                            "the resolution must be positive",
                        )));
                    }
                    reader_options.push(("resolution", option_real(resolution).into_bytes()));
                }
                let mut stages = vec![Stage {
                    role: StageRole::Reader,
                    driver,
                    filename: native_text(path),
                    options: reader_options,
                }];
                let filter = |driver: &str, option: StageOption| Stage {
                    role: StageRole::Filter,
                    driver: String::from(driver),
                    filename: Vec::new(),
                    options: vec![option],
                };

                if let Some(clip) = &options.clip {
                    let bounds = format!(
                        "([{},{}],[{},{}],[{},{}])",
                        format_exact_real(clip.min_x),
                        format_exact_real(clip.max_x),
                        format_exact_real(clip.min_y),
                        format_exact_real(clip.max_y),
                        format_exact_real(clip.min_z),
                        format_exact_real(clip.max_z)
                    );
                    stages.push(filter("filters.crop", ("bounds", bounds.into_bytes())));
                }
                if let Some(classification) = options.classification {
                    let limits = format!("Classification[{classification}:{classification}]");
                    stages.push(filter("filters.range", ("limits", limits.into_bytes())));
                }
                if options.decimation_step > 1 {
                    let step = format!("{}", options.decimation_step);
                    stages.push(filter("filters.decimation", ("step", step.into_bytes())));
                }
                // The last stage is the tail of the pipeline; the pipeline
                // executes the stage graph it has accumulated, so the tail is
                // what gets run. It lives until the end of this body, as the
                // C++'s PipelineManager does, past the header read below.
                let pipeline = ffi::run_pipeline(&stages)?;

                let mut cloud = PointCloud {
                    bounds: inverted_bounds(),
                    ..PointCloud::default()
                };
                let mut batch = Vec::new();
                for view_index in 0..pipeline.view_count() {
                    let view = pipeline.view_info(view_index)?;
                    if view.is_null {
                        continue;
                    }
                    let has_color = view.has_red && view.has_green && view.has_blue;
                    let fields = PointFields {
                        intensity: view.has_intensity,
                        classification: view.has_classification,
                        colour: has_color,
                    };
                    // The C++ checks the ceiling before each point; the points
                    // it would read before stopping are these.
                    let room = if options.max_points != 0 {
                        options.max_points.saturating_sub(cloud.points.len() as u64)
                    } else {
                        view.size
                    };
                    let take = view.size.min(room);
                    let mut first = 0;
                    while first < take {
                        let count = (take - first).min(READ_BATCH);
                        batch.clear();
                        batch.resize(count as usize, KatanaPdalPoint::default());
                        pipeline.view_points(view_index, first, fields, &mut batch)?;
                        for raw in &batch {
                            let point = PointCloudPoint {
                                x: raw.x,
                                y: raw.y,
                                z: raw.z,
                                intensity: if fields.intensity { raw.intensity } else { 0.0 },
                                classification: if fields.classification {
                                    raw.classification
                                } else {
                                    0
                                },
                                red: if has_color { eight_bits(raw.red) } else { 0 },
                                green: if has_color { eight_bits(raw.green) } else { 0 },
                                blue: if has_color { eight_bits(raw.blue) } else { 0 },
                                has_color,
                            };
                            grow_bounds(&mut cloud.bounds, point.x, point.y, point.z);
                            cloud.points.push(point);
                        }
                        first += count;
                    }
                    if take < view.size {
                        // Truncated: the ceiling stopped this view, and the
                        // read ends here.
                        break;
                    }
                    if cloud.projection_wkt.is_empty() {
                        cloud.projection_wkt = pipeline.view_srs(view_index)?;
                    }
                }

                if cloud.points.is_empty() {
                    cloud.bounds = PointCloudBounds::default();
                }

                // Report what the FILE holds, not what survived filtering, so
                // the caller can say "2 000 000 of 418 000 000" honestly.
                let header = self.read_header(path);
                cloud.source_point_count = match &header {
                    Ok(header) => header.point_count,
                    Err(_) => cloud.points.len() as u64,
                };
                if cloud.projection_wkt.is_empty()
                    && let Ok(header) = &header
                {
                    cloud.projection_wkt = header.projection_wkt.clone();
                }
                drop(pipeline);
                Ok(Ok(cloud))
            },
        )
    }

    /// True when PDAL reads `path` as a Cloud Optimised Point Cloud - a
    /// .copc.laz whose header carries the COPC record - and so can answer a
    /// resolution query. False for any other readable file. NotFound when the
    /// file does not exist.
    pub fn is_copc(&self, path: &Path) -> Result<bool> {
        if !exists(path) {
            return Err(make_error(
                ErrorCode::NotFound,
                "file does not exist",
                path_text(path),
            ));
        }
        if infer_unguarded(path, false) != "readers.copc" {
            return Ok(false);
        }
        // A .copc.laz that is not actually COPC makes readers.copc throw on
        // its header. "Will PDAL read it as COPC" is the question, so that is
        // false, not an error.
        let probe = guarded(ErrorCode::FileImportFailure, "not COPC", || {
            if ffi::preview("readers.copc", &[filename_option(path)], false)?.is_none() {
                return Ok(Err(Error::new(
                    ErrorCode::Unsupported,
                    "PDAL could not create readers.copc",
                )));
            }
            Ok(Ok(()))
        });
        Ok(probe.is_ok())
    }

    /// Writes `source` - any format PDAL reads - as COPC at `destination`,
    /// which must end in .copc.laz so that every later read infers
    /// readers.copc. Done once, on import, so that every later view of the
    /// cloud can ask for just the resolution it needs. Fails with
    /// InvalidArgument for the wrong extension, NotFound for a missing
    /// source, Unsupported for an unreadable format, and FileExportFailure
    /// from PDAL.
    pub fn convert_to_copc(&self, source: &Path, destination: &Path) -> Status {
        if !exists(source) {
            return Err(make_error(
                ErrorCode::NotFound,
                "file does not exist",
                path_text(source),
            ));
        }
        // ASCII folding, whatever the process locale (core's text module).
        let name = lowered(file_name_bytes(destination));
        if !name.ends_with(b".copc.laz") {
            // The extension is what makes every later read infer
            // readers.copc; a COPC file called .laz would be read as plain
            // LAZ and could never answer a resolution query.
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a COPC file must be named .copc.laz",
                path_text(destination),
            ));
        }
        let driver = infer_unguarded(source, false);
        if driver.is_empty() {
            return Err(no_reader(source));
        }
        guarded(
            ErrorCode::FileExportFailure,
            "PDAL could not write the COPC file",
            || {
                let mut writer_options = vec![filename_option(destination)];
                // The conversion must not coarsen the data it indexes (audit
                // IO-17): writers.copc, like writers.las, defaults to a 0.01
                // scale. From a LAS source the source's own scale and offset
                // are forwarded - a 0.1 mm scan stays 0.1 mm - with its header
                // fields and VLRs. Any other format has no LAS header to
                // forward, so it gets the millimetre, stated explicitly. Never
                // both: an explicit scale OVERRIDES a forwarded one (measured
                // with PDAL 2.10.2: a 0.0001-scale LAS converted with
                // forward=all kept 0.0001, and with forward=all plus
                // scale_x=0.001 came out at 0.001).
                let las_source = driver == "readers.las" || driver == "readers.copc";
                if las_source {
                    writer_options.push(("forward", Vec::from("all")));
                } else {
                    add_millimetre_scale(&mut writer_options);
                }
                let stages = [
                    Stage {
                        role: StageRole::Reader,
                        driver: driver.clone(),
                        filename: native_text(source),
                        options: Vec::new(),
                    },
                    Stage {
                        role: StageRole::Writer,
                        driver: String::from("writers.copc"),
                        filename: native_text(destination),
                        options: writer_options,
                    },
                ];
                ffi::run_pipeline(&stages)?;
                Ok(Ok(()))
            },
        )
    }

    pub fn write(&self, path: &Path, cloud: &PointCloud) -> Status {
        if cloud.points.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "refusing to write an empty point cloud",
                path_text(path),
            ));
        }

        let driver = infer_unguarded(path, true);
        if driver.is_empty() {
            return Err(make_error(
                ErrorCode::Unsupported,
                "no PDAL writer is registered for this extension",
                path_text(path),
            ));
        }

        guarded(
            ErrorCode::FileExportFailure,
            "PDAL could not write the point cloud",
            || {
                // A PointView built in memory and handed straight to the
                // writer. The previous implementation staged the points
                // through a temporary CSV file, which lost precision in
                // formatting, left the temporary behind on some failure paths,
                // and could not represent colour.
                let write_color = cloud.points.iter().any(|point| point.has_color);
                let points: Vec<KatanaPdalPoint> = cloud
                    .points
                    .iter()
                    .map(|point| KatanaPdalPoint {
                        x: point.x,
                        y: point.y,
                        z: point.z,
                        intensity: point.intensity,
                        classification: point.classification,
                        // Back to the 16-bit range LAS stores.
                        red: if write_color {
                            u16::from(point.red) << 8
                        } else {
                            0
                        },
                        green: if write_color {
                            u16::from(point.green) << 8
                        } else {
                            0
                        },
                        blue: if write_color {
                            u16::from(point.blue) << 8
                        } else {
                            0
                        },
                    })
                    .collect();

                let mut writer_options = vec![filename_option(path)];
                // Only the LAS family has a scale and an offset; any other
                // writer would refuse the options as unknown. writers.las
                // compresses a path ending .laz by its name alone (PDAL
                // 2.10.2, although its `compression` option defaults to
                // false), which ALazIsCompressedAndReadsBackAsTheSamePoints
                // holds it to.
                if driver == "writers.las" || driver == "writers.copc" {
                    add_millimetre_scale(&mut writer_options);
                }
                // PointView::setSpatialReference is private, so the CRS is
                // declared to the writer instead - which is also where it
                // belongs, since it is a property of the file being produced.
                if !cloud.projection_wkt.is_empty() {
                    writer_options.push(("a_srs", cloud.projection_wkt.clone().into_bytes()));
                }
                if !ffi::write_points(&driver, &writer_options, &points, write_color)? {
                    return Ok(Err(make_error(
                        ErrorCode::Unsupported,
                        "PDAL could not create writer",
                        driver.clone(),
                    )));
                }
                Ok(Ok(()))
            },
        )
    }

    /// A decimation step that brings `source_count` points under `budget`,
    /// never less than 1. Exposed because the GUI needs it to size a read
    /// before it knows anything else about the file.
    pub fn decimation_for_budget(source_count: u64, budget: u64) -> u32 {
        if budget == 0 || source_count <= budget {
            return 1;
        }
        // Round up, so the result is genuinely at or under the budget rather
        // than just over it. In the C++'s unsigned arithmetic, which wraps for
        // a count within a budget of 2^64.
        let step = source_count.wrapping_add(budget).wrapping_sub(1) / budget;
        step.min(u64::from(u32::MAX)) as u32
    }
}

pub fn pdal_version() -> String {
    ffi::version()
}
