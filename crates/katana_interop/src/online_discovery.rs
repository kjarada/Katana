//! What a web service offers, found from its own description: GIS > Online
//! Data > Add Custom Service... and the ONLINE CUSTOM verb (docs/gis_online.md,
//! "Custom services"). The port of include/katana/interop/online_discovery.hpp
//! and src/katana_interop/online_discovery.cpp.
//!
//! A person pastes a URL; Katana asks the service to describe itself - a WMS,
//! WMTS or WFS GetCapabilities document, an ArcGIS REST service's JSON, an OGC
//! API's collections, a STAC API's collections or a single STAC item, a COG's
//! header - and turns the answer into an OnlineProvider with one layer per
//! thing it can import, in the same form the built-in catalogue has. From
//! there a custom layer is imported exactly as a built-in one is.
//!
//! The parsers are pure functions of the document, tested on committed
//! capabilities fixtures (tests/interop/data/online/); `discover_service` adds
//! only the fetching, through a function the caller supplies, so the tests run
//! it over file:// URLs.
//!
//! XML is read with GDAL's own parser (katana_gis::web_access::parse_xml) and
//! JSON with katana_core::json under nlohmann's rules; a type_error the C++
//! lets out uncaught is a panic carrying its what() (online_requests says
//! why).

use std::collections::{BTreeMap, BTreeSet};

use katana_core::error::make_error;
use katana_core::json::{Json, JsonResult};
use katana_core::text::parse_finite_double;
use katana_core::{Error, ErrorCode, Result};
use katana_gis::reproject::{CrsBox, transform_box};
use katana_gis::web_access::{XmlElement, parse_xml, percent_encode, redact_url, sha256_hex};
use katana_math::cpp;

use crate::online_catalogue::{
    LonLatBox, OnlineLayer, OnlineLayerKind, OnlineProvider, OnlineService, OnlineServiceType,
    to_string,
};
use crate::online_requests::{
    byte_prefix, parse_stac_items, replace_all, trimmed_text, uncaught,
    web_mercator_tile_matrix_set, with_query,
};

/// What a discovered service's layers say about their terms until the person
/// checks: the service did not declare a licence Katana can read, and saying
/// nothing would record an import as though it had none.
const UNKNOWN_LICENCE: &str = "not stated by the service - check the publisher's terms";

/// Fetches text, for discovery: a URL in, the body out. online_fetch's
/// fetch_text is the one used in the program, with its cache and User-Agent.
/// A borrow, used only during `discover_service`.
pub type TextFetcher<'a> = &'a dyn Fn(&str) -> Result<String>;

/// Reads a COG's header as its description (katana_gis::reproject::probe_raster
/// over /vsicurl/). A borrow, used only during `discover_service`.
pub type CogProbe<'a> = &'a dyn Fn(&str) -> Result<OnlineProvider>;

fn lower(text: &str) -> String {
    text.to_ascii_lowercase()
}

// The URL without its query and fragment: the base a request builder appends
// its own parameters to.
fn base_of(url: &str) -> &str {
    match url.find(['?', '#']) {
        Some(cut) => &url[..cut],
        None => url,
    }
}

// The URL without the OGC request's own parameters (SERVICE, REQUEST, VERSION
// and the like) but with everything else: a MapServer's map=, a vendor's own
// switches, which every request needs. A key parameter stays; the user
// catalogue's writer moves it to the settings.
fn ogc_base_of(url: &str) -> String {
    let Some(query) = url.find('?') else {
        return String::from(url);
    };
    const OWN: [&str; 13] = [
        "service",
        "request",
        "version",
        "acceptversions",
        "sections",
        "updatesequence",
        "acceptformats",
        "layers",
        "typename",
        "typenames",
        "coverage",
        "format",
        "outputformat",
    ];
    let start = query + 1;
    let end = url[query..]
        .find('#')
        .map_or(url.len(), |found| found + query);
    let mut kept = String::new();
    for pair in url[start..end].split('&') {
        let name = lower(&pair[..pair.find('=').unwrap_or(pair.len())]);
        if !pair.is_empty() && !OWN.contains(&name.as_str()) {
            if !kept.is_empty() {
                kept.push('&');
            }
            kept.push_str(pair);
        }
    }
    if kept.is_empty() {
        String::from(&url[..query])
    } else {
        format!("{}?{kept}", &url[..query])
    }
}

fn host_of(url: &str) -> &str {
    let Some(scheme) = url.find("://") else {
        return "";
    };
    let start = scheme + 3;
    match url[start..].find(['/', '?', '#', ':']) {
        Some(end) => &url[start..start + end],
        None => &url[start..],
    }
}

// An id made of letters, digits and dashes, from any text: what a layer id and
// a provider id may hold (validate_catalogue refuses spaces, quotes and
// slashes, because the verbs split on them).
fn safe_id(text: &str, limit: usize) -> String {
    let mut id = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_lowercase() || byte.is_ascii_digit() {
            id.push(char::from(byte));
        } else if byte.is_ascii_uppercase() {
            id.push(char::from(byte.to_ascii_lowercase()));
        } else if !id.is_empty() && !id.ends_with('-') {
            id.push('-');
        }
        if id.len() >= limit {
            break;
        }
    }
    while id.ends_with('-') {
        id.pop();
    }
    if id.is_empty() {
        String::from("layer")
    } else {
        id
    }
}

/// safe_id's limit for a layer's id.
const LAYER_ID_LIMIT: usize = 48;

// Ids unique within the provider: a second "roads" becomes "roads-2".
fn unique_id(used: &mut BTreeSet<String>, wanted: &str) -> String {
    let mut id = String::from(wanted);
    let mut copy = 2;
    while !used.insert(id.clone()) {
        id = format!("{wanted}-{copy}");
        copy += 1;
    }
    id
}

fn lon_lat_from_corners(lower_corner: &str, upper_corner: &str) -> Option<LonLatBox> {
    let pair = |text: &str| -> Option<[f64; 2]> {
        let view = trimmed_text(text);
        let space = view.find([' ', '\t'])?;
        let a = parse_finite_double(trimmed_text(&view[..space]))?;
        let b = parse_finite_double(trimmed_text(&view[space..]))?;
        Some([a, b])
    };
    let low = pair(lower_corner)?;
    let high = pair(upper_corner)?;
    Some([low[0], low[1], high[0], high[1]])
}

