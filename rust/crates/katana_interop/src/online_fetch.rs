//! Importing a layer from a web service: GIS > Online Data and ONLINE IMPORT
//! (include/katana/interop/online_fetch.hpp, src/katana_interop/online_fetch.cpp;
//! docs/gis_online.md).
//!
//! `fetch_online_layer` turns one catalogue layer and an area into what the
//! existing import paths already know how to take in, and takes it in through
//! them:
//!
//! ```text
//! imagery, elevation  the service's images or tiles for the area are
//!                     fetched (through the disk cache), warped and mosaicked
//!                     by GDAL into ONE GeoTIFF in the project's CRS at the
//!                     asked resolution, and that file goes through
//!                     import_raster - a reference raster like any other. An
//!                     elevation keeps its true Float32 values, so Surface
//!                     From Raster builds ground from it.
//! vector              the features are fetched page by page, each page read
//!                     by import_vector with the page moved into the
//!                     project's CRS (VectorImportOptions::target_crs), and
//!                     the pages joined with the features one page repeats
//!                     from another dropped - Entity values for the caller to
//!                     add as ONE command.
//! ```
//!
//! Every result records where it came from: the raster's source_url, licence
//! and attribution; each entity's metadata "online.source", "online.licence",
//! "online.attribution" and "online.retrieved". The URL recorded is the
//! service's address with any key removed (redact_url) - a key is sent, never
//! stored in the project, never logged.
//!
//! Nothing here touches a Document or a widget: the desktop application runs
//! it in a background job and applies the result on its own thread. It
//! blocks its thread; `stop` cancels it between and during requests, and a
//! cancelled import leaves nothing behind but what the cache already held.
//!
//! The Rust shapes of the C++'s callbacks:
//!
//! ```text
//! OnlineEnvironment::transport  an OnlineTransport, Arc<dyn Fn + Send + Sync>:
//!                               the environment is a value the app builds per
//!                               import and hands to a job thread, so it owns
//!                               what it holds - a borrowed closure would put
//!                               a lifetime on it - and it is cloned as the
//!                               C++'s std::function is copied.
//! progress                      an OnlineProgress, a borrowed Sync closure:
//!                               it is used only during the call, and GDAL's
//!                               warp calls it from its own threads.
//! std::stop_token               Option<&AtomicBool>, as the bridge and the
//!                               warp take it.
//! ```

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use katana_core::civil_date::{SECONDS_PER_DAY, civil_from_days, unix_seconds};
use katana_core::error::make_error;
use katana_core::text::{lowered, trimmed};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::entity::{property_to_string, to_string};
use katana_entity::entity_geometry::bounding_box;
use katana_entity::{Entity, PropertyValue};
use katana_gis::reproject::{
    CrsBox, WarpOptions, WarpSource, build_true_colour_vrt, clip_vector_file, crs_axis_is_yx,
    crs_epsg_code, probe_raster, transform_box, transform_point, warp_to_geotiff,
};
use katana_gis::web_access::{
    HttpRequest, HttpResponse, http_fetch, is_secret_parameter, percent_encode, redact_url,
    sha256_hex,
};
use katana_math::cpp;

use crate::import::{
    RasterImportOptions, VectorImportOptions, VectorImportResult, import_raster, import_vector,
};
use crate::online_catalogue::{
    LonLatBox, OnlineLayer, OnlineLayerKind, OnlineProvider, OnlineService, OnlineServiceType,
};
use crate::online_discovery::{custom_provider_id, discover_service};
use crate::online_requests::{
    CkanResource, StacItem, arcgis_export_image_url, arcgis_export_url, arcgis_query_url, area_km2,
    choose_stac_items, choose_tile_matrix, ckan_search_url, degree_tile_urls, expand_tile_template,
    format_number, http_href, oapif_items_url, overpass_query, parse_ckan_search, parse_stac_items,
    plan_image_requests, query_has_more, read_items_page, read_query_page, stac_search_body,
    tile_box, tiles_covering, wcs_get_coverage_url, web_mercator_tile_matrix_set,
    wfs_get_feature_url, wms_get_map_url,
};
use crate::reference_data::RasterOverlay;

/// Imagery with no resolution asked for: the area's longer side over this
/// many pixels - a sharp full-screen backdrop, 16 MB of RGBA at most.
const DEFAULT_IMAGERY_PIXELS: f64 = 2048.0;
/// Elevation with neither a resolution asked for nor one in the catalogue.
const DEFAULT_ELEVATION_PIXELS: f64 = 1024.0;
/// How far below the level a resolution asks for a tile service may fall to
/// stay within its tile budget before the import is refused instead: three
/// levels is an image eight times coarser, about as far as "a little blurry"
/// stretches.
const MAX_ZOOM_REDUCTION: i32 = 3;
/// The most image requests one import may split into (plan_image_requests).
const MAX_IMAGE_REQUESTS: i32 = 64;
/// A STAC search asks for this many items: a month of Sentinel-2 over one
/// area is a few dozen, and the choice is made among them.
const STAC_SEARCH_LIMIT: i32 = 100;
/// The nanoseconds of a day: a cache's age is compared in them.
const DAY_NANOS: i128 = SECONDS_PER_DAY as i128 * 1_000_000_000;

/// What sends a request (OnlineEnvironment::transport): the request, and the
/// stop that cancels it.
pub type OnlineTransport =
    Arc<dyn Fn(&HttpRequest, Option<&AtomicBool>) -> Result<HttpResponse> + Send + Sync>;

/// Progress in [0, 1] (negative: busy with no measure) and a stage.
pub type OnlineProgress<'a> = &'a (dyn Fn(f64, &str) + Sync);

/// "Katana/<version> (survey and civil-engineering CAD;
/// +https://github.com/kjarada/Katana)": what every request says it is, as
/// the OpenStreetMap tile and Overpass policies require - the program by
/// name, and where to find who makes it.
pub fn online_user_agent(version: &str) -> String {
    format!(
        "Katana/{version} (survey and civil-engineering CAD; +https://github.com/kjarada/Katana)"
    )
}

/// How the fetcher may behave: where it caches, what it says it is, its keys
/// and its limits. Each limit is refused with a message saying how to stay
/// within it, never truncated silently.
#[derive(Clone)]
pub struct OnlineEnvironment {
    /// Where answers are kept: <cache>/http for what services returned,
    /// <cache>/products for finished rasters. Empty disables the cache.
    pub cache_directory: PathBuf,
    pub user_agent: String,
    /// Keys by name (OnlineLayer::key_name), from the application's settings.
    pub keys: BTreeMap<String, String>,
    pub timeout_seconds: i32,
    pub retries: i32,
    /// One answer's size, and the pixels of one finished raster.
    pub max_response_bytes: u64,
    pub max_pixels: u64,
    /// Features in one import, and pages of them.
    pub max_features: u64,
    pub max_pages: i32,
    /// Answer from the cache only, whatever its age; fail where it has
    /// nothing.
    pub offline: bool,
    /// What sends a request. None: http_fetch, the network. The tests put a
    /// function here that answers from fixtures and counts what it was asked,
    /// so paging, caching, cancellation and every limit are exercised with no
    /// network at all.
    pub transport: Option<OnlineTransport>,
}

impl Default for OnlineEnvironment {
    fn default() -> Self {
        Self {
            cache_directory: PathBuf::new(),
            user_agent: online_user_agent("dev"),
            keys: BTreeMap::new(),
            timeout_seconds: 120,
            retries: 3,
            max_response_bytes: 256 * 1024 * 1024,
            max_pixels: 64 * 1024 * 1024,
            max_features: 250_000,
            max_pages: 500,
            offline: false,
            transport: None,
        }
    }
}

