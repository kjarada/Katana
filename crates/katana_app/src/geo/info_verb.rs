//! INFO (docs/interop.md, "Dataset information"), the port of
//! src/katana_app/geo/info_verb.cpp: what a file, a folder or a URL holds,
//! read without importing it, as records - or as GDAL's own JSON.
//!
//! ```text
//! INFO <file|folder|url> [JSON] [STATS] [CHECK] [LAYER <name>]
//! ```
//!
//! A dataset is interop's describe_source: GDAL's own `raster info` and
//! `vector info` through the bridge, a point cloud PDAL's. The records are
//! read from the very JSON that INFO ... JSON gives, so the two cannot
//! disagree. STATS computes every band's statistics without writing an
//! .aux.xml beside the file; CHECK reads everything with GDAL's `dataset
//! check` and says what failed; a folder is GDAL's `dataset identify`,
//! recursive and detailed, beside the point clouds in it, which GDAL does not
//! read. The read is the work, so the window reads a large file on a worker.
//!
//! INFO <id> stays the interpreter's - an entity - unless a file of that name
//! exists; INFO #<id> is always the entity ([`takes_info`]).

use std::path::{Path, PathBuf};

use katana_cad::CommandInterpreter;
use katana_core::error::make_error;
use katana_core::json::OrderedJson;
use katana_core::{Error, ErrorCode, Result};
use katana_geometry::{Box2, Point2};
use katana_gis::processing::{self as gp, ArgValue, DatasetPath, DatasetValue, RunRequest};
use katana_interop::dataset_info::{DescribeOptions, describe_source, is_virtual_path};
use katana_interop::import::kind_for_path;
use katana_interop::{SourceDescription, SourceKind};

use super::bindings::Tokens;
use super::geo_verbs::{Context, Prepared, make_apply, make_work, stop_requested};
use super::replies::{joined, value, yes_no};
use crate::import_records::{bounds_text, path_from_text, path_text, record_number, warning_text};

/// GDAL's JSON keeps its own order, and INFO's wrapper the order it is
/// written in: an ordered object, not a sorted one.
type Json = OrderedJson;

const USAGE: &str =
    "usage: INFO <file|folder|url> [JSON] [STATS] [CHECK] [LAYER <name>] | INFO <id>";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct InfoRequest {
    path: PathBuf,
    json: bool,
    stats: bool,
    check: bool,
    layer: String,
}

fn keyword(tokens: &Tokens, i: usize) -> bool {
    tokens.is(i, "JSON") || tokens.is(i, "STATS") || tokens.is(i, "CHECK") || tokens.is(i, "LAYER")
}

/// The path is one quoted word, or - as INFO has always read it - the
/// unquoted words up to the first keyword, with a blank between each.
fn parse(tokens: &Tokens) -> Result<InfoRequest> {
    let mut request = InfoRequest::default();
    let mut at = 1;
    let mut path = String::new();
    if at < tokens.size() && tokens.quoted[at] {
        path = tokens[at].clone();
        at += 1;
    } else {
        while at < tokens.size() && !keyword(tokens, at) {
            if !path.is_empty() {
                path.push(' ');
            }
            path += &tokens[at];
            at += 1;
        }
    }
    if path.is_empty() {
        return Err(Error::new(ErrorCode::InvalidArgument, USAGE));
    }
    request.path = path_from_text(&path);
    while at < tokens.size() {
        if tokens.is(at, "JSON") {
            request.json = true;
        } else if tokens.is(at, "STATS") {
            request.stats = true;
        } else if tokens.is(at, "CHECK") {
            request.check = true;
        } else if tokens.is(at, "LAYER") {
            if at + 1 >= tokens.size() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "LAYER names a layer of the file",
                ));
            }
            at += 1;
            request.layer = tokens[at].clone();
        } else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "after the path INFO takes JSON, STATS, CHECK and LAYER <name>",
                tokens[at].clone(),
            ));
        }
        at += 1;
    }
    Ok(request)
}

/// A number as a record gives it; nothing for none.
fn number(found: Option<f64>) -> String {
    found.map(record_number).unwrap_or_default()
}

// ---- a dataset -----------------------------------------------------------------------------

fn dataset_kind(description: &SourceDescription) -> &'static str {
    if description.point_cloud.is_some() {
        return "pointcloud";
    }
    let raster = description.raster.is_some();
    let vector = !description.vector_layers.is_empty() || !description.vector_json.is_empty();
    if raster && vector {
        "raster,vector"
    } else if raster {
        "raster"
    } else if vector {
        "vector"
    } else {
        "none"
    }
}

