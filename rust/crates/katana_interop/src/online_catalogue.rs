//! The catalogue of public web services that GIS > Online Data and the ONLINE
//! verbs offer (docs/gis_online.md). The port of
//! include/katana/interop/online_catalogue.hpp and
//! src/katana_interop/online_catalogue.cpp.
//!
//! The catalogue is DATA, not code: resources/online/online_sources.json,
//! compiled into the program (`include_str!` here, `#embed` in the C++, of the
//! same committed file), lists providers, their services and each service's
//! layers, with everything a request and a record need - the service type, the
//! endpoint, the layer id, the native CRS, the WGS 84 coverage, the request and
//! page limits, the licence and the attribution to record, whether a key is
//! needed. A provider changing an address is an edit to that file, and a
//! person can add a service the program does not know - or replace a built-in
//! provider - with a catalogue of their own beside the settings, which is
//! merged over the built-in one by provider id.
//!
//! Inheritance keeps the file short and the parsed form flat: a provider sets
//! the licence, attribution and coverage its services share, a service the
//! CRS, type, endpoint and limits its layers share, and a layer overrides what
//! it needs. `parse_catalogue` resolves all of it, so every OnlineLayer carries
//! its complete description and nothing downstream walks the tree to find one.
//!
//! The JSON is read and written through katana_core::json, the translation of
//! the C++'s nlohmann, so a number, a member order and the written text are
//! the C++'s.

use std::collections::BTreeSet;

use katana_core::error::make_error;
use katana_core::json::{FromJson, Json, JsonResult};
use katana_core::text::lowered;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::EnumName;

/// The C++ `toString` overload set: `to_string(OnlineServiceType::Wms)`,
/// `to_string(OnlineLayerKind::Vector)`.
pub use katana_entity::entity::to_string;

/// How a service is asked for data. Each has its own request builder
/// (online_requests) and path through the fetcher (online_fetch).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OnlineServiceType {
    /// ArcGIS REST MapServer /export - a rendered image of a box
    ArcgisExport,
    /// ArcGIS REST ImageServer /exportImage - pixel values (elevation)
    ArcgisExportImage,
    /// ArcGIS REST FeatureServer or MapServer /<layer>/query - features, paged
    ArcgisQuery,
    /// OGC WMS GetMap
    Wms,
    /// OGC WMTS tiles, by a REST template or capabilities
    Wmts,
    /// OGC WCS GetCoverage - pixel values
    Wcs,
    /// OGC WFS GetFeature - features, paged
    Wfs,
    /// OGC API - Features /collections/{id}/items - features, paged by links
    OgcApiFeatures,
    /// {z}/{x}/{y} tiles on the Web Mercator grid
    XyzTiles,
    /// a STAC API search, then the matching items' COG assets
    Stac,
    /// a cloud-optimised GeoTIFF (or a tiled set of them) read over HTTP
    Cog,
    /// OpenStreetMap features by tag, through the Overpass API
    Overpass,
    /// one downloadable vector file (a zipped shapefile, a GeoJSON)
    File,
    /// a CKAN catalogue search that finds services, not data
    Ckan,
}

/// What a layer gives the drawing. Imagery and elevation become reference
/// rasters (elevation keeps its true values, so Surface From Raster can use
/// it); vectors become entities; a catalogue layer lists services.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OnlineLayerKind {
    Imagery,
    Elevation,
    Vector,
    Catalogue,
}

// The spellings the catalogue uses. One per type: a second spelling would be
// a second way of saying the same thing.
const TYPE_NAMES: [(OnlineServiceType, &str); 14] = [
    (OnlineServiceType::ArcgisExport, "arcgis-export"),
    (OnlineServiceType::ArcgisExportImage, "arcgis-exportimage"),
    (OnlineServiceType::ArcgisQuery, "arcgis-query"),
    (OnlineServiceType::Wms, "wms"),
    (OnlineServiceType::Wmts, "wmts"),
    (OnlineServiceType::Wcs, "wcs"),
    (OnlineServiceType::Wfs, "wfs"),
    (OnlineServiceType::OgcApiFeatures, "oapif"),
    (OnlineServiceType::XyzTiles, "xyz"),
    (OnlineServiceType::Stac, "stac"),
    (OnlineServiceType::Cog, "cog"),
    (OnlineServiceType::Overpass, "overpass"),
    (OnlineServiceType::File, "file"),
    (OnlineServiceType::Ckan, "ckan"),
];