impl fmt::Debug for OnlineEnvironment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OnlineEnvironment")
            .field("cache_directory", &self.cache_directory)
            .field("user_agent", &self.user_agent)
            .field("keys", &self.keys.keys().collect::<Vec<_>>())
            .field("timeout_seconds", &self.timeout_seconds)
            .field("retries", &self.retries)
            .field("max_response_bytes", &self.max_response_bytes)
            .field("max_pixels", &self.max_pixels)
            .field("max_features", &self.max_features)
            .field("max_pages", &self.max_pages)
            .field("offline", &self.offline)
            .field("transport", &self.transport.as_ref().map(|_| "a transport"))
            .finish()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OnlineRequestOptions {
    /// The area, in `area_crs` (traditional order).
    pub area: CrsBox,
    pub area_crs: String,
    /// The project's CRS: where everything is brought.
    pub target_crs: String,
    /// Rasters: target units per pixel. Absent: the layer's own resolution
    /// for elevation, and the area's longer side over 2048 pixels for
    /// imagery.
    pub resolution: Option<f64>,
    /// Vectors: the Katana layer the entities go on. Empty:
    /// online/<provider>/<layer>.
    pub target_layer: String,
    /// STAC: the acquisition dates (YYYY-MM-DD) and the cloud ceiling,
    /// percent. Empty dates: the layer's look-back from today.
    pub from_date: String,
    pub to_date: String,
    pub max_cloud: Option<f64>,
    /// Overpass: the tag test, when the layer has none of its own or it is to
    /// be replaced ("building", "highway=primary").
    pub filter: String,
    /// WMS/WMTS time dimension; empty: the layer's own.
    pub time: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OnlineStats {
    /// Made over the network.
    pub requests: i32,
    /// Answered from the cache.
    pub cache_hits: i32,
    pub bytes: u64,
    pub pages: i32,
    /// Features a later page repeated, dropped.
    pub duplicates: u64,
    /// The finished raster itself was cached.
    pub product_from_cache: bool,
    /// Rasters read by GDAL itself over HTTP (a COG through /vsicurl/), whose
    /// range requests are GDAL's and not counted in `requests`.
    pub remote_sources: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OnlineImport {
    pub kind: OnlineLayerKind,
    pub raster: Option<RasterOverlay>,
    pub vectors: Option<VectorImportResult>,
    /// The finished GeoTIFF, for a raster.
    pub file: PathBuf,
    /// For a raster: target units per pixel.
    pub resolution: f64,
    /// Redacted.
    pub source_url: String,
    pub licence: String,
    pub attribution: String,
    /// YYYY-MM-DD, UTC.
    pub retrieved: String,
    pub stats: OnlineStats,
    pub warnings: Vec<String>,
}

impl Default for OnlineImport {
    fn default() -> Self {
        Self {
            kind: OnlineLayerKind::Imagery,
            raster: None,
            vectors: None,
            file: PathBuf::new(),
            resolution: 0.0,
            source_url: String::new(),
            licence: String::new(),
            attribution: String::new(),
            retrieved: String::new(),
            stats: OnlineStats::default(),
            warnings: Vec::new(),
        }
    }
}

// ---- helpers ------------------------------------------------------------------------------------

fn stopped(stop: Option<&AtomicBool>) -> bool {
    stop.is_some_and(|stop| stop.load(Ordering::Acquire))
}

/// `std::format("{:%F}", days)`: YYYY-MM-DD.
fn date_text(days: i64) -> String {
    let date = civil_from_days(days);
    let year = if date.year < 0 {
        format!("-{:04}", date.year.unsigned_abs())
    } else {
        format!("{:04}", date.year)
    };
    format!("{year}-{:02}-{:02}", date.month, date.day)
}

/// Days since 1970-01-01, UTC, floored, as `floor<days>(system_clock::now())`.
fn days_now() -> i64 {
    unix_seconds(SystemTime::now()).div_euclid(SECONDS_PER_DAY)
}

fn today() -> String {
    date_text(days_now())
}

fn days_before(days: i32) -> String {
    date_text(days_now() - i64::from(days))
}

fn lower(text: &str) -> String {
    text.to_ascii_lowercase()
}

/// `path.string()`, for an error's context.
fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// `text.find(piece) != npos`, of bytes.
fn holds(text: &[u8], piece: &[u8]) -> bool {
    piece.is_empty() || text.windows(piece.len()).any(|window| window == piece)
}

/// A file extension for an answer, from its Content-Type, else from the
/// URL's path, else ".bin": GDAL chooses its driver partly by extension, so a
/// PNG saved as .bin is harder for it to open than one saved as .png.
fn extension_for(content_type: &str, url: &str) -> String {
    let content = lower(content_type);
    if content.contains("png") {
        return String::from(".png");
    }
    if content.contains("jpeg") || content.contains("jpg") {
        return String::from(".jpg");
    }
    if content.contains("tiff") {
        return String::from(".tif");
    }
    let path = lower(&url[..url.find('?').unwrap_or(url.len())]);
    for known in [
        ".png", ".jpg", ".jpeg", ".tif", ".tiff", ".json", ".geojson", ".xml",
    ] {
        if path.ends_with(known) {
            return String::from(known);
        }
    }
    String::from(".bin")
}

/// The first bytes of a file, to tell an image from an error document a
/// service returned with a 200: a WMS ServiceException, an ArcGIS {"error"}.
fn head_of(path: &Path, bytes: usize) -> Vec<u8> {
    use std::io::Read;
    let mut head = Vec::with_capacity(bytes);
    if let Ok(file) = fs::File::open(path) {
        let _ = file.take(bytes as u64).read_to_end(&mut head);
    }
    head
}

/// The first bytes of the image formats a service sends: PNG, JPEG, TIFF and
/// BigTIFF in either byte order, GIF, and RIFF (WebP).
const IMAGE_SIGNATURES: [&[u8]; 8] = [
    b"\x89PNG",
    b"\xFF\xD8",
    b"II*",
    b"MM\0*",
    b"GIF8",
    b"RIFF",
    b"II+",
    b"MM\0+",
];

fn looks_like_image(head: &[u8]) -> bool {
    IMAGE_SIGNATURES
        .iter()
        .any(|signature| head.starts_with(signature))
}

/// A file's bytes; nothing for a file that cannot be read.
fn read_whole(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_default()
}

/// An error document (XML or JSON) a service sent where data was expected,
/// reduced to the words a person can act on.
fn service_message(head: &[u8]) -> String {
    let mut text: Vec<u8> = Vec::new();
    let mut in_tag = false;
    for &byte in head {
        if byte == b'<' {
            in_tag = true;
            continue;
        }
        if byte == b'>' {
            in_tag = false;
            text.push(b' ');
            continue;
        }
        if !in_tag {
            text.push(if matches!(byte, b'\n' | b'\r' | b'\t') {
                b' '
            } else {
                byte
            });
        }
    }
    let mut collapsed: Vec<u8> = Vec::new();
    for &byte in &text {
        if byte == b' ' && collapsed.last().is_none_or(|&last| last == b' ') {
            continue;
        }
        collapsed.push(byte);
    }
    let trimmed_text = trimmed(&collapsed);
    String::from_utf8_lossy(&trimmed_text[..trimmed_text.len().min(300)]).into_owned()
}

fn make_request(url: &str, environment: &OnlineEnvironment) -> HttpRequest {
    HttpRequest {
        url: String::from(url),
        user_agent: environment.user_agent.clone(),
        timeout_seconds: environment.timeout_seconds,
        retries: environment.retries,
        max_bytes: environment.max_response_bytes,
        ..HttpRequest::default()
    }
}

fn cache_key(request: &HttpRequest) -> String {
    sha256_hex(format!(
        "{}{}\n{}",
        if request.post_body.is_empty() {
            "GET\n"
        } else {
            "POST\n"
        },
        request.url,
        request.post_body
    ))
}

fn cache_path(root: &Path, folder: &str, key: &str, extension: &str) -> PathBuf {
    root.join(folder)
        .join(&key[..key.len().min(2)])
        .join(format!("{key}{extension}"))
}

/// A finished raster used again is dated now, so the cache's pruning
/// (oldest first) lets go of it last, and it stays fresh while it is in use;
/// it is a product of answers that each keep their own age.
fn touched(path: &Path) {
    if let Ok(file) = fs::File::options().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

/// `now - written`, signed, in nanoseconds: negative for a time to come.
fn age_nanos(now: SystemTime, written: SystemTime) -> i128 {
    match now.duration_since(written) {
        Ok(age) => i128::try_from(age.as_nanos()).unwrap_or(i128::MAX),
        Err(ahead) => -i128::try_from(ahead.duration().as_nanos()).unwrap_or(i128::MAX),
    }
}

fn fresh(path: &Path, max_age_days: i32, offline: bool) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() || metadata.len() == 0 {
        return false;
    }
    if offline {
        return true;
    }
    let Ok(written) = metadata.modified() else {
        return false;
    };
    age_nanos(SystemTime::now(), written) < DAY_NANOS * i128::from(max_age_days.max(0))
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Status {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut temporary = OsString::from(path.as_os_str());
    temporary.push(".part");
    let temporary = PathBuf::from(temporary);
    if fs::write(&temporary, bytes).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "could not write to the online cache",
            path_string(&temporary),
        ));
    }
    if fs::rename(&temporary, path).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "could not write to the online cache",
            path_string(path),
        ));
    }
    Ok(())
}

/// Where one fetched answer lives when there is no cache: a scratch folder of
/// the system's, removed by nobody but small and named by hash like the
/// cache.
fn scratch_root() -> PathBuf {
    std::env::temp_dir().join("katana-online")
}

fn cache_root(environment: &OnlineEnvironment) -> PathBuf {
    if environment.cache_directory.as_os_str().is_empty() {
        scratch_root()
    } else {
        environment.cache_directory.clone()
    }
}