fn sane(lon_lat: &LonLatBox) -> bool {
    lon_lat[0] >= -180.0
        && lon_lat[2] <= 180.0
        && lon_lat[1] >= -90.0
        && lon_lat[3] <= 90.0
        && lon_lat[0] < lon_lat[2]
        && lon_lat[1] < lon_lat[3]
}

fn make_provider(url: &str, title: &str) -> OnlineProvider {
    let title = if title.is_empty() {
        String::from(host_of(url))
    } else {
        String::from(title)
    };
    OnlineProvider {
        id: custom_provider_id(url),
        attribution: title.clone(),
        title,
        group: String::from("Custom"),
        homepage: String::from(base_of(url)),
        licence: String::from(UNKNOWN_LICENCE),
        user_defined: true,
        services: Vec::new(),
    }
}

fn base_layer(provider: &OnlineProvider, service: &OnlineService) -> OnlineLayer {
    OnlineLayer {
        provider_id: provider.id.clone(),
        provider_title: provider.title.clone(),
        group: provider.group.clone(),
        service_id: service.id.clone(),
        service_title: service.title.clone(),
        r#type: service.r#type,
        endpoint: service.endpoint.clone(),
        licence: provider.licence.clone(),
        attribution: provider.attribution.clone(),
        homepage: provider.homepage.clone(),
        key_name: provider.id.clone(),
        user_defined: true,
        evidence: format!("discovered from {}", redact_url(&provider.homepage)),
        ..OnlineLayer::default()
    }
}

// The licence a capabilities document declares in its AccessConstraints (or
// Fees), when it says anything but "none": services write "NONE" where they
// mean "not stated", and that is not a licence.
fn declared_licence(root: &XmlElement) -> String {
    let Some(service) = root
        .child("ServiceIdentification")
        .or_else(|| root.child("Service"))
    else {
        return String::new();
    };
    for field in ["AccessConstraints", "Fees"] {
        let text = service.child_text(field);
        let folded = lower(&text);
        if !text.is_empty() && folded != "none" && folded != "no" && folded != "unknown" {
            return if text.len() > 200 {
                format!("{}...", byte_prefix(&text, 200))
            } else {
                text
            };
        }
    }
    String::new()
}

fn service_title(root: &XmlElement) -> String {
    for section in ["ServiceIdentification", "Service"] {
        if let Some(service) = root.child(section) {
            let title = service.child_text("Title");
            if !title.is_empty() {
                return title;
            }
        }
    }
    String::new()
}

// Among the CRSs a layer offers, the one Katana asks in: Web Mercator where
// offered (every web service draws it, and it needs no axis swap), then WGS 84,
// then whatever the service lists first.
fn preferred_crs(offered: &[String]) -> String {
    for wanted in ["EPSG:3857", "EPSG:900913", "EPSG:4326", "CRS:84"] {
        for crs in offered {
            if lower(crs) == lower(wanted) {
                return if crs == "EPSG:900913" {
                    String::from("EPSG:3857")
                } else {
                    crs.clone()
                };
            }
        }
    }
    offered
        .first()
        .cloned()
        .unwrap_or_else(|| String::from("EPSG:4326"))
}

/// What a WMS layer's collection carries down to its children.
struct WmsWalk<'a> {
    provider: &'a OnlineProvider,
    used: BTreeSet<String>,
    v13: bool,
}

// Named layers of a WMS, depth first, each inheriting its parents' CRS list and
// bounding box as the WMS specification says (7.2.4.8).
fn collect_wms_layers(
    element: &XmlElement,
    mut crs: Vec<String>,
    mut lon_lat: Option<LonLatBox>,
    walk: &mut WmsWalk<'_>,
    service: &mut OnlineService,
) {
    for code in element.children_named(if walk.v13 { "CRS" } else { "SRS" }) {
        // 1.1.1 may list several codes in one element, space separated.
        crs.extend(
            code.text
                .split(' ')
                .filter(|piece| !piece.is_empty())
                .map(String::from),
        );
    }
    if let Some(bounds) = element.child("EX_GeographicBoundingBox") {
        let w = parse_finite_double(bounds.child_text("westBoundLongitude"));
        let s = parse_finite_double(bounds.child_text("southBoundLatitude"));
        let e = parse_finite_double(bounds.child_text("eastBoundLongitude"));
        let n = parse_finite_double(bounds.child_text("northBoundLatitude"));
        if let (Some(w), Some(s), Some(e), Some(n)) = (w, s, e, n) {
            lon_lat = Some([w, s, e, n]);
        }
    } else if let Some(lat_lon) = element.child("LatLonBoundingBox") {
        let w = parse_finite_double(lat_lon.attribute("minx"));
        let s = parse_finite_double(lat_lon.attribute("miny"));
        let e = parse_finite_double(lat_lon.attribute("maxx"));
        let n = parse_finite_double(lat_lon.attribute("maxy"));
        if let (Some(w), Some(s), Some(e), Some(n)) = (w, s, e, n) {
            lon_lat = Some([w, s, e, n]);
        }
    }
    let name = element.child_text("Name");
    if !name.is_empty() {
        let title = element.child_text("Title");
        let mut layer = OnlineLayer {
            id: unique_id(&mut walk.used, &safe_id(&name, LAYER_ID_LIMIT)),
            title: if title.is_empty() {
                name.clone()
            } else {
                title
            },
            kind: OnlineLayerKind::Imagery,
            crs: preferred_crs(&crs),
            layer_name: name,
            ..base_layer(walk.provider, service)
        };
        if let Some(lon_lat) = lon_lat
            && sane(&lon_lat)
        {
            layer.coverage = lon_lat;
        }
        service.layers.push(layer);
    }
    for child in element.children_named("Layer") {
        collect_wms_layers(child, crs.clone(), lon_lat, walk, service);
    }
}

fn parse_json(text: &[u8]) -> Result<Json> {
    Json::parse(text).map_err(|error| {
        make_error(
            ErrorCode::ParseFailure,
            "the service's description is not JSON",
            error.what,
        )
    })
}