/// A count a record gives only when the format sets one.
fn when_set(count: i32) -> String {
    if count > 0 {
        format!("{count}")
    } else {
        String::new()
    }
}

fn dataset_records(description: &SourceDescription, request: &InfoRequest) -> Vec<String> {
    let mut records = vec![format!(
        "dataset file={} kind={} driver={} crs={}",
        value(&path_text(&request.path)),
        dataset_kind(description),
        value(&description.driver),
        value(&description.crs)
    )];
    if let Some(raster) = &description.raster {
        let cell = if raster.georeferenced {
            format!(
                "{},{}",
                record_number(raster.pixel_width),
                record_number(raster.pixel_height)
            )
        } else {
            String::new()
        };
        records.push(format!(
            "raster width={} height={} bands={} georeferenced={} cell={cell} bounds={}",
            raster.width,
            raster.height,
            raster.band_count,
            yes_no(raster.georeferenced),
            bounds_text(&raster.bounds)
        ));
        for band in &raster.bands {
            records.push(format!(
                "band band={} type={} nodata={} min={} max={} mean={} stddev={} color={} \
                 overviews={}",
                band.band,
                value(&band.data_type),
                number(band.no_data),
                number(band.min),
                number(band.max),
                number(band.mean),
                number(band.std_dev),
                value(&band.colour),
                band.overviews.len()
            ));
        }
        for band in &raster.bands {
            for (k, (width, height)) in band.overviews.iter().enumerate() {
                records.push(format!(
                    "overview band={} index={} width={width} height={height}",
                    band.band,
                    k + 1
                ));
            }
        }
    }
    for (k, subdataset) in description.subdatasets.iter().enumerate() {
        records.push(format!(
            "subdataset index={} name={} description={}",
            k + 1,
            value(&subdataset.name),
            value(&subdataset.description)
        ));
    }
    for layer in &description.vector_layers {
        records.push(format!(
            "layer name={} features={} geometry={} crs={} bounds={} fields={}",
            value(&layer.name),
            layer.feature_count,
            value(&layer.geometry_type),
            value(&layer.crs),
            bounds_text(&layer.extent),
            layer.fields.len()
        ));
    }
    for layer in &description.vector_layers {
        for field in &layer.fields {
            records.push(format!(
                "field layer={} name={} type={} subtype={} width={} precision={}",
                value(&layer.name),
                value(&field.name),
                value(&field.r#type),
                value(&field.subtype),
                when_set(field.width),
                when_set(field.precision)
            ));
        }
    }
    if let Some(cloud) = &description.point_cloud {
        let mut plan = Box2::default();
        let mut zmin = String::new();
        let mut zmax = String::new();
        if !cloud.bounds.empty() {
            plan.expand(Point2::new(cloud.bounds.min_x, cloud.bounds.min_y));
            plan.expand(Point2::new(cloud.bounds.max_x, cloud.bounds.max_y));
            zmin = record_number(cloud.bounds.min_z);
            zmax = record_number(cloud.bounds.max_z);
        }
        records.push(format!(
            "pointcloud points={} bounds={} zmin={zmin} zmax={zmax} color={} copc={}",
            cloud.point_count,
            bounds_text(&plan),
            yes_no(cloud.has_color),
            yes_no(cloud.copc)
        ));
    }
    if request.stats && description.raster.is_none() {
        records.push(warning_text("STATS is a raster's: this file has no bands"));
    }
    records
}

fn parsed_or_null(text: &str) -> Json {
    if text.is_empty() {
        return Json::Null;
    }
    // GDAL's own words, verbatim; text GDAL wrote that is not JSON is kept
    // as text rather than lost.
    Json::parse(text).unwrap_or_else(|_| Json::from(text))
}

/// A point cloud's description as INFO ... JSON gives it; `description` is
/// one describe_source made of a point cloud.
fn point_cloud_json(description: &SourceDescription) -> Json {
    let cloud = description
        .point_cloud
        .as_ref()
        .expect("a point cloud's description describes its points");
    let bounds = if cloud.bounds.empty() {
        Json::Null
    } else {
        let b = &cloud.bounds;
        Json::array_of(
            [b.min_x, b.min_y, b.min_z, b.max_x, b.max_y, b.max_z]
                .into_iter()
                .map(Json::from),
        )
    };
    Json::object_of([
        ("driver", Json::from(&description.driver)),
        ("points", Json::from(cloud.point_count)),
        ("bounds", bounds),
        ("color", Json::from(cloud.has_color)),
        ("copc", Json::from(cloud.copc)),
        ("crs", Json::from(&description.crs)),
    ])
}

// ---- CHECK -----------------------------------------------------------------------------------

struct Checked {
    code: i32,
    problems: Vec<String>,
}

/// The dataset GDAL reads at `path`, bound as an algorithm's input.
fn input_of(path: &Path) -> (String, ArgValue) {
    (
        String::from("input"),
        ArgValue::Dataset(DatasetValue::Path(DatasetPath {
            path: path_text(path),
            ..DatasetPath::default()
        })),
    )
}

/// GDAL's `dataset check`: every value read, and what failed. A file GDAL
/// cannot open at all is a failure of the line, not a problem found.
fn check(path: &Path) -> Result<Checked> {
    let request = RunRequest {
        path: vec![String::from("dataset"), String::from("check")],
        values: vec![input_of(path)],
        ..RunRequest::default()
    };
    let outputs = gp::run(&request, None, None)?;
    Ok(Checked {
        code: outputs.return_code.unwrap_or(0),
        problems: outputs
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.clone())
            .collect(),
    })
}