fn lon_lat_of(lon_lat: &LonLatBox) -> CrsBox {
    CrsBox {
        min_x: lon_lat[0],
        min_y: lon_lat[1],
        max_x: lon_lat[2],
        max_y: lon_lat[3],
    }
}

fn overlaps(a: &CrsBox, b: &CrsBox) -> bool {
    a.min_x < b.max_x && b.min_x < a.max_x && a.min_y < b.max_y && b.min_y < a.max_y
}

fn remove_quietly(path: &Path) {
    let _ = fs::remove_file(path);
}

struct Prepared {
    /// The area in WGS 84 longitude/latitude.
    lon_lat: CrsBox,
    /// The area in the project's CRS.
    target: CrsBox,
    endpoint: String,
}

fn prepare(
    layer: &OnlineLayer,
    options: &OnlineRequestOptions,
    environment: &OnlineEnvironment,
) -> Result<Prepared> {
    if options.target_crs.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidCrs,
            "the project has no coordinate system, so web data has nowhere to go; set one (the \
             project's metadata) or give crs=EPSG:<code>",
        ));
    }
    if !options.area.valid() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the area is empty or inverted",
        ));
    }
    let area_crs = if options.area_crs.is_empty() {
        options.target_crs.as_str()
    } else {
        options.area_crs.as_str()
    };
    let lon_lat = transform_box(&options.area, area_crs, "EPSG:4326")?;
    if !overlaps(&lon_lat, &lon_lat_of(&layer.coverage)) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "the area is outside what {} covers ({},{} to {},{})",
                layer.provider_title,
                format_number(layer.coverage[0]),
                format_number(layer.coverage[1]),
                format_number(layer.coverage[2]),
                format_number(layer.coverage[3])
            ),
            format!(
                "{},{} to {},{}",
                format_number(lon_lat.min_x),
                format_number(lon_lat.min_y),
                format_number(lon_lat.max_x),
                format_number(lon_lat.max_y)
            ),
        ));
    }
    let target = if area_crs == options.target_crs {
        options.area
    } else {
        transform_box(&options.area, area_crs, &options.target_crs)?
    };
    let endpoint = endpoint_with_key(layer, environment)?;
    Ok(Prepared {
        lon_lat,
        target,
        endpoint,
    })
}

/// The layer as the request builders should see it: its endpoint with the
/// key filled in.
fn with_endpoint(layer: &OnlineLayer, endpoint: &str) -> OnlineLayer {
    OnlineLayer {
        endpoint: String::from(endpoint),
        ..layer.clone()
    }
}

fn default_resolution(layer: &OnlineLayer, target: &CrsBox) -> f64 {
    let longer = cpp::max(target.width(), target.height());
    if layer.kind == OnlineLayerKind::Elevation {
        if let Some(resolution) = layer.resolution {
            // The data's own spacing, unless that would be over the pixel
            // budget - then the budget's.
            return cpp::max(resolution, longer / 8192.0);
        }
        return longer / DEFAULT_ELEVATION_PIXELS;
    }
    longer / DEFAULT_IMAGERY_PIXELS
}

// ---- rasters ------------------------------------------------------------------------------------

/// How many units of `service_crs` one unit of the project's CRS is, at the
/// area's centre: two short steps, east and north, measured in both. The
/// ratio of the two boxes' areas was used first and chose a zoom one level
/// too fine: a box in MGA is rotated against Web Mercator by the grid
/// convergence, so its bounding box is larger than the area by a few per
/// cent - enough to cross a level boundary.
fn local_scale(target_crs: &str, target: &CrsBox, service_crs: &str) -> Result<f64> {
    let cx = 0.5 * (target.min_x + target.max_x);
    let cy = 0.5 * (target.min_y + target.max_y);
    let step = 0.01 * cpp::max(target.width(), target.height());
    let centre = transform_point(cx, cy, target_crs, service_crs);
    let east = transform_point(cx + step, cy, target_crs, service_crs);
    let north = transform_point(cx, cy + step, target_crs, service_crs);
    let (Ok(centre), Ok(east), Ok(north)) = (centre, east, north) else {
        return Err(Error::new(
            ErrorCode::InvalidCrs,
            "the area cannot be expressed in the service's coordinate system",
        ));
    };
    let eastward = (east[0] - centre[0]).hypot(east[1] - centre[1]);
    let northward = (north[0] - centre[0]).hypot(north[1] - centre[1]);
    Ok(0.5 * (eastward + northward) / step)
}

#[derive(Default)]
struct RasterPlan {
    sources: Vec<WarpSource>,
    skip_missing: bool,
}

/// What a raster fetch hands along: the stop and the progress, and where the
/// counts and the warnings go.
struct Fetching<'a> {
    stats: &'a mut OnlineStats,
    warnings: &'a mut Vec<String>,
    stop: Option<&'a AtomicBool>,
    progress: Option<OnlineProgress<'a>>,
}

impl Fetching<'_> {
    fn report(&self, fraction: f64, stage: &str) {
        if let Some(progress) = self.progress {
            progress(fraction, stage);
        }
    }
}

fn tile_sources(
    layer: &OnlineLayer,
    prepared: &Prepared,
    resolution: f64,
    options: &OnlineRequestOptions,
    environment: &OnlineEnvironment,
    fetching: &mut Fetching<'_>,
) -> Result<RasterPlan> {
    let set = web_mercator_tile_matrix_set(layer.max_zoom);
    let mercator = transform_box(&prepared.lon_lat, "EPSG:4326", "EPSG:3857")?;
    // Web Mercator metres are not ground metres: at latitude phi one is
    // cos(phi) of a metre. The ratio of the two boxes carries that, and any
    // other scale between the project's CRS and the grid.
    let ratio = local_scale(&options.target_crs, &prepared.target, "EPSG:3857")?;
    let wanted = choose_tile_matrix(&set, resolution * ratio);
    let mut level = wanted;
    let mut tiles = tiles_covering(&set, level, &mercator);
    while tiles.len() as i64 > i64::from(layer.max_tiles) && level > 0 {
        level -= 1;
        tiles = tiles_covering(&set, level, &mercator);
    }
    if tiles.len() as i64 > i64::from(layer.max_tiles) || wanted - level > MAX_ZOOM_REDUCTION {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "the area needs more than the {} tiles {} allows one import at this resolution; \
                 choose a smaller area or a coarser resolution",
                layer.max_tiles, layer.provider_title
            ),
        ));
    }
    if level < wanted {
        fetching.warnings.push(format!(
            "tiles fetched at zoom {level} rather than {wanted} to stay within {} tiles",
            layer.max_tiles
        ));
    }
    let time = if !options.time.is_empty() {
        options.time.clone()
    } else if layer.time.is_empty() {
        String::from("default")
    } else {
        layer.time.clone()
    };
    let extra = BTreeMap::from([
        (String::from("layer"), layer.layer_name.clone()),
        (String::from("time"), time),
    ]);
    let mut plan = RasterPlan::default();
    let mut missing = 0;
    for (i, tile) in tiles.iter().enumerate() {
        if stopped(fetching.stop) {
            return Err(Error::new(ErrorCode::InvalidState, "cancelled"));
        }
        fetching.report(
            0.6 * i as f64 / tiles.len() as f64,
            &format!("Fetching tile {} of {}", i + 1, tiles.len()),
        );
        let url = expand_tile_template(&prepared.endpoint, tile, "", &extra);
        let file = match fetch_to_cache(
            &make_request(&url, environment),
            &extension_for("", &url),
            layer.cache_days,
            environment,
            fetching.stats,
            fetching.stop,
        ) {
            Ok(file) => file,
            // A tile the service has not rendered (the sea at zoom 19) is a
            // hole in the mosaic, not a failed import.
            Err(error) if error.code == ErrorCode::NotFound => {
                missing += 1;
                continue;
            }
            Err(error) => return Err(error),
        };
        let head = head_of(&file, 512);
        if !looks_like_image(&head) {
            remove_quietly(&file);
            return Err(make_error(
                ErrorCode::FileImportFailure,
                "the tile service sent something other than an image",
                service_message(&head),
            ));
        }
        plan.sources.push(WarpSource {
            path: path_string(&file),
            bounds: Some(tile_box(&set, tile)),
            crs: String::from("EPSG:3857"),
        });
    }
    if missing > 0 {
        fetching.warnings.push(format!(
            "{missing} of {} tiles do not exist at the service; those parts are empty",
            tiles.len()
        ));
    }
    if plan.sources.is_empty() {
        return Err(Error::new(
            ErrorCode::NotFound,
            "the service has no tiles for this area",
        ));
    }
    Ok(plan)
}