// An ArcGIS extent ({xmin, ymin, xmax, ymax, spatialReference}) as a
// longitude/latitude box, when the extent's CRS can be read.
fn arcgis_extent(extent: &Json) -> Option<LonLatBox> {
    if !extent.is_object() || !extent.find("xmin").is_some_and(Json::is_number) {
        return None;
    }
    let mut crs = String::new();
    if let Some(reference) = extent.find("spatialReference")
        && reference.is_object()
    {
        let wkid_of = |key: &str| {
            reference
                .find(key)
                .filter(|value| value.is_number())
                .and_then(|value| value.get::<i32>().ok())
        };
        let wkid = wkid_of("latestWkid")
            .or_else(|| wkid_of("wkid"))
            .unwrap_or(0);
        if wkid == 102100 || wkid == 102113 {
            // Esri's old codes for Web Mercator.
            crs = String::from("EPSG:3857");
        } else if wkid > 0 {
            crs = format!("EPSG:{wkid}");
        } else if let Some(wkt) = reference.find("wkt").and_then(Json::as_str) {
            crs = String::from(wkt);
        }
    }
    if crs.is_empty() {
        return None;
    }
    // A side that is missing or not a number is no box: the C++ catches
    // nlohmann's type_error here (a missing side, read through the const
    // operator[], is undefined there).
    let side = |key: &str| extent.at(key).and_then(|value| value.get::<f64>()).ok();
    let native = CrsBox {
        min_x: side("xmin")?,
        min_y: side("ymin")?,
        max_x: side("xmax")?,
        max_y: side("ymax")?,
    };
    let lon_lat = transform_box(&native, &crs, "EPSG:4326").ok()?;
    let out = [
        cpp::max(-180.0, lon_lat.min_x),
        cpp::max(-90.0, lon_lat.min_y),
        cpp::min(180.0, lon_lat.max_x),
        cpp::min(90.0, lon_lat.max_y),
    ];
    sane(&out).then_some(out)
}

/// The kind of service a URL most probably is, from its shape alone: an ArcGIS
/// REST path (/MapServer, /ImageServer, /FeatureServer), a service= parameter
/// or a capabilities file name, a .tif, a STAC or OGC API path. None when the
/// URL does not say; discovery then tries the candidates in turn.
pub fn guess_service_type(url: &str) -> Option<OnlineServiceType> {
    let folded = lower(url);
    let path = lower(base_of(url));
    let has = |piece: &str| folded.contains(piece);
    if has("/imageserver") {
        return Some(OnlineServiceType::ArcgisExportImage);
    }
    if has("/featureserver") {
        return Some(OnlineServiceType::ArcgisQuery);
    }
    if has("/mapserver")
        && !has("wmsserver")
        && !has("wfsserver")
        && !has("wcsserver")
        && !has("/wmts")
    {
        return Some(OnlineServiceType::ArcgisExport);
    }
    if has("service=wmts") || has("wmtscapabilities") || has("/wmts") {
        return Some(OnlineServiceType::Wmts);
    }
    if has("service=wfs") || has("wfsserver") {
        return Some(OnlineServiceType::Wfs);
    }
    if has("service=wcs") || has("wcsserver") {
        return Some(OnlineServiceType::Wcs);
    }
    if has("service=wms") || has("wmsserver") {
        return Some(OnlineServiceType::Wms);
    }
    if path.ends_with(".tif") || path.ends_with(".tiff") {
        return Some(OnlineServiceType::Cog);
    }
    if has("stac") || path.ends_with(".json") {
        return Some(OnlineServiceType::Stac);
    }
    if path.ends_with("/collections") || has("/ogc/features") {
        return Some(OnlineServiceType::OgcApiFeatures);
    }
    None
}

/// The address of the description a service of `type` publishes at `url`:
/// GetCapabilities for the OGC services, ?f=json for ArcGIS REST, /collections
/// for OGC API and STAC, the URL itself for a STAC item or COG.
pub fn description_url(r#type: OnlineServiceType, url: &str) -> String {
    let base = base_of(url);
    let trimmed_base = base.trim_end_matches('/');
    match r#type {
        OnlineServiceType::Wms => {
            with_query(&ogc_base_of(url), "SERVICE=WMS&REQUEST=GetCapabilities")
        }
        OnlineServiceType::Wmts => {
            if lower(base).ends_with(".xml") {
                String::from(base)
            } else {
                with_query(
                    &ogc_base_of(url),
                    "SERVICE=WMTS&REQUEST=GetCapabilities&VERSION=1.0.0",
                )
            }
        }
        OnlineServiceType::Wfs => {
            with_query(&ogc_base_of(url), "SERVICE=WFS&REQUEST=GetCapabilities")
        }
        OnlineServiceType::Wcs => with_query(
            &ogc_base_of(url),
            "SERVICE=WCS&REQUEST=GetCapabilities&VERSION=1.0.0",
        ),
        OnlineServiceType::ArcgisExport
        | OnlineServiceType::ArcgisExportImage
        | OnlineServiceType::ArcgisQuery => with_query(base, "f=json"),
        OnlineServiceType::OgcApiFeatures => {
            if lower(trimmed_base).ends_with("/collections") {
                with_query(trimmed_base, "f=json")
            } else {
                with_query(&format!("{trimmed_base}/collections"), "f=json")
            }
        }
        OnlineServiceType::Stac => {
            let root = lower(trimmed_base);
            if root.ends_with(".json") || root.ends_with("/collections") {
                String::from(url)
            } else {
                format!("{trimmed_base}/collections")
            }
        }
        _ => String::from(url),
    }
}

/// The id a discovered provider gets: "custom-" and the host, made safe for a
/// verb and a file name, e.g. custom-opendata-maps-vic-gov-au-1a2b3c4d.
pub fn custom_provider_id(url: &str) -> String {
    // The host says whose it is; eight hex digits of the address without its
    // query say which service, so two services of one server are two
    // providers - one host alone let the second replace the first.
    let hash = sha256_hex(base_of(url));
    format!("custom-{}-{}", safe_id(host_of(url), 40), &hash[..8])
}