// ---- a folder ------------------------------------------------------------------------------

struct Folder {
    /// GDAL's JSON.
    identified: String,
    clouds: Vec<SourceDescription>,
}

/// The point clouds under `folder`, each described: depth first in the order
/// the file system lists them, into a folder as it is met but not through a
/// link to one, as std::filesystem::recursive_directory_iterator walks. False
/// when a folder could not be read, where the C++'s walk stops.
fn walk(folder: &Path, clouds: &mut Vec<SourceDescription>) -> bool {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return false;
    };
    for entry in entries {
        let Ok(entry) = entry else {
            return false;
        };
        let path = entry.path();
        // is_regular_file follows a link, as the C++'s does.
        if path.metadata().is_ok_and(|metadata| metadata.is_file())
            && kind_for_path(&path) == SourceKind::PointCloud
            && let Ok(cloud) = describe_source(&path, &DescribeOptions::default())
        {
            clouds.push(cloud);
        }
        let folder_entry = entry.file_type().is_ok_and(|kind| kind.is_dir());
        if folder_entry && !walk(&path, clouds) {
            return false;
        }
    }
    true
}

/// GDAL's `dataset identify`, recursive and detailed, and the point clouds
/// beside what it found: GDAL reads no LAS, so a survey folder's scans would
/// otherwise be missing from what it holds.
fn identify(folder: &Path) -> Result<Folder> {
    let request = RunRequest {
        path: vec![String::from("dataset"), String::from("identify")],
        tokens: vec![
            String::from("--recursive"),
            String::from("--detailed"),
            String::from("--format=json"),
            path_text(folder),
        ],
        ..RunRequest::default()
    };
    let outputs = gp::run(&request, None, None)?;
    let mut found = Folder {
        identified: outputs.text.unwrap_or_else(|| String::from("[]")),
        clouds: Vec::new(),
    };
    walk(folder, &mut found.clouds);
    Ok(found)
}

/// A member of GDAL's identify JSON as text: the C++'s `value(key,
/// std::string())`, which throws for what is not text - here a panic with
/// nlohmann's words, which the session reports as the C++ catches it.
fn text_member(each: &Json, key: &str) -> String {
    each.value(key, String::new())
        .unwrap_or_else(|error| panic!("{}", error.what))
}

fn folder_records(folder: &Path, found: &Folder) -> Vec<String> {
    let mut files = Vec::new();
    let identified = parsed_or_null(&found.identified);
    if identified.is_array() {
        for each in identified.iter() {
            // GDAL names a file with the separator this system uses; a record
            // names it as a line would.
            let mut record = format!(
                "found file={} driver={}",
                value(&path_text(&path_from_text(&text_member(each, "name")))),
                value(&text_member(each, "driver"))
            );
            for (key, flag) in each.items() {
                if let Some(flag) = flag.as_bool() {
                    record += &format!(" {key}={}", yes_no(flag));
                }
            }
            files.push(record);
        }
    }
    // GDAL and the folder walk list files in the order the file system
    // gives them - by name on NTFS, by hash on ext4 - and a reply must not
    // change with the machine: each list is sorted by file name, GDAL's
    // datasets first and then the clouds PDAL reads.
    files.sort();
    let gdal_files = files.len();
    for cloud in &found.clouds {
        files.push(format!(
            "found file={} driver={} has_crs={}",
            value(&path_text(&cloud.path)),
            value(&cloud.driver),
            yes_no(!cloud.crs.is_empty())
        ));
    }
    files[gdal_files..].sort();
    let mut records = vec![format!(
        "dataset file={} kind=folder driver= crs= datasets={}",
        value(&path_text(folder)),
        files.len()
    )];
    records.extend(files);
    records
}