fn image_sources(
    layer: &OnlineLayer,
    prepared: &Prepared,
    resolution: f64,
    target_crs: &str,
    environment: &OnlineEnvironment,
    fetching: &mut Fetching<'_>,
) -> Result<RasterPlan> {
    let arcgis = layer.r#type == OnlineServiceType::ArcgisExport
        || layer.r#type == OnlineServiceType::ArcgisExportImage;
    let crs = if !layer.crs.is_empty() {
        layer.crs.as_str()
    } else if arcgis {
        "EPSG:3857"
    } else {
        "EPSG:4326"
    };
    // The area as the service sees it; from longitude/latitude, which every
    // area has, rather than chaining through the project's CRS.
    let service_box = transform_box(&prepared.lon_lat, "EPSG:4326", crs)?;
    let ratio = local_scale(target_crs, &prepared.target, crs)?;
    let requests = plan_image_requests(
        &service_box,
        resolution * ratio,
        layer.max_request_pixels,
        MAX_IMAGE_REQUESTS,
    )?;
    let resolved = with_endpoint(layer, &prepared.endpoint);
    let mut epsg = 0;
    if arcgis {
        let Some(code) = crs_epsg_code(crs) else {
            return Err(make_error(
                ErrorCode::InvalidCrs,
                "ArcGIS requests need an EPSG code",
                crs,
            ));
        };
        epsg = code;
    }
    let mut axis_yx = false;
    if layer.r#type == OnlineServiceType::Wms {
        axis_yx = crs_axis_is_yx(crs)?;
    }
    let mut plan = RasterPlan::default();
    for (i, request) in requests.iter().enumerate() {
        if stopped(fetching.stop) {
            return Err(Error::new(ErrorCode::InvalidState, "cancelled"));
        }
        fetching.report(
            0.6 * i as f64 / requests.len() as f64,
            &format!("Fetching image {} of {}", i + 1, requests.len()),
        );
        let (url, extension) = match layer.r#type {
            OnlineServiceType::ArcgisExport => {
                (arcgis_export_url(&resolved, request, epsg), ".png")
            }
            OnlineServiceType::ArcgisExportImage => (
                arcgis_export_image_url(&resolved, request, epsg),
                if layer.kind == OnlineLayerKind::Elevation {
                    ".tif"
                } else {
                    ".png"
                },
            ),
            OnlineServiceType::Wms => (
                wms_get_map_url(&resolved, request, crs, axis_yx, "image/png"),
                ".png",
            ),
            OnlineServiceType::Wcs => (wcs_get_coverage_url(&resolved, request, crs), ".tif"),
            _ => return Err(Error::new(ErrorCode::Internal, "not an image service")),
        };
        let file = fetch_to_cache(
            &make_request(&url, environment),
            extension,
            layer.cache_days,
            environment,
            fetching.stats,
            fetching.stop,
        )?;
        let head = head_of(&file, 512);
        if !looks_like_image(&head) {
            // Not kept: the next run must ask again rather than read the
            // refusal back from the cache.
            remove_quietly(&file);
            return Err(make_error(
                ErrorCode::FileImportFailure,
                "the service answered with a message instead of an image",
                service_message(&head),
            ));
        }
        plan.sources.push(WarpSource {
            path: path_string(&file),
            bounds: Some(request.r#box),
            crs: String::from(crs),
        });
    }
    Ok(plan)
}

/// The %XX of a file URL's path decoded, for GDAL to open directly; any
/// other URL through /vsicurl/.
fn vsicurl(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("file://") {
        let rest = rest.as_bytes();
        let mut path: Vec<u8> = Vec::with_capacity(rest.len());
        let mut i = 0;
        while i < rest.len() {
            if rest[i] == b'%'
                && i + 2 < rest.len()
                && rest[i + 1].is_ascii_hexdigit()
                && rest[i + 2].is_ascii_hexdigit()
            {
                let hex = std::str::from_utf8(&rest[i + 1..i + 3]).unwrap_or("0");
                path.push(u8::from_str_radix(hex, 16).unwrap_or(0));
                i += 3;
            } else {
                path.push(rest[i]);
                i += 1;
            }
        }
        if path.len() > 2 && path[0] == b'/' && path[2] == b':' {
            path.remove(0);
        }
        return String::from_utf8_lossy(&path).into_owned();
    }
    format!("/vsicurl/{url}")
}

/// The first `count` bytes of `text`, lower-cased: the C++'s
/// `lower(text.substr(0, count))`.
fn lowered_prefix(text: &str, count: usize) -> Vec<u8> {
    lowered(&text.as_bytes()[..text.len().min(count)])
}

/// Where a STAC asset is, from its href and the address of the document that
/// named it. An absolute http(s) or s3 href is used as it is (s3:// through
/// the bucket's https address); a relative one ("./B04.tif", which static
/// catalogues write) is resolved against the document's folder. A file://
/// href is honoured only in a document that was itself read from a file: a
/// service's answer cannot make Katana open a file on this computer.
fn asset_source(href: &str, base: &str) -> Result<String> {
    let folded = lowered_prefix(href, 8);
    if folded.starts_with(b"https://")
        || folded.starts_with(b"http://")
        || folded.starts_with(b"s3://")
    {
        return Ok(vsicurl(&http_href(href)));
    }
    if folded.starts_with(b"file://") {
        if !lowered_prefix(base, 7).starts_with(b"file://") {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a STAC item from the network names a file on this computer; refused",
                href,
            ));
        }
        return Ok(vsicurl(href));
    }
    if href.contains("://") || href.starts_with('/') {
        return Err(make_error(
            ErrorCode::Unsupported,
            "the STAC asset's address is not one Katana reads",
            href,
        ));
    }
    let folder = &base[..base.find(['?', '#']).unwrap_or(base.len())];
    let folder = &folder[..folder.rfind('/').map_or(0, |slash| slash + 1)];
    let mut relative = href;
    while let Some(rest) = relative.strip_prefix("./") {
        relative = rest;
    }
    Ok(vsicurl(&format!("{folder}{relative}")))
}

fn cog_sources(layer: &OnlineLayer, prepared: &Prepared) -> RasterPlan {
    let mut plan = RasterPlan::default();
    if layer.tiling == "degree" {
        for url in degree_tile_urls(&prepared.endpoint, &prepared.lon_lat) {
            plan.sources.push(WarpSource {
                path: vsicurl(&url),
                ..WarpSource::default()
            });
        }
        // Over the sea there is no tile: skipped, and the mosaic is empty
        // there.
        plan.skip_missing = true;
    } else {
        plan.sources.push(WarpSource {
            path: vsicurl(&prepared.endpoint),
            ..WarpSource::default()
        });
    }
    plan
}

fn stac_sources(
    layer: &OnlineLayer,
    prepared: &Prepared,
    options: &OnlineRequestOptions,
    environment: &OnlineEnvironment,
    fetching: &mut Fetching<'_>,
) -> Result<RasterPlan> {
    let search = lower(&prepared.endpoint).ends_with("/search");
    let items: Vec<StacItem> = if search {
        let from = if !options.from_date.is_empty() {
            options.from_date.clone()
        } else if layer.days > 0 {
            days_before(layer.days)
        } else {
            String::new()
        };
        let to = if !options.to_date.is_empty() {
            options.to_date.clone()
        } else {
            today()
        };
        let request = HttpRequest {
            post_body: stac_search_body(
                &layer.layer_name,
                &prepared.lon_lat,
                &from,
                &to,
                Some(options.max_cloud.unwrap_or(layer.max_cloud)),
                STAC_SEARCH_LIMIT,
            ),
            post_content_type: String::from("application/geo+json"),
            ..make_request(&prepared.endpoint, environment)
        };
        fetching.report(-1.0, &format!("Searching {}", layer.provider_title));
        // A search is fresh for a day: new scenes arrive daily.
        let file = fetch_to_cache(
            &request,
            ".json",
            1,
            environment,
            fetching.stats,
            fetching.stop,
        )?;
        match parse_stac_items(read_whole(&file)) {
            Ok(items) => items,
            Err(error) => {
                remove_quietly(&file);
                return Err(error);
            }
        }
    } else {
        let file = fetch_to_cache(
            &make_request(&prepared.endpoint, environment),
            ".json",
            layer.cache_days,
            environment,
            fetching.stats,
            fetching.stop,
        )?;
        match parse_stac_items(read_whole(&file)) {
            Ok(items) => items,
            Err(error) => {
                remove_quietly(&file);
                return Err(error);
            }
        }
    };
    let choice = choose_stac_items(&items, &prepared.lon_lat);
    if choice.items.is_empty() {
        return Err(Error::new(
            ErrorCode::NotFound,
            format!(
                "no scene meets the area{}",
                if search {
                    " between those dates under that cloud cover; widen the dates (from= to=) or \
                     raise cloud="
                } else {
                    ""
                }
            ),
        ));
    }
    if !choice.covers_area {
        fetching.warnings.push(format!(
            "the scenes of {} cover only part of the area",
            choice.day
        ));
    }
    fetching.warnings.push(format!(
        "scene date {}, cloud cover {}%",
        choice.day,
        format_number((choice.cloud_cover * 10.0).round() / 10.0)
    ));
    let mut plan = RasterPlan::default();
    let composite = layer.asset.contains(',');
    for item in &choice.items {
        if !composite {
            let Some(asset) = item.assets.get(&layer.asset) else {
                return Err(make_error(
                    ErrorCode::NotFound,
                    format!("the scene has no '{}' asset", layer.asset),
                    item.id.clone(),
                ));
            };
            let source = asset_source(asset, &prepared.endpoint)?;
            plan.sources.push(WarpSource {
                path: source,
                ..WarpSource::default()
            });
            continue;
        }
        let mut bands: Vec<String> = Vec::new();
        for key in ["red", "green", "blue"] {
            let Some(asset) = item.assets.get(key) else {
                return Err(make_error(
                    ErrorCode::NotFound,
                    format!("the scene has no '{key}' band"),
                    item.id.clone(),
                ));
            };
            bands.push(asset_source(asset, &prepared.endpoint)?);
        }
        let vrt = cache_path(
            &cache_root(environment),
            "products",
            &sha256_hex(format!("composite\n{}\n{}", item.id, bands[0])),
            ".vrt",
        );
        // Sentinel-2 L2A reflectance times 10000. Earth Search's COGs have
        // the offset of processing baseline 04.00 already removed (measured:
        // Sydney Harbour reads 35 in red on 2026-09-20), so 0..3000 - the
        // usual true-colour stretch - holds dark water to bright roofs.
        build_true_colour_vrt(
            &bands,
            0.0,
            3000.0,
            &vrt,
            &environment.user_agent,
            environment.timeout_seconds,
        )?;
        plan.sources.push(WarpSource {
            path: path_string(&vrt),
            ..WarpSource::default()
        });
    }
    Ok(plan)
}