pub fn parse_wms_capabilities(xml: impl AsRef<[u8]>, url: &str) -> Result<OnlineProvider> {
    let root = parse_xml(xml)?;
    let name = root.local_name();
    if name != "WMS_Capabilities" && name != "WMT_MS_Capabilities" {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "the answer is not WMS capabilities",
            name,
        ));
    }
    let version = match root.attribute("version") {
        version if version.is_empty() => String::from("1.3.0"),
        version => version,
    };
    let mut provider = make_provider(url, &service_title(&root));
    let licence = declared_licence(&root);
    if !licence.is_empty() {
        provider.licence = licence;
    }
    let mut service = OnlineService {
        id: String::from("wms"),
        title: format!("{} (WMS)", provider.title),
        r#type: OnlineServiceType::Wms,
        endpoint: ogc_base_of(url),
        layers: Vec::new(),
    };
    // The GetMap address the service names, which may differ from where its
    // capabilities were found.
    if let Some(capability) = root.child("Capability") {
        if let Some(get_map) = capability
            .child("Request")
            .and_then(|request| request.child("GetMap"))
        {
            for dcp in get_map.children_named("DCPType") {
                if let Some(resource) = dcp
                    .child("HTTP")
                    .and_then(|http| http.child("Get"))
                    .and_then(|get| get.child("OnlineResource"))
                {
                    let href = resource.attribute("href");
                    if href.starts_with("http") {
                        service.endpoint = ogc_base_of(&href);
                    }
                }
            }
        }
        let mut walk = WmsWalk {
            provider: &provider,
            used: BTreeSet::new(),
            v13: version.starts_with("1.3"),
        };
        for layer in capability.children_named("Layer") {
            collect_wms_layers(layer, Vec::new(), None, &mut walk, &mut service);
        }
    }
    for layer in &mut service.layers {
        layer.endpoint.clone_from(&service.endpoint);
        layer.version.clone_from(&version);
        layer.licence.clone_from(&provider.licence);
    }
    if service.layers.is_empty() {
        return Err(Error::new(
            ErrorCode::NotFound,
            "the WMS lists no named layer",
        ));
    }
    provider.services.push(service);
    Ok(provider)
}

/// A WMTS matrix set on the Web Mercator grid, level for level: the prefix its
/// matrix ids carry ("EPSG:3857:" or "") and its deepest level.
struct GridInfo {
    prefix: String,
    max_zoom: i32,
}

pub fn parse_wmts_capabilities(xml: impl AsRef<[u8]>, url: &str) -> Result<OnlineProvider> {
    let root = parse_xml(xml)?;
    if root.local_name() != "Capabilities" {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "the answer is not WMTS capabilities",
            root.local_name(),
        ));
    }
    let mut provider = make_provider(url, &service_title(&root));
    let licence = declared_licence(&root);
    if !licence.is_empty() {
        provider.licence = licence;
    }
    let Some(contents) = root.child("Contents") else {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "the WMTS capabilities have no Contents",
        ));
    };
    // Only matrix sets on the Web Mercator grid, level for level: those are
    // what the tile path computes with (web_mercator_tile_matrix_set).
    let mut grids: BTreeMap<String, GridInfo> = BTreeMap::new();
    let reference = web_mercator_tile_matrix_set(24);
    for set in contents.children_named("TileMatrixSet") {
        let id = set.child_text("Identifier");
        let crs = lower(&set.child_text("SupportedCRS"));
        if !crs.contains("3857") && !crs.contains("900913") {
            continue;
        }
        let matrices = set.children_named("TileMatrix");
        let mut matches = !matrices.is_empty();
        let mut prefix = String::new();
        let mut level: i32 = 0;
        for matrix in matrices {
            let scale = parse_finite_double(matrix.child_text("ScaleDenominator"));
            let matrix_id = matrix.child_text("Identifier");
            let (level_text, this_prefix) = match matrix_id.rfind(':') {
                Some(colon) => (&matrix_id[colon + 1..], &matrix_id[..=colon]),
                None => (matrix_id.as_str(), ""),
            };
            let parsed = parse_finite_double(level_text);
            let (Some(scale), Some(parsed)) = (scale, parsed) else {
                matches = false;
                break;
            };
            if level as usize >= reference.matrices.len() {
                matches = false;
                break;
            }
            let expected = reference.matrices[level as usize].scale_denominator;
            // A level past i32's range is undefined in the C++ and saturates
            // here.
            if (scale - expected).abs() > expected * 1e-6
                || parsed as i32 != level
                || (level > 0 && this_prefix != prefix)
            {
                matches = false;
                break;
            }
            prefix = String::from(this_prefix);
            level += 1;
        }
        if matches {
            grids.insert(
                id,
                GridInfo {
                    prefix,
                    max_zoom: level - 1,
                },
            );
        }
    }
    let mut service = OnlineService {
        id: String::from("wmts"),
        title: format!("{} (WMTS)", provider.title),
        r#type: OnlineServiceType::Wmts,
        endpoint: ogc_base_of(url),
        layers: Vec::new(),
    };
    let mut used = BTreeSet::new();
    for layer_element in contents.children_named("Layer") {
        let identifier = layer_element.child_text("Identifier");
        let grid_id = layer_element
            .children_named("TileMatrixSetLink")
            .into_iter()
            .map(|link| link.child_text("TileMatrixSet"))
            .find(|linked| grids.contains_key(linked))
            .unwrap_or_default();
        if identifier.is_empty() || grid_id.is_empty() {
            continue;
        }
        let grid = &grids[&grid_id];
        let mut style = String::from("default");
        for style_element in layer_element.children_named("Style") {
            if style_element.attribute("isDefault") == "true" || style == "default" {
                let id = style_element.child_text("Identifier");
                if !id.is_empty() {
                    style = id;
                }
            }
        }
        let mut format = layer_element.child_text("Format");
        let mut pattern = String::new();
        for resource in layer_element.children_named("ResourceURL") {
            if resource.attribute("resourceType") == "tile" {
                pattern = resource.attribute("template");
                let resource_format = resource.attribute("format");
                if !resource_format.is_empty() {
                    format = resource_format;
                }
                break;
            }
        }
        if pattern.is_empty() {
            // KVP only: the same request as a template.
            pattern = with_query(
                &service.endpoint,
                &format!(
                    "SERVICE=WMTS&REQUEST=GetTile&VERSION=1.0.0&LAYER={}&STYLE={}&FORMAT={}\
                     &TILEMATRIXSET={}&TILEMATRIX={{TileMatrix}}&TILEROW={{TileRow}}\
                     &TILECOL={{TileCol}}",
                    percent_encode(&identifier),
                    percent_encode(&style),
                    percent_encode(if format.is_empty() {
                        "image/png"
                    } else {
                        &format
                    }),
                    percent_encode(&grid_id)
                ),
            );
        }
        // One template form for the tile path: {z} {x} {y}, the matrix id's
        // prefix written in, the style, layer and set fixed, and a time
        // dimension left as {time} with its default.
        pattern = replace_all(pattern, "{TileMatrixSet}", &grid_id);
        pattern = replace_all(pattern, "{TileMatrix}", &format!("{}{{z}}", grid.prefix));
        pattern = replace_all(pattern, "{TileCol}", "{x}");
        pattern = replace_all(pattern, "{TileRow}", "{y}");
        pattern = replace_all(pattern, "{Style}", &style);
        pattern = replace_all(pattern, "{style}", &style);
        pattern = replace_all(pattern, "{Layer}", &identifier);
        pattern = replace_all(pattern, "{Time}", "{time}");
        let title = layer_element.child_text("Title");
        let mut layer = OnlineLayer {
            id: unique_id(&mut used, &safe_id(&identifier, LAYER_ID_LIMIT)),
            title: if title.is_empty() {
                identifier.clone()
            } else {
                title
            },
            kind: OnlineLayerKind::Imagery,
            layer_name: identifier,
            endpoint: pattern,
            crs: String::from("EPSG:3857"),
            max_zoom: grid.max_zoom,
            max_tiles: 256,
            time: String::from("default"),
            ..base_layer(&provider, &service)
        };
        if let Some(bounds) = layer_element.child("WGS84BoundingBox")
            && let Some(lon_lat) = lon_lat_from_corners(
                &bounds.child_text("LowerCorner"),
                &bounds.child_text("UpperCorner"),
            )
            && sane(&lon_lat)
        {
            layer.coverage = lon_lat;
        }
        service.layers.push(layer);
    }
    if service.layers.is_empty() {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "the WMTS offers no layer on the Web Mercator (GoogleMapsCompatible) grid, which is \
             the tile grid Katana reads",
        ));
    }
    provider.services.push(service);
    Ok(provider)
}