/// std::filesystem::path::extension of `path`, lower case: a path whose text
/// ends in a separator has an empty file name and so none, where Rust's Path
/// reads the last name before the separator.
fn lowered_extension(path: &Path) -> String {
    if path_text(path).ends_with('/') {
        return String::new();
    }
    path.extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()).to_ascii_lowercase())
        .unwrap_or_default()
}

/// The whole of INFO's work: pure, reading only the file.
fn describe(request: &InfoRequest) -> Result<String> {
    let folder = !is_virtual_path(&request.path)
        && request.path.is_dir()
        && lowered_extension(&request.path) != ".gdb";
    if folder {
        let found = identify(&request.path)?;
        if request.json {
            let mut clouds = Vec::new();
            for cloud in &found.clouds {
                let mut entry = point_cloud_json(cloud);
                entry["file"] = Json::from(path_text(&cloud.path));
                clouds.push(entry);
            }
            return Ok(Json::object_of([
                ("path", Json::from(path_text(&request.path))),
                ("identify", parsed_or_null(&found.identified)),
                ("pointclouds", Json::array_of(clouds)),
            ])
            .dump());
        }
        return Ok(joined(&folder_records(&request.path, &found), "\n"));
    }

    let options = DescribeOptions {
        statistics: request.stats,
        layer: request.layer.clone(),
        multidim: request.json,
        any_format: true,
    };
    let description = describe_source(&request.path, &options)?;
    let checked = if request.check {
        Some(check(&request.path)?)
    } else {
        None
    };

    if request.json {
        let mut json = Json::object_of([("path", Json::from(path_text(&request.path)))]);
        if description.point_cloud.is_some() {
            json["pointcloud"] = point_cloud_json(&description);
        } else {
            json["raster"] = parsed_or_null(&description.raster_json);
            json["vector"] = parsed_or_null(&description.vector_json);
            json["multidim"] = parsed_or_null(&description.multidim_json);
        }
        if let Some(checked) = &checked {
            json["check"] = Json::object_of([
                ("return_code", Json::from(checked.code)),
                (
                    "problems",
                    Json::array_of(checked.problems.iter().map(Json::from)),
                ),
            ]);
        }
        return Ok(json.dump());
    }
    let mut records = dataset_records(&description, request);
    if let Some(checked) = &checked {
        records.push(format!(
            "check code={} problems={}",
            checked.code,
            checked.problems.len()
        ));
        for problem in &checked.problems {
            records.push(format!("problem text={}", value(problem)));
        }
    }
    Ok(joined(&records, "\n"))
}

/// Whether an INFO line is the executor's. INFO 12 is the interpreter's: an
/// entity, described. Every INFO was once taken for a file, so
/// katana_describe_entity answered "file does not exist" in every build with
/// GDAL. A file that is really called 12 is still described - but INFO #12 is
/// always the entity: Entity Information and katana_describe_entity send it,
/// and a file called 1 or #1 beside the program (a mistyped shell 2>1 makes
/// one) once turned both into "no importer reads files named ''".
pub fn takes_info(tokens: &Tokens) -> bool {
    if tokens.size() == 2 && CommandInterpreter::is_entity_id(&tokens[1]) {
        if tokens[1].starts_with('#') {
            return false;
        }
        return path_from_text(&tokens[1]).exists();
    }
    true
}

/// An INFO line (verb_table.rs, the I0 block's INFO row).
pub fn prepare_info(_context: &mut Context<'_>, tokens: &Tokens, _line: &str) -> Result<Prepared> {
    let request = parse(tokens)?;
    let title = format!(
        "INFO {}",
        request
            .path
            .file_name()
            .map(|name| path_text(Path::new(name)))
            .unwrap_or_default()
    );
    let work = make_work(move |stop, _progress| {
        let reply = describe(&request)?;
        if stop_requested(stop) {
            return Err(Error::new(ErrorCode::InvalidState, "cancelled"));
        }
        Ok(make_apply(move |_context| Ok(reply)))
    });
    Ok(Prepared {
        title,
        reply: None,
        work: Some(work),
    })
}