fn product_key(
    layer: &OnlineLayer,
    prepared: &Prepared,
    options: &OnlineRequestOptions,
    resolution: f64,
) -> String {
    let mut key = format!(
        "product\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}/{}/{}\n{}\n{},{},{},{}\n{}\n{}/{}/{}/{}",
        to_string(layer.r#type),
        prepared.endpoint,
        layer.layer_name,
        layer.asset,
        to_string(layer.kind),
        layer.crs,
        layer.version,
        layer.max_zoom,
        layer.max_tiles,
        layer.max_request_pixels,
        options.target_crs,
        format_number(prepared.target.min_x),
        format_number(prepared.target.min_y),
        format_number(prepared.target.max_x),
        format_number(prepared.target.max_y),
        format_number(resolution),
        options.from_date,
        options.to_date,
        options.max_cloud.map_or_else(String::new, format_number),
        options.time
    );
    // A STAC search with no dates means "the last N days": the same words
    // tomorrow ask for a different window, so the day is part of the key.
    if layer.r#type == OnlineServiceType::Stac && options.to_date.is_empty() {
        key.push('\n');
        key.push_str(&today());
    }
    sha256_hex(key)
}

fn fetch_raster(
    layer: &OnlineLayer,
    options: &OnlineRequestOptions,
    environment: &OnlineEnvironment,
    stop: Option<&AtomicBool>,
    progress: Option<OnlineProgress<'_>>,
) -> Result<OnlineImport> {
    let prepared = prepare(layer, options, environment)?;
    let mut result = OnlineImport {
        kind: layer.kind,
        ..OnlineImport::default()
    };
    let resolution = options
        .resolution
        .unwrap_or_else(|| default_resolution(layer, &prepared.target));
    if !(resolution > 0.0) || !resolution.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the resolution must be positive",
        ));
    }
    result.resolution = resolution;
    // The finished raster's own size, refused before a single request.
    let pixels = (prepared.target.width() / resolution).ceil()
        * (prepared.target.height() / resolution).ceil();
    let max_pixels = environment.max_pixels as f64;
    if pixels > max_pixels {
        let fits = resolution * (pixels / max_pixels).sqrt();
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "the area at this resolution is {} million pixels, more than the {} million \
                 allowed; use res={} or coarser, or a smaller area",
                format_number((pixels / 1e6).round()),
                format_number((max_pixels / 1e5).round() / 10.0),
                format_number((fits * 100.0).ceil() / 100.0)
            ),
        ));
    }

    let product = cache_path(
        &cache_root(environment),
        "products",
        &product_key(layer, &prepared, options, resolution),
        ".tif",
    );
    let report = |fraction: f64, stage: &str| {
        if let Some(progress) = progress {
            progress(fraction, stage);
        }
    };
    if !fresh(&product, layer.cache_days, environment.offline) {
        if environment.offline {
            return Err(Error::new(
                ErrorCode::NotFound,
                "working offline, and the cache has no copy of this",
            ));
        }
        let mut fetching = Fetching {
            stats: &mut result.stats,
            warnings: &mut result.warnings,
            stop,
            progress,
        };
        let plan = match layer.r#type {
            OnlineServiceType::XyzTiles | OnlineServiceType::Wmts => tile_sources(
                layer,
                &prepared,
                resolution,
                options,
                environment,
                &mut fetching,
            )?,
            OnlineServiceType::ArcgisExport
            | OnlineServiceType::ArcgisExportImage
            | OnlineServiceType::Wms
            | OnlineServiceType::Wcs => image_sources(
                layer,
                &prepared,
                resolution,
                &options.target_crs,
                environment,
                &mut fetching,
            )?,
            OnlineServiceType::Cog => cog_sources(layer, &prepared),
            OnlineServiceType::Stac => {
                stac_sources(layer, &prepared, options, environment, &mut fetching)?
            }
            _ => {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    format!("a {} service gives no raster", to_string(layer.r#type)),
                ));
            }
        };
        report(0.6, "Warping into the project's coordinate system");
        let warp = WarpOptions {
            target_crs: options.target_crs.clone(),
            target_bounds: prepared.target,
            resolution,
            elevation: layer.kind == OnlineLayerKind::Elevation,
            max_pixels: environment.max_pixels,
            timeout_seconds: environment.timeout_seconds,
            user_agent: environment.user_agent.clone(),
            skip_missing_sources: plan.skip_missing,
        };
        let mut partial = OsString::from(product.as_os_str());
        partial.push(".part.tif");
        let partial = PathBuf::from(partial);
        let warp_progress = |fraction: f64| report(0.6 + 0.35 * fraction, "Warping");
        let warped = warp_to_geotiff(&plan.sources, &warp, &partial, stop, Some(&warp_progress))?;
        result.warnings.extend(warped.warnings);
        for source in &plan.sources {
            if source.path.starts_with("/vsicurl/") {
                result.stats.remote_sources += 1;
            }
        }
        if fs::rename(&partial, &product).is_err() {
            remove_quietly(&partial);
            return Err(make_error(
                ErrorCode::FileExportFailure,
                "could not keep the finished raster",
                path_string(&product),
            ));
        }
    } else {
        result.stats.product_from_cache = true;
        result.stats.cache_hits += 1;
        // Used now: pruning goes oldest-USED first, so a raster a session is
        // showing is the last thing the cache lets go of.
        touched(&product);
    }
    report(0.97, "Reading the result");
    let import_options = RasterImportOptions {
        name: format!("{} - {}", layer.provider_title, layer.title),
        ..RasterImportOptions::default()
    };
    let mut raster = import_raster(&product, &import_options)?;
    result.file = product;
    result.source_url = redact_url(&prepared.endpoint);
    result.licence.clone_from(&layer.licence);
    result.attribution.clone_from(&layer.attribution);
    result.retrieved = today();
    raster.source_url.clone_from(&result.source_url);
    raster.licence.clone_from(&result.licence);
    raster.attribution.clone_from(&result.attribution);
    result.raster = Some(raster);
    Ok(result)
}

// ---- vectors ------------------------------------------------------------------------------------

/// Joins the pages of one import, dropping a feature a later page repeats. A
/// feature is known by `id_field` when the service has one (matched without
/// regard to case), else by all its attributes together; within one page
/// nothing is dropped, because a multi-part feature arrives as several
/// entities with the same attributes on purpose.
struct PageJoiner {
    id_field: String,
    seen: BTreeSet<String>,
    result: VectorImportResult,
}

impl PageJoiner {
    fn new(id_field: &str) -> Self {
        Self {
            id_field: lower(id_field),
            seen: BTreeSet::new(),
            result: VectorImportResult::default(),
        }
    }