pub fn parse_wfs_capabilities(xml: impl AsRef<[u8]>, url: &str) -> Result<OnlineProvider> {
    let root = parse_xml(xml)?;
    if root.local_name() != "WFS_Capabilities" {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "the answer is not WFS capabilities",
            root.local_name(),
        ));
    }
    let version = match root.attribute("version") {
        version if version.is_empty() => String::from("2.0.0"),
        version => version,
    };
    let mut provider = make_provider(url, &service_title(&root));
    let licence = declared_licence(&root);
    if !licence.is_empty() {
        provider.licence = licence;
    }
    let mut service = OnlineService {
        id: String::from("wfs"),
        title: format!("{} (WFS)", provider.title),
        r#type: OnlineServiceType::Wfs,
        endpoint: ogc_base_of(url),
        layers: Vec::new(),
    };
    let mut used = BTreeSet::new();
    if let Some(list) = root.child("FeatureTypeList") {
        for feature_type in list.children_named("FeatureType") {
            let name = feature_type.child_text("Name");
            if name.is_empty() {
                continue;
            }
            let mut crs = feature_type.child_text("DefaultCRS");
            if crs.is_empty() {
                crs = feature_type.child_text("DefaultSRS");
            }
            // urn:ogc:def:crs:EPSG::28356 -> EPSG:28356, the form the request
            // builders and GDAL's user input both take.
            if let Some(at) = crs.find("EPSG::") {
                crs = format!("EPSG:{}", &crs[at + 6..]);
            }
            let title = feature_type.child_text("Title");
            let mut layer = OnlineLayer {
                id: unique_id(&mut used, &safe_id(&name, LAYER_ID_LIMIT)),
                title: if title.is_empty() {
                    name.clone()
                } else {
                    title
                },
                kind: OnlineLayerKind::Vector,
                layer_name: name,
                version: version.clone(),
                crs: if crs.is_empty() {
                    String::from("EPSG:4326")
                } else {
                    crs
                },
                ..base_layer(&provider, &service)
            };
            if let Some(bounds) = feature_type.child("WGS84BoundingBox")
                && let Some(lon_lat) = lon_lat_from_corners(
                    &bounds.child_text("LowerCorner"),
                    &bounds.child_text("UpperCorner"),
                )
                && sane(&lon_lat)
            {
                layer.coverage = lon_lat;
            }
            service.layers.push(layer);
        }
    }
    if service.layers.is_empty() {
        return Err(Error::new(
            ErrorCode::NotFound,
            "the WFS lists no feature type",
        ));
    }
    provider.services.push(service);
    Ok(provider)
}

pub fn parse_wcs_capabilities(xml: impl AsRef<[u8]>, url: &str) -> Result<OnlineProvider> {
    let root = parse_xml(xml)?;
    if root.local_name() != "WCS_Capabilities" && root.local_name() != "Capabilities" {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "the answer is not WCS capabilities",
            root.local_name(),
        ));
    }
    let mut title = service_title(&root);
    if title.is_empty()
        && let Some(service) = root.child("Service")
    {
        title = service.child_text("label");
    }
    let mut provider = make_provider(url, &title);
    let mut service = OnlineService {
        id: String::from("wcs"),
        title: format!("{} (WCS)", provider.title),
        r#type: OnlineServiceType::Wcs,
        endpoint: ogc_base_of(url),
        layers: Vec::new(),
    };
    let mut used = BTreeSet::new();
    if let Some(metadata) = root.child("ContentMetadata") {
        for brief in metadata.children_named("CoverageOfferingBrief") {
            let name = brief.child_text("name");
            if name.is_empty() {
                continue;
            }
            let label = brief.child_text("label");
            let mut layer = OnlineLayer {
                id: unique_id(&mut used, &safe_id(&name, LAYER_ID_LIMIT)),
                title: if label.is_empty() {
                    name.clone()
                } else {
                    label
                },
                kind: OnlineLayerKind::Elevation,
                layer_name: name,
                version: String::from("1.0.0"),
                crs: String::from("EPSG:4326"),
                ..base_layer(&provider, &service)
            };
            if let Some(envelope) = brief.child("lonLatEnvelope") {
                let positions = envelope.children_named("pos");
                if positions.len() == 2
                    && let Some(lon_lat) =
                        lon_lat_from_corners(&positions[0].text, &positions[1].text)
                    && sane(&lon_lat)
                {
                    layer.coverage = lon_lat;
                }
            }
            service.layers.push(layer);
        }
    }
    if service.layers.is_empty() {
        return Err(Error::new(
            ErrorCode::NotFound,
            "the WCS lists no coverage (only WCS 1.0.0 is read)",
        ));
    }
    provider.services.push(service);
    Ok(provider)
}