const LAYER_KINDS: [OnlineLayerKind; 4] = [
    OnlineLayerKind::Imagery,
    OnlineLayerKind::Elevation,
    OnlineLayerKind::Vector,
    OnlineLayerKind::Catalogue,
];

/// The C++ `toString(OnlineServiceType)`.
impl EnumName for OnlineServiceType {
    fn name(self) -> &'static str {
        TYPE_NAMES
            .iter()
            .find(|(each, _)| *each == self)
            .map_or("unknown", |&(_, name)| name)
    }
}

/// The C++ `toString(OnlineLayerKind)`.
impl EnumName for OnlineLayerKind {
    fn name(self) -> &'static str {
        match self {
            OnlineLayerKind::Imagery => "imagery",
            OnlineLayerKind::Elevation => "elevation",
            OnlineLayerKind::Vector => "vector",
            OnlineLayerKind::Catalogue => "catalogue",
        }
    }
}

/// The type of that spelling, in any case; None for none.
pub fn service_type_from_string(text: &str) -> Option<OnlineServiceType> {
    let lower = lowered(text);
    TYPE_NAMES
        .iter()
        .find(|(_, name)| lower == name.as_bytes())
        .map(|(each, _)| *each)
}

/// The kind of that spelling, in any case; None for none.
pub fn layer_kind_from_string(text: &str) -> Option<OnlineLayerKind> {
    let lower = lowered(text);
    LAYER_KINDS
        .into_iter()
        .find(|kind| lower == kind.name().as_bytes())
}

/// A WGS 84 longitude/latitude box: west, south, east, north.
pub type LonLatBox = [f64; 4];

/// One layer with its service and provider resolved into it.
#[derive(Clone, Debug, PartialEq)]
pub struct OnlineLayer {
    pub provider_id: String,
    pub provider_title: String,
    /// "Australia/NSW", "Global": the tree the dialog shows
    pub group: String,
    pub service_id: String,
    pub service_title: String,
    /// unique within its provider; what the ONLINE verbs name
    pub id: String,
    pub title: String,
    pub kind: OnlineLayerKind,
    pub r#type: OnlineServiceType,
    /// The service's address. For XYZ, WMTS and COG services a template with
    /// {z} {x} {y} {layer} {time} {lat} {lon}; for the others the base URL the
    /// request builder appends to.
    pub endpoint: String,
    /// WMS 1.3.0, WFS 2.0.0, WCS 1.0.0
    pub version: String,
    /// the service's own id: ArcGIS layer, WMS layer, WFS type, collection, tag
    pub layer_name: String,
    /// the CRS requests are made in; empty: the service's own (EPSG:4326 for
    /// queries)
    pub crs: String,
    pub coverage: LonLatBox,
    /// The largest image one request may ask for, each side, in pixels; a
    /// larger area is split into several requests.
    pub max_request_pixels: i32,
    /// Features asked for per request; the paging step.
    pub page_size: i32,
    /// Tiles: the finest zoom the service has, and the most tiles one import
    /// may fetch (the OpenStreetMap tile policy's "small areas only").
    pub max_zoom: i32,
    pub max_tiles: i32,
    /// The ground resolution of the data, metres, when it has one; the
    /// default for an elevation import.
    pub resolution: Option<f64>,
    /// Days a cached answer stays fresh (the OSM tile policy asks for at
    /// least 7).
    pub cache_days: i32,
    /// Overpass: the largest area, square kilometres, and the server-side
    /// timeout.
    pub max_area_km2: f64,
    pub timeout_seconds: i32,
    /// STAC: the default cloud ceiling (percent) and look-back (days); the
    /// asset(s).
    pub max_cloud: f64,
    pub days: i32,
    pub asset: String,
    /// the attribute that identifies a feature across pages
    pub id_field: String,
    /// WMTS/WMS time dimension; "default" for the latest
    pub time: String,
    /// "degree" for the one-degree COG tiles of Copernicus
    pub tiling: String,
    pub licence: String,
    pub attribution: String,
    pub homepage: String,
    pub key_required: bool,
    /// The name the key is stored under (ONLINE KEY <name> <value>); the
    /// provider id unless the catalogue says otherwise.
    pub key_name: String,
    /// the date it answered Katana's own requests; empty: documented only
    pub verified: String,
    /// where the address and the terms were checked
    pub evidence: String,
    pub user_defined: bool,
}