    /// Adds a page; returns how many of its features were new.
    fn add(&mut self, page: VectorImportResult, stats: &mut OnlineStats) -> u64 {
        let VectorImportResult {
            entities,
            layers_needed,
            bounds,
            features_read,
            features_skipped,
            projection_wkt,
            warnings,
            ..
        } = page;
        let mut this_page = BTreeSet::new();
        let mut added = 0;
        for entity in entities {
            let key = self.key_of(&entity);
            if self.seen.contains(&key) {
                stats.duplicates += 1;
                continue;
            }
            if this_page.insert(key) {
                added += 1;
            }
            self.result.bounds.expand_box(bounds);
            self.result.entities.push(entity);
        }
        self.seen.extend(this_page);
        for layer in layers_needed {
            if !self.result.layers_needed.contains(&layer) {
                self.result.layers_needed.push(layer);
            }
        }
        self.result.features_read += features_read;
        self.result.features_skipped += features_skipped;
        if self.result.projection_wkt.is_empty() {
            self.result.projection_wkt = projection_wkt;
        }
        for warning in warnings {
            if !self.result.warnings.contains(&warning) {
                self.result.warnings.push(warning);
            }
        }
        added
    }

    fn size(&self) -> usize {
        self.result.entities.len()
    }

    fn take(self) -> VectorImportResult {
        self.result
    }

    fn key_of(&self, entity: &Entity) -> String {
        if !self.id_field.is_empty() {
            for (name, value) in &entity.properties {
                if lower(name) == self.id_field {
                    return format!("id:{}", property_to_string(value));
                }
            }
        }
        // No id: the attributes AND where the feature is. Attributes alone
        // took two buildings with the same tags on different pages for one,
        // and dropped the second.
        let found = bounding_box(&entity.geometry);
        let mut key = format!(
            "{}|{},{},{},{}|",
            entity.geometry.index(),
            format_number(found.min.x),
            format_number(found.min.y),
            format_number(found.max.x),
            format_number(found.max.y)
        );
        for (name, value) in &entity.properties {
            key.push_str(name);
            key.push('=');
            key.push_str(&property_to_string(value));
            key.push('\x1f');
        }
        key
    }
}

fn import_page(
    path: &Path,
    layer: &OnlineLayer,
    options: &OnlineRequestOptions,
    filter: Option<CrsBox>,
) -> Result<VectorImportResult> {
    let import_options = VectorImportOptions {
        target_layer: if options.target_layer.is_empty() {
            format!("online/{}/{}", layer.provider_id, layer.id)
        } else {
            options.target_layer.clone()
        },
        layer_attribute: String::new(),
        target_crs: options.target_crs.clone(),
        // GeoJSON without a crs member is WGS 84 longitude/latitude (RFC
        // 7946).
        assumed_source_crs: String::from("EPSG:4326"),
        source_filter: filter,
        ..VectorImportOptions::default()
    };
    // A page, or a file filtered to the area, that holds no feature at all
    // is an empty answer - the area has none of these - not a failure. One
    // whose features could not be read still is.
    match import_vector(path, &import_options) {
        Err(error)
            if error.code == ErrorCode::InvalidArgument
                && error.context.starts_with("0 features read") =>
        {
            Ok(VectorImportResult::default())
        }
        imported => imported,
    }
}

/// The scheme and host of a URL: what a next link must share with the
/// service.
fn host_of(url: &str) -> &str {
    match url.find("://") {
        None => "",
        Some(scheme) => {
            let after = scheme + 3;
            &url[..url[after..]
                .find('/')
                .map_or(url.len(), |slash| slash + after)]
        }
    }
}