fn parse_arcgis_description_json(text: &[u8], url: &str) -> JsonResult<Result<OnlineProvider>> {
    let json = match parse_json(text) {
        Ok(json) => json,
        Err(error) => return Ok(Err(error)),
    };
    if let Some(error) = json.find("error") {
        return Ok(Err(make_error(
            ErrorCode::FileImportFailure,
            "the ArcGIS service refused the request",
            error.dump(),
        )));
    }
    if !json.is_object() {
        return Ok(Err(Error::new(
            ErrorCode::ParseFailure,
            "the ArcGIS description is not an object",
        )));
    }
    let base = base_of(url);
    let folded = lower(base);
    // The C++ evaluates the defaults from the inside out.
    let described = json.value("serviceDescription", String::from(host_of(url)))?;
    let named = json.value("name", described)?;
    let title = json.value("mapName", named)?;
    let mut provider = make_provider(
        url,
        &if title.is_empty() {
            String::from(host_of(url))
        } else {
            byte_prefix(&title, 120)
        },
    );
    if let Some(copyright) = json.find("copyrightText").and_then(Json::as_str)
        && !copyright.is_empty()
    {
        provider.attribution = String::from(copyright);
    }
    let number = |key: &str| json.find(key).filter(|value| value.is_number());
    let page_size = match number("maxRecordCount") {
        Some(count) => count.get::<i32>()?.clamp(1, 10000),
        None => 1000,
    };
    let extent = match json.find("fullExtent") {
        Some(full) => arcgis_extent(full),
        None => json.find("extent").and_then(arcgis_extent),
    };
    let apply_extent = |layer: &mut OnlineLayer| {
        if let Some(extent) = extent {
            layer.coverage = extent;
        }
    };
    let mut used = BTreeSet::new();

    // One layer of a MapServer or FeatureServer: .../MapServer/9
    let last_slash = base.rfind('/');
    let tail = last_slash.map_or("", |slash| &base[slash + 1..]);
    let layer_url = !tail.is_empty() && tail.bytes().all(|byte| byte.is_ascii_digit());
    if let Some(slash) = last_slash
        && layer_url
    {
        let mut service = OnlineService {
            id: String::from("query"),
            title: provider.title.clone(),
            r#type: OnlineServiceType::ArcgisQuery,
            endpoint: String::from(&base[..slash]),
            layers: Vec::new(),
        };
        let name = json.value("name", String::from(tail))?;
        let mut layer = OnlineLayer {
            id: unique_id(&mut used, &safe_id(&name, LAYER_ID_LIMIT)),
            title: json.value("name", String::from(tail))?,
            kind: OnlineLayerKind::Vector,
            layer_name: String::from(tail),
            page_size,
            ..base_layer(&provider, &service)
        };
        // The layer's own id field (OBJECTID, FID, OID...), which pages are
        // ordered and joined by.
        if let Some(id_field) = json.find("objectIdField").and_then(Json::as_str) {
            layer.id_field = String::from(id_field);
        } else if let Some(fields) = json.find("fields")
            && fields.is_array()
        {
            for field in fields.iter() {
                if field.is_object() && field.value("type", String::new())? == "esriFieldTypeOID" {
                    layer.id_field = field.value("name", String::new())?;
                    break;
                }
            }
        }
        apply_extent(&mut layer);
        service.layers.push(layer);
        provider.services.push(service);
        return Ok(Ok(provider));
    }

    if folded.ends_with("/imageserver") {
        let mut service = OnlineService {
            id: String::from("image"),
            title: provider.title.clone(),
            r#type: OnlineServiceType::ArcgisExportImage,
            endpoint: String::from(base),
            layers: Vec::new(),
        };
        let pixel_type = json.value("pixelType", String::new())?;
        let bands = match number("bandCount") {
            Some(count) => count.get::<i32>()?,
            None => 1,
        };
        // One band of anything but unsigned bytes is values, not a picture: an
        // elevation model, read as numbers.
        let values = bands == 1 && pixel_type != "U8";
        let named = json.value(
            "name",
            String::from(if values { "elevation" } else { "image" }),
        )?;
        let mut layer = OnlineLayer {
            id: unique_id(&mut used, &safe_id(&named, LAYER_ID_LIMIT)),
            title: json.value("name", provider.title.clone())?,
            kind: if values {
                OnlineLayerKind::Elevation
            } else {
                OnlineLayerKind::Imagery
            },
            crs: String::from("EPSG:3857"),
            max_request_pixels: match number("maxImageWidth") {
                Some(width) => width.get::<i32>()?.clamp(64, 16384),
                None => 4000,
            },
            ..base_layer(&provider, &service)
        };
        if let Some(size) = number("pixelSizeX") {
            let size = size.get::<f64>()?;
            // Degrees when the service is geographic; metres otherwise.
            layer.resolution = Some(if size < 0.01 { size * 111320.0 } else { size });
        }
        apply_extent(&mut layer);
        service.layers.push(layer);
        provider.services.push(service);
        return Ok(Ok(provider));
    }

    // A MapServer or FeatureServer: the map as a whole, as an image (a
    // MapServer only), and each feature layer as vectors.
    if folded.ends_with("/mapserver") {
        let mut service = OnlineService {
            id: String::from("map"),
            title: format!("{} (map image)", provider.title),
            r#type: OnlineServiceType::ArcgisExport,
            endpoint: String::from(base),
            layers: Vec::new(),
        };
        let mut layer = OnlineLayer {
            id: unique_id(&mut used, "map"),
            title: format!("{} - the whole map as an image", provider.title),
            kind: OnlineLayerKind::Imagery,
            crs: String::from("EPSG:3857"),
            max_request_pixels: match number("maxImageWidth") {
                Some(width) => width.get::<i32>()?.clamp(64, 16384),
                None => 2048,
            },
            ..base_layer(&provider, &service)
        };
        apply_extent(&mut layer);
        service.layers.push(layer);
        provider.services.push(service);
    }
    let mut features = OnlineService {
        id: String::from("features"),
        title: format!("{} (features)", provider.title),
        r#type: OnlineServiceType::ArcgisQuery,
        endpoint: String::from(base),
        layers: Vec::new(),
    };
    if let Some(layers) = json.find("layers")
        && layers.is_array()
    {
        for entry in layers.iter() {
            let Some(id) = entry.find("id").filter(|id| id.is_number()) else {
                continue;
            };
            // A group layer has sublayers and no features of its own.
            if entry
                .find("subLayerIds")
                .is_some_and(|sublayers| sublayers.is_array() && !sublayers.empty())
            {
                continue;
            }
            let kind = entry.value("type", String::from("Feature Layer"))?;
            if kind != "Feature Layer" && kind != "Table" {
                continue;
            }
            if kind == "Table" {
                continue; // no geometry to draw
            }
            let number_text = format!("{}", id.get::<i32>()?);
            let name = entry.value("name", number_text.clone())?;
            let mut layer = OnlineLayer {
                id: unique_id(&mut used, &safe_id(&name, LAYER_ID_LIMIT)),
                title: name,
                kind: OnlineLayerKind::Vector,
                layer_name: number_text,
                page_size,
                ..base_layer(&provider, &features)
            };
            apply_extent(&mut layer);
            features.layers.push(layer);
        }
    }
    if !features.layers.is_empty() {
        provider.services.push(features);
    }
    if provider.services.is_empty() {
        return Ok(Err(Error::new(
            ErrorCode::NotFound,
            "the ArcGIS service offers no layer Katana can import",
        )));
    }
    Ok(Ok(provider))
}