impl Default for OnlineLayer {
    fn default() -> Self {
        Self {
            provider_id: String::new(),
            provider_title: String::new(),
            group: String::new(),
            service_id: String::new(),
            service_title: String::new(),
            id: String::new(),
            title: String::new(),
            kind: OnlineLayerKind::Imagery,
            r#type: OnlineServiceType::XyzTiles,
            endpoint: String::new(),
            version: String::new(),
            layer_name: String::new(),
            crs: String::new(),
            coverage: [-180.0, -90.0, 180.0, 90.0],
            max_request_pixels: 2048,
            page_size: 1000,
            max_zoom: 19,
            max_tiles: 256,
            resolution: None,
            cache_days: 30,
            max_area_km2: 25.0,
            timeout_seconds: 90,
            max_cloud: 20.0,
            days: 90,
            asset: String::new(),
            id_field: String::new(),
            time: String::new(),
            tiling: String::new(),
            licence: String::new(),
            attribution: String::new(),
            homepage: String::new(),
            key_required: false,
            key_name: String::new(),
            verified: String::new(),
            evidence: String::new(),
            user_defined: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OnlineService {
    pub id: String,
    pub title: String,
    pub r#type: OnlineServiceType,
    pub endpoint: String,
    pub layers: Vec<OnlineLayer>,
}

impl Default for OnlineService {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            r#type: OnlineServiceType::XyzTiles,
            endpoint: String::new(),
            layers: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OnlineProvider {
    pub id: String,
    pub title: String,
    pub group: String,
    pub homepage: String,
    pub licence: String,
    pub attribution: String,
    pub user_defined: bool,
    pub services: Vec<OnlineService>,
}

impl OnlineProvider {
    /// Every layer of every service, in order.
    pub fn layers(&self) -> Vec<&OnlineLayer> {
        self.services
            .iter()
            .flat_map(|service| service.layers.iter())
            .collect()
    }

    /// The layer of that id, in any case; None for none.
    pub fn find_layer(&self, layer_id: &str) -> Option<&OnlineLayer> {
        self.layers()
            .into_iter()
            .find(|layer| layer.id.eq_ignore_ascii_case(layer_id))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OnlineCatalogue {
    pub providers: Vec<OnlineProvider>,
}

impl OnlineCatalogue {
    /// The provider of that id, in any case; None for none.
    pub fn find_provider(&self, provider_id: &str) -> Option<&OnlineProvider> {
        self.providers
            .iter()
            .find(|provider| provider.id.eq_ignore_ascii_case(provider_id))
    }

    /// Providers whose id, title or group, or one of whose layers' ids or
    /// titles, contains every word of `text` (case-insensitive). An empty
    /// filter matches everything. In catalogue order.
    pub fn filter(&self, text: &str) -> Vec<&OnlineProvider> {
        let lower = text.to_ascii_lowercase();
        let words: Vec<&str> = lower
            .split([' ', '\t'])
            .filter(|word| !word.is_empty())
            .collect();
        let mut out = Vec::new();
        for provider in &self.providers {
            let mut haystack = format!("{} {} {}", provider.id, provider.title, provider.group);
            for layer in provider.layers() {
                haystack.push_str(&format!(
                    " {} {} {}",
                    layer.id,
                    layer.title,
                    layer.kind.name()
                ));
            }
            if words
                .iter()
                .all(|needle| contains_folded(&haystack, needle))
            {
                out.push(provider);
            }
        }
        out
    }
}

// ---- reading ------------------------------------------------------------------------------

// A value looked up on the layer, then its service, then its provider: the
// inheritance the file format promises.
struct Scope<'a> {
    chain: [&'a Json; 3],
}

impl<'a> Scope<'a> {
    fn new(provider: &'a Json, service: &'a Json, layer: &'a Json) -> Self {
        Self {
            chain: [layer, service, provider],
        }
    }

    fn find(&self, key: &str) -> Option<&'a Json> {
        self.chain
            .into_iter()
            .filter(|level| level.is_object())
            .find_map(|level| level.find(key).filter(|value| !value.is_null()))
    }

    fn text(&self, key: &str, fallback: &str) -> String {
        self.find(key)
            .and_then(Json::as_str)
            .map_or_else(|| String::from(fallback), String::from)
    }

    fn number<T: FromJson>(&self, key: &str, fallback: T) -> T {
        match self.find(key) {
            Some(value) if value.is_number() => value.get::<T>().unwrap_or(fallback),
            _ => fallback,
        }
    }
}

// "provider" or "provider/layer": where a problem is.
fn where_text(provider: &str, layer: &str) -> String {
    if layer.is_empty() {
        String::from(provider)
    } else {
        format!("{provider}/{layer}")
    }
}

fn is_http_url(url: &str) -> bool {
    let bytes = url.as_bytes();
    let lower = lowered(&bytes[..bytes.len().min(8)]);
    let start = if lower.starts_with(b"https://") {
        8
    } else if lower.starts_with(b"http://") {
        7
    } else {
        0
    };
    if start == 0 || bytes.len() <= start {
        return false;
    }
    // A host of at least one character before any path, and no whitespace
    // anywhere: a URL a person could paste, not a sentence.
    let host_end = bytes[start..]
        .iter()
        .position(|byte| matches!(byte, b'/' | b'?' | b'#'));
    if host_end == Some(0) {
        return false;
    }
    !bytes
        .iter()
        .any(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
}

fn valid_lon_lat(lon_lat: &LonLatBox) -> bool {
    lon_lat.iter().all(|value| value.is_finite())
        && lon_lat[0] >= -180.0
        && lon_lat[2] <= 180.0
        && lon_lat[1] >= -90.0
        && lon_lat[3] <= 90.0
        && lon_lat[0] < lon_lat[2]
        && lon_lat[1] < lon_lat[3]
}

fn contains_folded(haystack: &str, needle: &str) -> bool {
    haystack.to_ascii_lowercase().contains(needle)
}

fn parse_failure(
    message: impl Into<String>,
    context: impl Into<String>,
) -> Result<OnlineCatalogue> {
    Err(make_error(ErrorCode::ParseFailure, message, context))
}

// What parse_catalogue reads, nlohmann's exceptions apart: Err is the
// type_error value() throws for a member of another type than its default's
// ("id": 7), which the C++ lets out of parseCatalogue uncaught.
fn read_catalogue(text: &[u8], user_defined: bool) -> JsonResult<Result<OnlineCatalogue>> {
    let document = match Json::parse(text) {
        Ok(document) => document,
        Err(error) => {
            return Ok(parse_failure(
                "the online catalogue is not valid JSON",
                error.what,
            ));
        }
    };
    let providers = match document.find("providers") {
        Some(providers) if providers.is_array() => providers,
        _ => {
            return Ok(Err(Error::new(
                ErrorCode::ParseFailure,
                "the online catalogue has no \"providers\" list",
            )));
        }
    };
    let mut catalogue = OnlineCatalogue::default();
    let empty = Json::object();
    for provider_json in providers.iter() {
        if !provider_json.is_object() {
            return Ok(Err(Error::new(
                ErrorCode::ParseFailure,
                "a provider is not an object",
            )));
        }
        let id = provider_json.value("id", String::new())?;
        let title = provider_json.value("title", id.clone())?;
        let group = provider_json.value("group", String::from("Other"))?;
        let homepage = provider_json.value("homepage", String::new())?;
        let licence = provider_json.value("licence", String::new())?;
        let attribution = provider_json.value("attribution", String::new())?;
        let mut provider = OnlineProvider {
            id,
            title,
            group,
            homepage,
            licence,
            attribution,
            user_defined,
            services: Vec::new(),
        };
        if provider.id.is_empty() {
            return Ok(parse_failure("a provider has no id", provider.title));
        }
        let services = provider_json.find("services").unwrap_or(&empty);
        if !services.is_array() {
            return Ok(parse_failure(
                "a provider's \"services\" is not a list",
                provider.id,
            ));
        }
        for service_json in services.iter() {
            if !service_json.is_object() {
                return Ok(parse_failure("a service is not an object", provider.id));
            }
            let id = service_json.value("id", String::new())?;
            let title = service_json.value("title", id.clone())?;
            let endpoint = service_json.value("endpoint", String::new())?;
            let type_text = service_json.value("type", String::new())?;
            let Some(service_type) = service_type_from_string(&type_text) else {
                return Ok(parse_failure(
                    format!("unknown service type \"{type_text}\""),
                    where_text(&provider.id, &id),
                ));
            };
            let mut service = OnlineService {
                id,
                title,
                r#type: service_type,
                endpoint,
                layers: Vec::new(),
            };
            let layers = service_json.find("layers").unwrap_or(&empty);
            if !layers.is_array() || layers.empty() {
                return Ok(parse_failure(
                    "a service lists no layers",
                    where_text(&provider.id, &service.id),
                ));
            }
            for layer_json in layers.iter() {
                if !layer_json.is_object() {
                    return Ok(parse_failure(
                        "a layer is not an object",
                        where_text(&provider.id, &service.id),
                    ));
                }
                let scope = Scope::new(provider_json, service_json, layer_json);
                let defaults = OnlineLayer::default();
                let id = layer_json.value("id", String::new())?;
                let title = layer_json.value("title", id.clone())?;
                let kind_text = layer_json.value("kind", String::new())?;
                let Some(kind) = layer_kind_from_string(&kind_text) else {
                    return Ok(parse_failure(
                        format!("unknown layer kind \"{kind_text}\""),
                        where_text(&provider.id, &id),
                    ));
                };
                let version = scope.text("version", "");
                let layer_name = layer_json.value("layer", String::new())?;
                let crs = scope.text("crs", "");
                let mut coverage = defaults.coverage;
                if let Some(given) = scope.find("coverage") {
                    if !given.is_array() || given.size() != 4 {
                        return Ok(parse_failure(
                            "coverage is not [west, south, east, north]",
                            where_text(&provider.id, &id),
                        ));
                    }
                    for (i, side) in coverage.iter_mut().enumerate() {
                        let value = &given[i];
                        if !value.is_number() {
                            return Ok(parse_failure(
                                "coverage holds something other than numbers",
                                where_text(&provider.id, &id),
                            ));
                        }
                        *side = value.get::<f64>()?;
                    }
                }
                let max_request_pixels =
                    scope.number("maxRequestPixels", defaults.max_request_pixels);
                let page_size = scope.number("pageSize", defaults.page_size);
                let max_zoom = scope.number("maxZoom", defaults.max_zoom);
                let max_tiles = scope.number("maxTiles", defaults.max_tiles);
                let resolution = match scope.find("resolution") {
                    Some(value) if value.is_number() => Some(value.get::<f64>()?),
                    _ => None,
                };
                let cache_days = scope.number("cacheDays", defaults.cache_days);
                let max_area_km2 = scope.number("maxArea", defaults.max_area_km2);
                let timeout_seconds = scope.number("timeout", defaults.timeout_seconds);
                let max_cloud = scope.number("maxCloud", defaults.max_cloud);
                let days = scope.number("days", defaults.days);
                let key_required = match scope.find("keyRequired") {
                    Some(value) if value.is_boolean() => value.get::<bool>()?,
                    _ => defaults.key_required,
                };
                service.layers.push(OnlineLayer {
                    provider_id: provider.id.clone(),
                    provider_title: provider.title.clone(),
                    group: provider.group.clone(),
                    service_id: service.id.clone(),
                    service_title: service.title.clone(),
                    id,
                    title,
                    kind,
                    r#type: service.r#type,
                    endpoint: service.endpoint.clone(),
                    version,
                    layer_name,
                    crs,
                    coverage,
                    max_request_pixels,
                    page_size,
                    max_zoom,
                    max_tiles,
                    resolution,
                    cache_days,
                    max_area_km2,
                    timeout_seconds,
                    max_cloud,
                    days,
                    asset: scope.text("asset", ""),
                    id_field: scope.text("idField", ""),
                    time: scope.text("time", ""),
                    tiling: scope.text("tiling", ""),
                    licence: scope.text("licence", ""),
                    attribution: scope.text("attribution", ""),
                    homepage: scope.text("homepage", ""),
                    key_required,
                    key_name: scope.text("keyName", &provider.id),
                    verified: scope.text("verified", ""),
                    evidence: scope.text("evidence", ""),
                    user_defined,
                });
            }
            provider.services.push(service);
        }
        catalogue.providers.push(provider);
    }
    Ok(Ok(catalogue))
}

/// Parses a catalogue document. `user_defined` marks every provider in it as
/// the person's own (the dialog says so, and ONLINE CUSTOM writes there).
/// ParseFailure naming the provider, service and field for a document that is
/// not JSON or lacks something a request needs; validate_catalogue is the
/// fuller check.
///
/// A member of another JSON type than the text it should be (`"id": 7`) is
/// nlohmann's type_error, which the C++ lets out of parseCatalogue uncaught
/// (a defect, docs/rust.md "Not done"). Ported as it is: a panic carrying the
/// exception's what().
pub fn parse_catalogue(json: impl AsRef<[u8]>, user_defined: bool) -> Result<OnlineCatalogue> {
    match read_catalogue(json.as_ref(), user_defined) {
        Ok(answer) => answer,
        Err(error) => panic!("{}", error.what),
    }
}

// The built-in catalogue, embedded by the compiler from the file the C++
// embeds with #embed, so both programs carry the same bytes.
static BUILT_IN_CATALOGUE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../resources/online/online_sources.json"
));

/// The catalogue compiled into the program.
pub fn built_in_catalogue_json() -> &'static str {
    BUILT_IN_CATALOGUE
}

pub fn built_in_catalogue() -> Result<OnlineCatalogue> {
    parse_catalogue(built_in_catalogue_json(), false)
}

/// `overlay` merged over `base`: a provider with an id `base` has replaces it
/// in place, a new one is appended.
pub fn merge_catalogues(mut base: OnlineCatalogue, overlay: &OnlineCatalogue) -> OnlineCatalogue {
    for provider in &overlay.providers {
        match base
            .providers
            .iter()
            .position(|existing| existing.id.eq_ignore_ascii_case(&provider.id))
        {
            Some(at) => base.providers[at] = provider.clone(),
            None => base.providers.push(provider.clone()),
        }
    }
    base
}

// ---- validation ---------------------------------------------------------------------------

// The problem of an endpoint that lacks a placeholder its type needs.
fn needs(problems: &mut Vec<String>, at: &str, layer: &OnlineLayer, placeholder: &str) {
    if !layer.endpoint.contains(placeholder) {
        problems.push(format!(
            "{at}: the {} endpoint has no {placeholder}",
            layer.r#type.name()
        ));
    }
}

/// Everything wrong with a catalogue, one sentence per problem, each naming
/// provider/layer: a missing title, licence or attribution, an endpoint that
/// is not an https or http URL (or a template missing a placeholder its type
/// needs), a coverage box that is not a valid longitude/latitude box, a
/// duplicate id, a limit out of range. Empty when it is sound.
pub fn validate_catalogue(catalogue: &OnlineCatalogue) -> Vec<String> {
    let mut problems = Vec::new();
    let mut provider_ids = BTreeSet::new();
    for provider in &catalogue.providers {
        if !provider_ids.insert(provider.id.to_ascii_lowercase()) {
            problems.push(format!("{}: the provider id is used twice", provider.id));
        }
        if provider.title.is_empty() {
            problems.push(format!("{}: no title", provider.id));
        }
        if provider.group.is_empty() {
            problems.push(format!("{}: no group", provider.id));
        }
        let mut layer_ids = BTreeSet::new();
        for layer in provider.layers() {
            let at = where_text(&provider.id, &layer.id);
            if layer.id.is_empty() {
                problems.push(format!("{}: a layer has no id", provider.id));
            } else if !layer_ids.insert(layer.id.to_ascii_lowercase()) {
                problems.push(format!("{at}: the layer id is used twice in this provider"));
            }
            if layer.id.contains([' ', '\t', '/', '"']) {
                problems.push(format!(
                    "{at}: a layer id may not hold spaces, quotes or slashes"
                ));
            }
            if layer.title.is_empty() {
                problems.push(format!("{at}: no title"));
            }
            if layer.licence.is_empty() {
                problems.push(format!("{at}: no licence"));
            }
            if layer.attribution.is_empty() {
                problems.push(format!("{at}: no attribution to record"));
            }
            if !is_http_url(&layer.endpoint) {
                problems.push(format!("{at}: the endpoint is not an http or https URL"));
            }
            if !valid_lon_lat(&layer.coverage) {
                problems.push(format!(
                    "{at}: the coverage is not a valid longitude/latitude box"
                ));
            }
            match layer.r#type {
                OnlineServiceType::XyzTiles | OnlineServiceType::Wmts => {
                    needs(&mut problems, &at, layer, "{z}");
                    needs(&mut problems, &at, layer, "{x}");
                    needs(&mut problems, &at, layer, "{y}");
                    if layer.crs != "EPSG:3857" {
                        problems.push(format!(
                            "{at}: tile templates are on the Web Mercator grid, so the crs \
                             must be EPSG:3857"
                        ));
                    }
                    if !(0..=24).contains(&layer.max_zoom) {
                        problems.push(format!("{at}: maxZoom is outside 0 to 24"));
                    }
                    if !(1..=4096).contains(&layer.max_tiles) {
                        problems.push(format!("{at}: maxTiles is outside 1 to 4096"));
                    }
                }
                OnlineServiceType::Cog => {
                    if layer.tiling == "degree" {
                        needs(&mut problems, &at, layer, "{lat}");
                        needs(&mut problems, &at, layer, "{lon}");
                    }
                }
                OnlineServiceType::ArcgisQuery
                | OnlineServiceType::Wfs
                | OnlineServiceType::OgcApiFeatures => {
                    if !(1..=10000).contains(&layer.page_size) {
                        problems.push(format!("{at}: pageSize is outside 1 to 10000"));
                    }
                    if layer.r#type == OnlineServiceType::ArcgisQuery && layer.layer_name.is_empty()
                    {
                        problems.push(format!("{at}: an ArcGIS query needs the layer id"));
                    }
                }
                OnlineServiceType::ArcgisExport
                | OnlineServiceType::ArcgisExportImage
                | OnlineServiceType::Wms
                | OnlineServiceType::Wcs => {
                    if !(64..=16384).contains(&layer.max_request_pixels) {
                        problems.push(format!("{at}: maxRequestPixels is outside 64 to 16384"));
                    }
                    if (layer.r#type == OnlineServiceType::Wms
                        || layer.r#type == OnlineServiceType::Wcs)
                        && layer.layer_name.is_empty()
                    {
                        problems.push(format!(
                            "{at}: a {} layer needs the service's layer name",
                            layer.r#type.name()
                        ));
                    }
                }
                OnlineServiceType::File => {
                    if layer.endpoint.contains("{layer}") && layer.layer_name.is_empty() {
                        problems.push(format!(
                            "{at}: the file endpoint names {{layer}} but the layer gives none"
                        ));
                    }
                }
                OnlineServiceType::Stac => {
                    if layer.layer_name.is_empty() {
                        problems.push(format!("{at}: a STAC layer needs its collection"));
                    }
                    if layer.asset.is_empty() {
                        problems.push(format!("{at}: a STAC layer needs the asset(s) to read"));
                    }
                }
                OnlineServiceType::Overpass | OnlineServiceType::Ckan => {}
            }
            let image_kind =
                layer.kind == OnlineLayerKind::Imagery || layer.kind == OnlineLayerKind::Elevation;
            let image_service = layer.r#type != OnlineServiceType::ArcgisQuery
                && layer.r#type != OnlineServiceType::Wfs
                && layer.r#type != OnlineServiceType::OgcApiFeatures
                && layer.r#type != OnlineServiceType::Overpass
                && layer.r#type != OnlineServiceType::File
                && layer.r#type != OnlineServiceType::Ckan;
            if image_kind != image_service && layer.kind != OnlineLayerKind::Catalogue {
                problems.push(format!(
                    "{at}: a {} layer cannot come from a {} service",
                    layer.kind.name(),
                    layer.r#type.name()
                ));
            }
            if let Some(resolution) = layer.resolution
                && !(resolution > 0.0)
            {
                problems.push(format!("{at}: the resolution must be positive"));
            }
            if layer.cache_days < 0 {
                problems.push(format!("{at}: cacheDays is negative"));
            }
        }
    }
    problems
}

// ---- writing ------------------------------------------------------------------------------

fn coverage_json(coverage: &LonLatBox) -> Json {
    Json::array_of(coverage.iter().map(|&side| Json::from(side)))
}

fn layer_to_json(layer: &OnlineLayer) -> Json {
    let mut out = Json::object();
    out["id"] = Json::from(&layer.id);
    out["title"] = Json::from(&layer.title);
    out["kind"] = Json::from(layer.kind.name());
    if !layer.layer_name.is_empty() {
        out["layer"] = Json::from(&layer.layer_name);
    }
    if !layer.id_field.is_empty() {
        out["idField"] = Json::from(&layer.id_field);
    }
    if !layer.asset.is_empty() {
        out["asset"] = Json::from(&layer.asset);
    }
    out
}

/// One provider written as catalogue JSON, as ONLINE CUSTOM saves a
/// discovered service to the user catalogue; parse_catalogue reads it back to
/// the same layers.
pub fn provider_to_json(provider: &OnlineProvider) -> String {
    let one = OnlineCatalogue {
        providers: vec![provider.clone()],
    };
    catalogue_to_json(&one)
}

pub fn catalogue_to_json(catalogue: &OnlineCatalogue) -> String {
    let mut document = Json::object();
    document["version"] = Json::from(1_i32);
    let mut providers = Vec::new();
    for provider in &catalogue.providers {
        let mut p = Json::object();
        p["id"] = Json::from(&provider.id);
        p["title"] = Json::from(&provider.title);
        p["group"] = Json::from(&provider.group);
        if !provider.homepage.is_empty() {
            p["homepage"] = Json::from(&provider.homepage);
        }
        p["licence"] = Json::from(&provider.licence);
        p["attribution"] = Json::from(&provider.attribution);
        let mut services = Vec::new();
        for service in &provider.services {
            let mut s = Json::object();
            s["id"] = Json::from(&service.id);
            s["title"] = Json::from(&service.title);
            s["type"] = Json::from(service.r#type.name());
            s["endpoint"] = Json::from(&service.endpoint);
            // What every layer of a service shares is written on the
            // service; a discovered service's layers share all of it.
            if let Some(first) = service.layers.first() {
                if !first.version.is_empty() {
                    s["version"] = Json::from(&first.version);
                }
                if !first.crs.is_empty() {
                    s["crs"] = Json::from(&first.crs);
                }
                s["coverage"] = coverage_json(&first.coverage);
                s["maxRequestPixels"] = Json::from(first.max_request_pixels);
                s["pageSize"] = Json::from(first.page_size);
                s["maxZoom"] = Json::from(first.max_zoom);
                s["maxTiles"] = Json::from(first.max_tiles);
                if first.licence != provider.licence {
                    s["licence"] = Json::from(&first.licence);
                }
                if first.attribution != provider.attribution {
                    s["attribution"] = Json::from(&first.attribution);
                }
                if first.key_required {
                    s["keyRequired"] = Json::from(true);
                    s["keyName"] = Json::from(&first.key_name);
                }
                if !first.evidence.is_empty() {
                    s["evidence"] = Json::from(&first.evidence);
                }
            }
            let mut layers = Vec::new();
            for layer in &service.layers {
                let mut l = layer_to_json(layer);
                // Within the loop the service has a first layer.
                let first = &service.layers[0];
                if layer.coverage != first.coverage {
                    l["coverage"] = coverage_json(&layer.coverage);
                }
                if !layer.crs.is_empty() && layer.crs != first.crs {
                    l["crs"] = Json::from(&layer.crs);
                }
                layers.push(l);
            }
            s["layers"] = Json::from(layers);
            services.push(s);
        }
        p["services"] = Json::from(services);
        providers.push(p);
    }
    document["providers"] = Json::from(providers);
    document.dump_with(1, ' ', false)
}