fn fetch_vectors(
    layer: &OnlineLayer,
    options: &OnlineRequestOptions,
    environment: &OnlineEnvironment,
    stop: Option<&AtomicBool>,
    progress: Option<OnlineProgress<'_>>,
) -> Result<OnlineImport> {
    let prepared = prepare(layer, options, environment)?;
    let mut result = OnlineImport {
        kind: OnlineLayerKind::Vector,
        ..OnlineImport::default()
    };
    let resolved = with_endpoint(layer, &prepared.endpoint);
    let mut joiner = PageJoiner::new(
        if layer.id_field.is_empty() && layer.r#type == OnlineServiceType::ArcgisQuery {
            "objectid"
        } else {
            layer.id_field.as_str()
        },
    );
    let too_many = || {
        Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "the area holds more than {} features; choose a smaller area",
                environment.max_features
            ),
        )
    };
    let report = |stage: &str| {
        if let Some(progress) = progress {
            progress(-1.0, stage);
        }
    };
    let over_limit = |joiner: &PageJoiner| joiner.size() as u64 > environment.max_features;

    match layer.r#type {
        OnlineServiceType::ArcgisQuery => {
            let mut offset: u64 = 0;
            for page in 0_i32.. {
                if page >= environment.max_pages {
                    return Err(too_many());
                }
                report(&format!(
                    "Fetching page {} ({} features)",
                    page + 1,
                    joiner.size()
                ));
                // Ordered by the layer's id field, when it names one, so that
                // no two pages can overlap or skip under a server that does
                // not order by itself.
                let url = arcgis_query_url(
                    &resolved,
                    &prepared.lon_lat,
                    offset,
                    layer.page_size,
                    &layer.id_field,
                );
                let file = fetch_to_cache(
                    &make_request(&url, environment),
                    ".geojson",
                    layer.cache_days,
                    environment,
                    &mut result.stats,
                    stop,
                )?;
                let state = match read_query_page(read_whole(&file)) {
                    Ok(state) => state,
                    Err(error) => {
                        remove_quietly(&file);
                        return Err(error);
                    }
                };
                result.stats.pages += 1;
                if state.features > 0 {
                    let imported = import_page(&file, layer, options, None)?;
                    if joiner.add(imported, &mut result.stats) == 0 && page > 0 {
                        // Every feature was one already had: the server
                        // ignored resultOffset. Stopping is the only way this
                        // ends.
                        result.warnings.push(String::from(
                            "the service repeated a page instead of moving on; it may not \
                             support paging, so some features may be missing - try a smaller \
                             area",
                        ));
                        break;
                    }
                }
                if over_limit(&joiner) {
                    return Err(too_many());
                }
                if !query_has_more(&state, layer.page_size) {
                    break;
                }
                offset += state.features;
            }
        }
        OnlineServiceType::Wfs => {
            let crs = if layer.crs.is_empty() {
                "EPSG:4326"
            } else {
                layer.crs.as_str()
            };
            let wfs_box = transform_box(&prepared.lon_lat, "EPSG:4326", crs)?;
            let v2 = layer.version.is_empty() || layer.version.starts_with("2.");
            let swap = crs_axis_is_yx(crs)?;
            let mut start: u64 = 0;
            for page in 0_i32.. {
                if page >= environment.max_pages {
                    return Err(too_many());
                }
                report(&format!(
                    "Fetching page {} ({} features)",
                    page + 1,
                    joiner.size()
                ));
                // 1.1.0 is always x,y in its BBOX for EPSG:4326 as GDAL and
                // servers read it; 2.0 follows the authority.
                let url = wfs_get_feature_url(
                    &resolved,
                    &wfs_box,
                    crs,
                    v2 && swap,
                    start,
                    layer.page_size,
                );
                let file = fetch_to_cache(
                    &make_request(&url, environment),
                    ".gml",
                    layer.cache_days,
                    environment,
                    &mut result.stats,
                    stop,
                )?;
                let head = head_of(&file, 512);
                if holds(&head, b"ExceptionReport") || holds(&head, b"ServiceException") {
                    remove_quietly(&file);
                    return Err(make_error(
                        ErrorCode::FileImportFailure,
                        "the WFS refused the request",
                        service_message(&head),
                    ));
                }
                result.stats.pages += 1;
                // An empty page comes back as an empty result (import_page);
                // a page GDAL cannot read is a failure, not the end of the
                // data.
                let imported = match import_page(&file, layer, options, None) {
                    Ok(imported) => imported,
                    Err(error) => {
                        remove_quietly(&file);
                        return Err(error);
                    }
                };
                let read = imported.features_read;
                let added = joiner.add(imported, &mut result.stats);
                if over_limit(&joiner) {
                    return Err(too_many());
                }
                if read == 0 {
                    break;
                }
                if !v2 {
                    // WFS 1.x has no paging: MAXFEATURES is all there is.
                    if read >= layer.page_size as u64 {
                        result.warnings.push(format!(
                            "the WFS (version {}) returned its {read} feature limit and cannot \
                             page, so there may be more; choose a smaller area",
                            layer.version
                        ));
                    }
                    break;
                }
                if added == 0 && page > 0 {
                    result.warnings.push(String::from(
                        "the WFS repeated a page instead of moving on; it may not support \
                         paging, so some features may be missing - try a smaller area",
                    ));
                    break;
                }
                // A server may cap its pages below COUNT, so a short page is
                // not the last: the next is asked for until one comes back
                // empty.
                start += read;
            }
        }
        OnlineServiceType::OgcApiFeatures => {
            let mut url = oapif_items_url(&resolved, &prepared.lon_lat, layer.page_size);
            let mut page: i32 = 0;
            while !url.is_empty() {
                if page >= environment.max_pages {
                    return Err(too_many());
                }
                report(&format!(
                    "Fetching page {} ({} features)",
                    page + 1,
                    joiner.size()
                ));
                let file = fetch_to_cache(
                    &make_request(&url, environment),
                    ".geojson",
                    layer.cache_days,
                    environment,
                    &mut result.stats,
                    stop,
                )?;
                // Parsed once, for the count and the next link together.
                let state = match read_items_page(read_whole(&file)) {
                    Ok(state) => state,
                    Err(error) => {
                        remove_quietly(&file);
                        return Err(error);
                    }
                };
                result.stats.pages += 1;
                if state.features == 0 {
                    break;
                }
                let imported = import_page(&file, layer, options, None)?;
                if joiner.add(imported, &mut result.stats) == 0 && page > 0 {
                    result.warnings.push(String::from(
                        "the service repeated a page; paging stopped there",
                    ));
                    break;
                }
                if over_limit(&joiner) {
                    return Err(too_many());
                }
                // Only links to the same service are followed: an answer
                // cannot send the importer somewhere else.
                url = if !state.next.is_empty()
                    && host_of(&state.next) == host_of(&prepared.endpoint)
                {
                    state.next
                } else {
                    String::new()
                };
                page += 1;
            }
        }
        OnlineServiceType::Overpass => {
            let area = area_km2(&prepared.lon_lat);
            if area > layer.max_area_km2 {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!(
                        "the area is {} km2; OpenStreetMap queries through the public Overpass \
                         API are limited to {} km2 - choose a smaller area",
                        format_number((area * 10.0).round() / 10.0),
                        format_number(layer.max_area_km2)
                    ),
                ));
            }
            let filter = if options.filter.is_empty() {
                layer.layer_name.as_str()
            } else {
                options.filter.as_str()
            };
            let query = overpass_query(
                filter,
                &prepared.lon_lat,
                layer.timeout_seconds,
                environment.max_response_bytes.min(512 << 20),
            )?;
            let request = HttpRequest {
                post_body: format!("data={}", percent_encode(&query)),
                post_content_type: String::from("application/x-www-form-urlencoded"),
                // The server may take its whole timeout; the transfer is given
                // a little longer so that its own answer, not ours, ends a
                // slow query.
                timeout_seconds: environment.timeout_seconds.max(layer.timeout_seconds + 30),
                ..make_request(&prepared.endpoint, environment)
            };
            report("Querying OpenStreetMap");
            let file = fetch_to_cache(
                &request,
                ".osm",
                layer.cache_days,
                environment,
                &mut result.stats,
                stop,
            )?;
            let head = head_of(&file, 512);
            if !holds(&head, b"<osm") {
                remove_quietly(&file);
                return Err(make_error(
                    ErrorCode::FileImportFailure,
                    "the Overpass API did not return OpenStreetMap data",
                    service_message(&head),
                ));
            }
            result.stats.pages += 1;
            report("Reading OpenStreetMap data");
            let imported = import_page(&file, layer, options, None)?;
            joiner.add(imported, &mut result.stats);
            if over_limit(&joiner) {
                return Err(too_many());
            }
        }
        OnlineServiceType::File => {
            let mut url = prepared.endpoint.clone();
            // Each "{layer}" from the start again, as the C++ searches.
            while let Some(at) = url.find("{layer}") {
                url.replace_range(at..at + 7, &layer.layer_name);
            }
            let zipped = lower(&url).ends_with(".zip");
            report(&format!("Downloading {}", layer.title));
            let file = fetch_to_cache(
                &make_request(&url, environment),
                &if zipped {
                    String::from(".shp.zip")
                } else {
                    extension_for("", &url)
                },
                layer.cache_days,
                environment,
                &mut result.stats,
                stop,
            )?;
            result.stats.pages += 1;
            report(&format!("Clipping {} to the area", layer.title));
            // The file covers far more than the area: its features are cut
            // at the area's edge (in the file's own CRS, WGS 84 for every
            // File layer the catalogue has) before they are read.
            let clipped = cache_path(
                &cache_root(environment),
                "products",
                &sha256_hex(format!(
                    "clip\n{}\n{},{},{},{}",
                    path_string(&file),
                    format_number(prepared.lon_lat.min_x),
                    format_number(prepared.lon_lat.min_y),
                    format_number(prepared.lon_lat.max_x),
                    format_number(prepared.lon_lat.max_y)
                )),
                ".gpkg",
            );
            if let Err(error) = clip_vector_file(&file, &prepared.lon_lat, &clipped) {
                // Not a vector file - an HTML page where a zip was expected:
                // not kept for the next run to trip on.
                remove_quietly(&file);
                return Err(error);
            }
            report(&format!("Reading {}", layer.title));
            let imported = import_page(&clipped, layer, options, Some(prepared.lon_lat))?;
            joiner.add(imported, &mut result.stats);
        }
        OnlineServiceType::Ckan => {
            return Err(Error::new(
                ErrorCode::Unsupported,
                format!(
                    "a catalogue finds services rather than holding data: search it with ONLINE \
                     LAYERS {} <words>, then add a result with ONLINE CUSTOM <url>",
                    layer.provider_id
                ),
            ));
        }
        _ => {
            return Err(Error::new(
                ErrorCode::Unsupported,
                format!("a {} service gives no vectors", to_string(layer.r#type)),
            ));
        }
    }

    let mut vectors = joiner.take();
    result.source_url = redact_url(&prepared.endpoint);
    result.licence.clone_from(&layer.licence);
    result.attribution.clone_from(&layer.attribution);
    result.retrieved = today();
    for entity in &mut vectors.entities {
        let metadata = &mut entity.metadata;
        metadata.insert(
            String::from("online.source"),
            PropertyValue::Text(result.source_url.clone()),
        );
        metadata.insert(
            String::from("online.licence"),
            PropertyValue::Text(result.licence.clone()),
        );
        metadata.insert(
            String::from("online.attribution"),
            PropertyValue::Text(result.attribution.clone()),
        );
        metadata.insert(
            String::from("online.retrieved"),
            PropertyValue::Text(result.retrieved.clone()),
        );
        // A page's cache file name says nothing to anyone: the service does.
        metadata.insert(
            String::from("source.file"),
            PropertyValue::Text(format!("{}/{}", layer.provider_id, layer.id)),
        );
    }
    if vectors.entities.is_empty() && vectors.layers_needed.is_empty() {
        vectors
            .layers_needed
            .push(if options.target_layer.is_empty() {
                format!("online/{}/{}", layer.provider_id, layer.id)
            } else {
                options.target_layer.clone()
            });
    }
    result.vectors = Some(vectors);
    Ok(result)
}

// ---- the public functions -----------------------------------------------------------------------

/// A discovered address with the value of its first secret-looking query
/// parameter (redact_url's list) replaced by {key}, and that value; the value
/// is empty when there is none.
pub fn extract_key(url: &str) -> (String, String) {
    let Some(query) = url.find('?') else {
        return (String::from(url), String::new());
    };
    let mut at = query + 1;
    while at < url.len() {
        let end = url[at..].find('&').map_or(url.len(), |found| found + at);
        let equals = url[at..].find('=').map(|found| found + at);
        if let Some(equals) = equals
            && equals < end
            && is_secret_parameter(&url[at..equals])
        {
            let value = &url[equals + 1..end];
            if !value.is_empty() && value != "{key}" {
                return (
                    format!("{}{{key}}{}", &url[..=equals], &url[end..]),
                    String::from(value),
                );
            }
        }
        at = end + 1;
    }
    (String::from(url), String::new())
}

/// The address a layer is fetched from with its {key} filled in. NotFound,
/// naming the key and the verb that stores it, when the layer needs a key the
/// environment does not have.
pub fn endpoint_with_key(layer: &OnlineLayer, environment: &OnlineEnvironment) -> Result<String> {
    let key = environment
        .keys
        .get(&layer.key_name)
        .filter(|key| !key.is_empty());
    let placeholder = layer.endpoint.contains("{key}");
    let key = match key {
        Some(key) => key.as_str(),
        None if layer.key_required || placeholder => {
            return Err(make_error(
                ErrorCode::NotFound,
                format!(
                    "{} needs a key; store yours with ONLINE KEY {} <key>",
                    layer.provider_title, layer.key_name
                ),
                format!("{}/{}", layer.provider_id, layer.id),
            ));
        }
        None => "",
    };
    let mut endpoint = layer.endpoint.clone();
    if placeholder {
        let encoded = percent_encode(key);
        let mut found = endpoint.find("{key}");
        while let Some(at) = found {
            endpoint.replace_range(at..at + 5, &encoded);
            let after = at + encoded.len();
            found = endpoint[after..].find("{key}").map(|next| next + after);
        }
    }
    Ok(endpoint)
}