/// A MapServer, ImageServer or FeatureServer description, or one layer of a
/// MapServer or FeatureServer (…/MapServer/9?f=json).
pub fn parse_arcgis_description(json: impl AsRef<[u8]>, url: &str) -> Result<OnlineProvider> {
    uncaught(parse_arcgis_description_json(json.as_ref(), url))
}

/// The root a /collections address is under: the base without trailing
/// slashes or its "/collections".
fn collections_root(url: &str) -> String {
    let mut root = String::from(base_of(url).trim_end_matches('/'));
    if lower(&root).ends_with("/collections") {
        root.truncate(root.len() - "/collections".len());
    }
    root
}

fn parse_oapif_collections_json(text: &[u8], url: &str) -> JsonResult<Result<OnlineProvider>> {
    let json = match parse_json(text) {
        Ok(json) => json,
        Err(error) => return Ok(Err(error)),
    };
    let collections = match json.find("collections") {
        Some(collections) if collections.is_array() => collections,
        _ => {
            return Ok(Err(Error::new(
                ErrorCode::ParseFailure,
                "the answer lists no collections",
            )));
        }
    };
    let root = collections_root(url);
    let mut provider = make_provider(url, &json.value("title", String::from(host_of(url)))?);
    let mut service = OnlineService {
        id: String::from("features"),
        title: format!("{} (OGC API - Features)", provider.title),
        r#type: OnlineServiceType::OgcApiFeatures,
        endpoint: root,
        layers: Vec::new(),
    };
    let mut used = BTreeSet::new();
    for collection in collections.iter() {
        let Some(id_value) = collection.find("id") else {
            continue;
        };
        let id = match id_value.as_str() {
            Some(text) => String::from(text),
            None => id_value.dump(),
        };
        let mut layer = OnlineLayer {
            id: unique_id(&mut used, &safe_id(&id, LAYER_ID_LIMIT)),
            title: collection.value("title", id.clone())?,
            kind: OnlineLayerKind::Vector,
            layer_name: id,
            crs: String::from("OGC:CRS84"),
            ..base_layer(&provider, &service)
        };
        if let Some(bbox) = collection
            .find("extent")
            .and_then(|extent| extent.find("spatial"))
            .and_then(|spatial| spatial.find("bbox"))
            && bbox.is_array()
            && !bbox.empty()
        {
            let first = &bbox[0];
            if first.is_array() && first.size() >= 4 && first[0].is_number() {
                let three_d = first.size() >= 6;
                // In the C++'s order: a side that is not a number is the
                // type_error it lets out.
                let coverage = [
                    first[0].get::<f64>()?,
                    first[1].get::<f64>()?,
                    first[if three_d { 3 } else { 2 }].get::<f64>()?,
                    first[if three_d { 4 } else { 3 }].get::<f64>()?,
                ];
                if sane(&coverage) {
                    layer.coverage = coverage;
                }
            }
        }
        // A STAC collection listed at /collections of a STAC API is not a
        // feature collection; parse_stac_description reads those.
        if collection.value("type", String::new())? == "Collection"
            && collection.contains("stac_version")
        {
            continue;
        }
        service.layers.push(layer);
    }
    if service.layers.is_empty() {
        return Ok(Err(Error::new(
            ErrorCode::NotFound,
            "the OGC API lists no feature collection",
        )));
    }
    provider.services.push(service);
    Ok(Ok(provider))
}

pub fn parse_oapif_collections(json: impl AsRef<[u8]>, url: &str) -> Result<OnlineProvider> {
    uncaught(parse_oapif_collections_json(json.as_ref(), url))
}