/// A request's answer, from the cache when it holds a fresh one, else from
/// the network, kept in the cache for next time. The cache file is named by
/// the SHA-256 of the method, URL and body, so no two requests share a file
/// and no key appears in a file name; it is written to a temporary name and
/// renamed, so a cancelled or failed transfer never leaves a partial answer
/// that a later run would take for a whole one. Returns the file's path.
pub fn fetch_to_cache(
    request: &HttpRequest,
    extension: &str,
    max_age_days: i32,
    environment: &OnlineEnvironment,
    stats: &mut OnlineStats,
    stop: Option<&AtomicBool>,
) -> Result<PathBuf> {
    let key = cache_key(request);
    let path = cache_path(&cache_root(environment), "http", &key, extension);
    let caching = !environment.cache_directory.as_os_str().is_empty();
    if caching && fresh(&path, max_age_days, environment.offline) {
        stats.cache_hits += 1;
        return Ok(path);
    }
    if environment.offline {
        return Err(make_error(
            ErrorCode::NotFound,
            "working offline, and the cache has no copy of this",
            redact_url(&request.url),
        ));
    }
    if stopped(stop) {
        return Err(make_error(
            ErrorCode::InvalidState,
            "cancelled",
            redact_url(&request.url),
        ));
    }
    let response = match &environment.transport {
        Some(transport) => transport(request, stop),
        None => http_fetch(request, stop, None),
    }?;
    stats.requests += 1;
    stats.bytes += response.body.len() as u64;
    // A stop that arrived while the answer came in: the answer is whole and
    // is kept for next time, but nothing more is done with it now.
    if stopped(stop) {
        let _ = write_atomically(&path, &response.body);
        return Err(make_error(
            ErrorCode::InvalidState,
            "cancelled",
            redact_url(&request.url),
        ));
    }
    write_atomically(&path, &response.body)?;
    Ok(path)
}

/// The same, for text (a capabilities document, a JSON answer). The C++'s
/// text is the file's bytes; a String holds UTF-8, so bytes that are not are
/// read as U+FFFD (docs/rust.md, "Where Rust differs").
pub fn fetch_text(
    url: &str,
    environment: &OnlineEnvironment,
    stats: &mut OnlineStats,
    stop: Option<&AtomicBool>,
    max_age_days: i32,
) -> Result<String> {
    let file = fetch_to_cache(
        &make_request(url, environment),
        ".txt",
        max_age_days,
        environment,
        stats,
        stop,
    )?;
    Ok(String::from_utf8_lossy(&read_whole(&file)).into_owned())
}

/// A file of the cache, as pruning weighs it.
struct CacheEntry {
    written: SystemTime,
    size: u64,
    path: PathBuf,
}

/// Every regular file under `root`, walked as a recursive_directory_iterator
/// walks: into folders, not through a link to one, and stopping at the first
/// folder it cannot read.
fn cache_files(root: &Path) -> Vec<(PathBuf, fs::Metadata)> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(folder) = pending.pop() {
        let Ok(entries) = fs::read_dir(folder) else {
            break;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                return found;
            };
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                pending.push(path);
                continue;
            }
            if let Ok(metadata) = fs::metadata(&path)
                && metadata.is_file()
            {
                found.push((path, metadata));
            }
        }
    }
    found
}

/// Removes cache files older than `max_age_days`, and then the oldest until
/// the cache holds at most `max_bytes`. Returns the bytes removed.
pub fn prune_cache(cache_directory: &Path, max_age_days: i32, max_bytes: u64) -> u64 {
    if cache_directory.as_os_str().is_empty() || !cache_directory.is_dir() {
        return 0;
    }
    let mut entries: Vec<CacheEntry> = Vec::new();
    let mut removed: u64 = 0;
    let now = SystemTime::now();
    for folder in ["http", "products"] {
        let root = cache_directory.join(folder);
        if !root.is_dir() {
            continue;
        }
        for (path, metadata) in cache_files(&root) {
            let Ok(written) = metadata.modified() else {
                continue;
            };
            let size = metadata.len();
            if age_nanos(now, written) > DAY_NANOS * i128::from(max_age_days) {
                if fs::remove_file(path).is_ok() {
                    removed += size;
                }
                continue;
            }
            entries.push(CacheEntry {
                written,
                size,
                path,
            });
        }
    }
    entries.sort_by_key(|entry| entry.written);
    let mut total: u64 = entries.iter().map(|entry| entry.size).sum();
    for entry in &entries {
        if total <= max_bytes {
            break;
        }
        if fs::remove_file(&entry.path).is_ok() {
            removed += entry.size;
            total -= entry.size;
        }
    }
    removed
}

pub fn fetch_online_layer(
    layer: &OnlineLayer,
    options: &OnlineRequestOptions,
    environment: &OnlineEnvironment,
    stop: Option<&AtomicBool>,
    progress: Option<OnlineProgress<'_>>,
) -> Result<OnlineImport> {
    match layer.kind {
        OnlineLayerKind::Imagery | OnlineLayerKind::Elevation => {
            fetch_raster(layer, options, environment, stop, progress)
        }
        OnlineLayerKind::Vector | OnlineLayerKind::Catalogue => {
            fetch_vectors(layer, options, environment, stop, progress)
        }
    }
}

/// discover_service over the network with the environment's User-Agent,
/// timeouts and cache, and a COG's header read through GDAL.
pub fn discover_online(
    url: &str,
    environment: &OnlineEnvironment,
    stop: Option<&AtomicBool>,
) -> Result<OnlineProvider> {
    let stats = RefCell::new(OnlineStats::default());
    let fetch = |address: &str| -> Result<String> {
        // Always fresh: a description is small, discovery is rare, and an
        // error page kept for a day would make the service look broken.
        fetch_text(address, environment, &mut stats.borrow_mut(), stop, 0)
    };
    let probe = |address: &str| -> Result<OnlineProvider> {
        let raster = probe_raster(
            &vsicurl(address),
            &environment.user_agent,
            environment.timeout_seconds,
        )?;
        // The file's name, never its query: a token there would otherwise
        // become the title that is shown, saved and given to the raster.
        let path = &address[..address.find(['?', '#']).unwrap_or(address.len())];
        let slash = path.rfind('/').map_or(0, |slash| slash + 1);
        let name = String::from(&path[slash..]);
        let mut provider = OnlineProvider {
            id: custom_provider_id(address),
            title: name.clone(),
            group: String::from("Custom"),
            homepage: String::from(&path[..slash]),
            licence: String::from("not stated by the service - check the publisher's terms"),
            user_defined: true,
            ..OnlineProvider::default()
        };
        provider.attribution.clone_from(&provider.title);
        let mut service = OnlineService {
            id: String::from("cog"),
            title: name.clone(),
            r#type: OnlineServiceType::Cog,
            endpoint: String::from(address),
            layers: Vec::new(),
        };
        let mut layer = OnlineLayer {
            provider_id: provider.id.clone(),
            provider_title: provider.title.clone(),
            group: provider.group.clone(),
            service_id: service.id.clone(),
            service_title: service.title.clone(),
            id: String::from("cog"),
            title: format!(
                "{name} ({} x {}, {} band {})",
                raster.width, raster.height, raster.band_count, raster.data_type
            ),
            r#type: OnlineServiceType::Cog,
            endpoint: String::from(address),
            // One band that is not bytes is values: an elevation model.
            kind: if raster.band_count == 1 && raster.data_type != "Byte" {
                OnlineLayerKind::Elevation
            } else {
                OnlineLayerKind::Imagery
            },
            licence: provider.licence.clone(),
            attribution: provider.attribution.clone(),
            key_name: provider.id.clone(),
            user_defined: true,
            evidence: String::from("discovered from the COG's header"),
            ..OnlineLayer::default()
        };
        if let Some(bounds) = raster.lon_lat_bounds {
            layer.coverage = [bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y];
        }
        service.layers.push(layer);
        provider.services.push(service);
        Ok(provider)
    };
    discover_service(url, &fetch, Some(&probe))
}

/// A CKAN catalogue search (the data-gov-au provider's layer) for `words`.
pub fn search_catalogue(
    layer: &OnlineLayer,
    words: &str,
    environment: &OnlineEnvironment,
    stop: Option<&AtomicBool>,
) -> Result<Vec<CkanResource>> {
    let mut stats = OnlineStats::default();
    let body = fetch_text(
        &ckan_search_url(&layer.endpoint, words, 50),
        environment,
        &mut stats,
        stop,
        1,
    )?;
    parse_ckan_search(body)
}