fn parse_stac_description_json(text: &[u8], url: &str) -> JsonResult<Result<OnlineProvider>> {
    let json = match parse_json(text) {
        Ok(json) => json,
        Err(error) => return Ok(Err(error)),
    };
    // One item: a layer reading that item's assets.
    if json.is_object()
        && json.value("type", String::new())? == "Feature"
        && json.contains("assets")
    {
        let items = parse_stac_items(text);
        let Some(item) = items.as_ref().ok().and_then(|items| items.first()) else {
            return Ok(Err(Error::new(
                ErrorCode::ParseFailure,
                "the STAC item could not be read",
            )));
        };
        let mut provider = make_provider(url, &format!("STAC item {}", item.id));
        let mut service = OnlineService {
            id: String::from("item"),
            title: provider.title.clone(),
            r#type: OnlineServiceType::Stac,
            endpoint: String::from(base_of(url)),
            layers: Vec::new(),
        };
        let asset = if item.assets.contains_key("visual") {
            "visual"
        } else if item.assets.contains_key("red")
            && item.assets.contains_key("green")
            && item.assets.contains_key("blue")
        {
            "red,green,blue"
        } else {
            ""
        };
        let mut layer = OnlineLayer {
            id: safe_id(&item.id, LAYER_ID_LIMIT),
            title: if item.datetime.is_empty() {
                item.id.clone()
            } else {
                format!("{} ({})", item.id, byte_prefix(&item.datetime, 10))
            },
            kind: OnlineLayerKind::Imagery,
            layer_name: json.value("collection", String::from("item"))?,
            asset: String::from(asset),
            ..base_layer(&provider, &service)
        };
        if layer.asset.is_empty() {
            return Ok(Err(Error::new(
                ErrorCode::Unsupported,
                "the STAC item has no visual asset and no red, green and blue bands",
            )));
        }
        layer.coverage = [
            item.bounds.min_x,
            item.bounds.min_y,
            item.bounds.max_x,
            item.bounds.max_y,
        ];
        layer.resolution = Some(10.0);
        layer.days = 0;
        if layer.layer_name.starts_with("sentinel-2") {
            layer.licence = String::from("Copernicus Sentinel data terms (free, full and open)");
            layer.attribution = String::from("Contains modified Copernicus Sentinel data");
        }
        service.layers.push(layer);
        provider.services.push(service);
        return Ok(Ok(provider));
    }
    // A STAC API's collections: each a layer searched through <root>/search.
    let collections = match json.find("collections") {
        Some(collections) if collections.is_array() => collections,
        _ => {
            return Ok(Err(Error::new(
                ErrorCode::ParseFailure,
                "the answer is neither a STAC item nor a list of collections",
            )));
        }
    };
    let root = collections_root(url);
    let mut provider = make_provider(url, &format!("{} (STAC)", host_of(url)));
    let mut service = OnlineService {
        id: String::from("stac"),
        title: provider.title.clone(),
        r#type: OnlineServiceType::Stac,
        endpoint: format!("{root}/search"),
        layers: Vec::new(),
    };
    let mut used = BTreeSet::new();
    for collection in collections.iter() {
        let Some(id) = collection.find("id").and_then(Json::as_str) else {
            continue;
        };
        let assets: BTreeSet<String> = match collection.find("item_assets") {
            Some(item_assets) if item_assets.is_object() => item_assets
                .items()
                .into_iter()
                .map(|(key, _)| key.into_owned())
                .collect(),
            _ => BTreeSet::new(),
        };
        let asset = if assets.contains("visual") {
            "visual"
        } else if assets.contains("red") && assets.contains("green") && assets.contains("blue") {
            "red,green,blue"
        } else {
            continue;
        };
        let mut layer = OnlineLayer {
            id: unique_id(&mut used, &safe_id(id, LAYER_ID_LIMIT)),
            title: collection.value("title", String::from(id))?,
            kind: OnlineLayerKind::Imagery,
            layer_name: String::from(id),
            asset: String::from(asset),
            ..base_layer(&provider, &service)
        };
        if let Some(licence) = collection.find("license").and_then(Json::as_str) {
            layer.licence = String::from(licence);
        }
        service.layers.push(layer);
    }
    if service.layers.is_empty() {
        return Ok(Err(Error::new(
            ErrorCode::NotFound,
            "no STAC collection offers a visual asset or red, green and blue bands",
        )));
    }
    provider.services.push(service);
    Ok(Ok(provider))
}

/// A STAC API's /collections, or a single STAC item (which becomes a layer of
/// its own: the item's visual asset, or red/green/blue composed).
pub fn parse_stac_description(json: impl AsRef<[u8]>, url: &str) -> Result<OnlineProvider> {
    uncaught(parse_stac_description_json(json.as_ref(), url))
}

/// Asks the service at `url` to describe itself and returns what it offers,
/// every layer marked user_defined. The type is guessed from the URL, and when
/// the URL does not say, WMS, then WMTS, WFS, ArcGIS REST and OGC API are tried
/// in turn. A COG (.tif) is probed through `probe_cog` (probe_raster over
/// /vsicurl/), since its header is its description; None is the C++'s empty
/// std::function. Unsupported, naming what was tried, when nothing answers as a
/// service Katana reads; the service's own failure otherwise.
pub fn discover_service(
    url: &str,
    fetch: TextFetcher<'_>,
    probe_cog: Option<CogProbe<'_>>,
) -> Result<OnlineProvider> {
    let folded = lower(&byte_prefix(url, 8));
    if !folded.starts_with("https://")
        && !folded.starts_with("http://")
        && !folded.starts_with("file://")
    {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a service address starts with https://",
            redact_url(url),
        ));
    }
    let guessed = guess_service_type(url);
    if guessed == Some(OnlineServiceType::Cog) {
        return match probe_cog {
            Some(probe) => probe(url),
            None => Err(Error::new(
                ErrorCode::Unsupported,
                "no way to read a COG header here",
            )),
        };
    }
    let candidates = match guessed {
        Some(guessed) => vec![guessed],
        None => vec![
            OnlineServiceType::Wms,
            OnlineServiceType::Wmts,
            OnlineServiceType::Wfs,
            OnlineServiceType::ArcgisExport,
            OnlineServiceType::OgcApiFeatures,
        ],
    };
    let mut tried = String::new();
    let mut first_failure: Option<Error> = None;
    let note = |tried: &mut String, r#type: OnlineServiceType| {
        if !tried.is_empty() {
            tried.push_str(", ");
        }
        tried.push_str(to_string(r#type));
    };
    for r#type in candidates {
        let description = description_url(r#type, url);
        let body = match fetch(&description) {
            Ok(body) => body,
            Err(error) => {
                first_failure.get_or_insert(error);
                note(&mut tried, r#type);
                continue;
            }
        };
        let parsed = match r#type {
            OnlineServiceType::Wms => parse_wms_capabilities(&body, url),
            OnlineServiceType::Wmts => parse_wmts_capabilities(&body, url),
            OnlineServiceType::Wfs => parse_wfs_capabilities(&body, url),
            OnlineServiceType::Wcs => parse_wcs_capabilities(&body, url),
            OnlineServiceType::ArcgisExport
            | OnlineServiceType::ArcgisExportImage
            | OnlineServiceType::ArcgisQuery => parse_arcgis_description(&body, url),
            OnlineServiceType::OgcApiFeatures => parse_oapif_collections(&body, url),
            OnlineServiceType::Stac => parse_stac_description(&body, url),
            _ => Err(Error::new(ErrorCode::Unsupported, "unreachable")),
        };
        match parsed {
            Ok(provider) => return Ok(provider),
            Err(error) if guessed.is_some() => return Err(error),
            Err(_) => note(&mut tried, r#type),
        }
    }
    if guessed.is_some()
        && let Some(failure) = first_failure
    {
        return Err(failure);
    }
    Err(make_error(
        ErrorCode::Unsupported,
        format!("the address did not answer as any service Katana reads (tried {tried})"),
        redact_url(url),
    ))
}
