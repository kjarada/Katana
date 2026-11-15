//! Requests for every kind of web service the online catalogue lists, built as
//! plain strings by pure functions (docs/gis_online.md, "How each service is
//! asked"). The port of include/katana/interop/online_requests.hpp and
//! src/katana_interop/online_requests.cpp.
//!
//! Nothing here touches the network, the disk or a clock, so every URL, query
//! and paging decision is tested byte for byte against what the service's
//! specification says it should be (tests/interop/test_online_requests.cpp);
//! online_fetch sends what these build.
//!
//! COORDINATES. Every box handed to a builder is in traditional GIS order - x
//! easting or longitude, y northing or latitude (katana_gis::reproject) - in
//! the CRS the builder names. A builder whose protocol follows the CRS
//! authority's axis order (WMS 1.3.0, WFS 2.0) takes `axis_yx` from
//! katana_gis::reproject::crs_axis_is_yx and swaps the pair itself; nothing
//! else in the program handles a swapped pair.
//!
//! The JSON answers are read through katana_core::json, the translation of the
//! C++'s nlohmann, with nlohmann's rules: a member of another type than the
//! default `value()` is given throws a type_error, which some readers here let
//! out uncaught (a defect, docs/rust.md "Not done"). That is ported as it is,
//! as online_catalogue's parse_catalogue has it: a panic carrying the
//! exception's what().

use std::collections::BTreeMap;

use katana_core::error::make_error;
use katana_core::json::{Json, JsonResult};
use katana_core::text::trimmed;
use katana_core::{Error, ErrorCode, Result};
use katana_gis::reproject::CrsBox;
use katana_gis::web_access::percent_encode;
use katana_math::cpp;

use crate::online_catalogue::{OnlineLayer, OnlineLayerKind};

/// WMTS 1.0 (6.1): the "standardized rendering pixel" is 0.28 mm.
const RENDERING_PIXEL_METRES: f64 = 0.28e-3;
/// Web Mercator's scale denominator at level 0 for 256-pixel tiles: the
/// equator's length (2 pi 6378137 m) over 256 pixels of 0.28 mm (WMTS 1.0,
/// annex E.4, GoogleMapsCompatible).
const WEB_MERCATOR_SCALE_0: f64 = 559082264.0287178;
/// pi * 6378137.
const WEB_MERCATOR_HALF: f64 = 20037508.342789244;
/// Mean radius, IUGG.
const EARTH_RADIUS_KM: f64 = 6371.0;

/// What nlohmann throws and the C++ lets out of a reader uncaught: a panic
/// carrying the exception's what(), as online_catalogue's parse_catalogue
/// has it. The readers return their answer inside a JsonResult, whose Err is
/// that exception.
pub(crate) fn uncaught<T>(answer: JsonResult<Result<T>>) -> Result<T> {
    match answer {
        Ok(answer) => answer,
        Err(error) => panic!("{}", error.what),
    }
}

/// `text` with every `from` replaced by `to`, the search going on after each
/// replacement.
pub(crate) fn replace_all(mut text: String, from: &str, to: &str) -> String {
    let mut found = text.find(from);
    while let Some(at) = found {
        text.replace_range(at..at + from.len(), to);
        let after = at + to.len();
        found = text[after..].find(from).map(|next| next + after);
    }
    text
}

/// katana_core::text::trimmed, of text: only ASCII is cut, so what is left is
/// text.
pub(crate) fn trimmed_text(text: &str) -> &str {
    std::str::from_utf8(trimmed(text.as_bytes())).unwrap_or_default()
}

fn box4(crs_box: &CrsBox, swap: bool) -> String {
    if swap {
        format!(
            "{},{},{},{}",
            format_number(crs_box.min_y),
            format_number(crs_box.min_x),
            format_number(crs_box.max_y),
            format_number(crs_box.max_x)
        )
    } else {
        format!(
            "{},{},{},{}",
            format_number(crs_box.min_x),
            format_number(crs_box.min_y),
            format_number(crs_box.max_x),
            format_number(crs_box.max_y)
        )
    }
}

fn trim_slash(url: &str) -> &str {
    url.trim_end_matches('/')
}

// A tag key or value an Overpass filter may hold: letters, digits and the
// punctuation OpenStreetMap keys use (addr:street, name:en, building:levels,
// a value like "1-2"). No quote, backslash, bracket or brace can appear, so a
// term can only ever be quoted text inside the one statement.
fn plain_tag_text(text: &str) -> bool {
    !text.is_empty()
        && text.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b':' | b'-' | b'.' | b' ')
                || byte >= 0x80
        })
}

fn bbox_of(value: &Json) -> Option<CrsBox> {
    if !value.is_array() || value.size() < 4 {
        return None;
    }
    // 2D [w, s, e, n] or 3D [w, s, zmin, e, n, zmax] (GeoJSON 5).
    let three_d = value.size() >= 6;
    let side = |index: usize| value[index].get::<f64>().ok();
    Some(CrsBox {
        min_x: side(0)?,
        min_y: side(1)?,
        max_x: side(if three_d { 3 } else { 2 })?,
        max_y: side(if three_d { 4 } else { 3 })?,
    })
}

fn intersects(a: &CrsBox, b: &CrsBox) -> bool {
    a.min_x <= b.max_x && b.min_x <= a.max_x && a.min_y <= b.max_y && b.min_y <= a.max_y
}

/// The link with rel "next", followed by GET, of a "links" array: its href.
/// nlohmann's type_error for a member of another type than text.
fn next_of(links: &Json) -> JsonResult<Option<String>> {
    for link in links.iter() {
        if link.is_object()
            && link.value("rel", String::new())? == "next"
            && link.value("method", String::from("GET"))? == "GET"
        {
            return link.value("href", String::new()).map(Some);
        }
    }
    Ok(None)
}

/// `%.17g`, made shorter when shorter reads back as the same double; never in
/// exponent form. The form every coordinate in a URL is written in, so a box
/// survives the round trip.
pub fn format_number(value: f64) -> String {
    let reads_back = |text: &str| text.parse::<f64>().ok() == Some(value);
    // Shorter when shorter reads back the same: 151.2 rather than
    // 151.19999999999999, which is the same double and an easier URL to read.
    // Never in exponent form, which not every service parses: 16830000, not
    // 1.683e+07. (format_general(v, p) is printf's "%.*g".)
    for precision in 1..=17 {
        let shorter = katana_core::text::format_general(value, precision);
        if !shorter.contains('e') && reads_back(&shorter) {
            return shorter;
        }
    }
    // A value %g will only write with an exponent: fixed notation, trimmed.
    // ("%.10f"; format_fixed drops the sign of an all-zero text, which can
    // only read back as a value already returned above.)
    let mut fixed = katana_core::text::format_fixed(value, 10);
    if reads_back(&fixed) {
        while fixed.ends_with('0') {
            fixed.pop();
        }
        if fixed.ends_with('.') {
            fixed.pop();
        }
        return fixed;
    }
    katana_core::text::format_general(value, 17)
}

/// `base` with `query` appended after '?' or '&' as the base needs.
pub fn with_query(base: &str, query: &str) -> String {
    if !base.contains('?') {
        return format!("{base}?{query}");
    }
    let joint = if base.ends_with(['?', '&']) { "" } else { "&" };
    format!("{base}{joint}{query}")
}

// ---- sizing an image request --------------------------------------------------------------

/// One image request: a box and the pixels asked for it. The box is the
/// requested area grown to a whole number of pixels, so the image's pixels are
/// square in the request CRS and the service has no reason to stretch or
/// re-fit it - ArcGIS's export, for one, silently widens a box whose aspect
/// does not match the image's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ImageRequest {
    pub r#box: CrsBox,
    pub width: i32,
    pub height: i32,
}

/// Splits `crs_box` into a grid of requests no larger than `max_pixels` on
/// either side at `units_per_pixel`, row by row from the north-west.
/// InvalidArgument for an empty box, a non-positive resolution, or a grid over
/// `max_requests` requests (the message gives the resolution that would fit).
/// The C++'s default for `max_requests` is 64.
pub fn plan_image_requests(
    crs_box: &CrsBox,
    units_per_pixel: f64,
    max_pixels: i32,
    max_requests: i32,
) -> Result<Vec<ImageRequest>> {
    if !crs_box.valid() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the area is empty or inverted",
        ));
    }
    if !(units_per_pixel > 0.0) || !units_per_pixel.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the resolution must be positive",
        ));
    }
    if max_pixels < 1 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the request size limit must be positive",
        ));
    }
    let total_columns = (crs_box.width() / units_per_pixel - 1e-9).ceil();
    let total_rows = (crs_box.height() / units_per_pixel - 1e-9).ceil();
    let across = (total_columns / f64::from(max_pixels)).ceil();
    let down = (total_rows / f64::from(max_pixels)).ceil();
    if across * down > f64::from(max_requests) {
        let fits = units_per_pixel * (across * down / f64::from(max_requests)).sqrt();
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "the area needs {} image requests at this resolution, more than the {max_requests} \
                 allowed; use a resolution of {} or coarser, or a smaller area",
                format_number(across * down),
                format_number((fits * 100.0).ceil() / 100.0)
            ),
        ));
    }
    let mut requests = Vec::new();
    // A count past i64's range is undefined in the C++ and saturates here.
    let columns = total_columns as i64;
    let rows = total_rows as i64;
    let step = i64::from(max_pixels);
    let mut row_start: i64 = 0;
    while row_start < rows {
        let row_count = step.min(rows - row_start);
        let mut column_start: i64 = 0;
        while column_start < columns {
            let column_count = step.min(columns - column_start);
            let min_x = crs_box.min_x + column_start as f64 * units_per_pixel;
            let max_y = crs_box.max_y - row_start as f64 * units_per_pixel;
            requests.push(ImageRequest {
                r#box: CrsBox {
                    min_x,
                    max_x: min_x + column_count as f64 * units_per_pixel,
                    max_y,
                    min_y: max_y - row_count as f64 * units_per_pixel,
                },
                width: column_count as i32,
                height: row_count as i32,
            });
            column_start += step;
        }
        row_start += step;
    }
    Ok(requests)
}

// ---- ArcGIS REST ----------------------------------------------------------------------------

/// MapServer /export of `request` in EPSG `epsg`: a PNG with transparency.
/// `layers` limits the drawn layers ("show:3") when the catalogue names one.
pub fn arcgis_export_url(layer: &OnlineLayer, request: &ImageRequest, epsg: i32) -> String {
    let mut query = format!(
        "bbox={}&bboxSR={epsg}&imageSR={epsg}&size={},{}&dpi=96&format=png32&transparent=true",
        box4(&request.r#box, false),
        request.width,
        request.height
    );
    if !layer.layer_name.is_empty() {
        query.push_str("&layers=");
        query.push_str(&percent_encode(&format!("show:{}", layer.layer_name)));
    }
    query.push_str("&f=image");
    with_query(&format!("{}/export", trim_slash(&layer.endpoint)), &query)
}

/// ImageServer /exportImage: Float32 GeoTIFF values, bilinear, with the
/// no-data value warp_to_geotiff uses.
pub fn arcgis_export_image_url(layer: &OnlineLayer, request: &ImageRequest, epsg: i32) -> String {
    // Elevation as Float32 values in a GeoTIFF; imagery as the picture, a PNG.
    let format = if layer.kind == OnlineLayerKind::Elevation {
        "&format=tiff&pixelType=F32&noData=-32767&interpolation=RSP_BilinearInterpolation"
    } else {
        "&format=png&pixelType=U8&interpolation=RSP_BilinearInterpolation"
    };
    let query = format!(
        "bbox={}&bboxSR={epsg}&imageSR={epsg}&size={},{}{format}&f=image",
        box4(&request.r#box, false),
        request.width,
        request.height
    );
    with_query(
        &format!("{}/exportImage", trim_slash(&layer.endpoint)),
        &query,
    )
}

/// FeatureServer/MapServer /<layer>/query: every field, geometry in WGS 84
/// (outSR=4326, f=geojson), the envelope `lon_lat` intersected, one page of
/// `count` from `offset` (resultOffset/resultRecordCount), ordered by
/// `order_by` when given (the C++'s default is empty) so pages cannot overlap
/// or skip.
pub fn arcgis_query_url(
    layer: &OnlineLayer,
    lon_lat: &CrsBox,
    offset: u64,
    count: i32,
    order_by: &str,
) -> String {
    let mut query = format!(
        "where={}&geometry={}&geometryType=esriGeometryEnvelope&inSR=4326\
         &spatialRel=esriSpatialRelIntersects&outFields=*&returnGeometry=true\
         &outSR=4326&resultOffset={offset}&resultRecordCount={count}",
        percent_encode("1=1"),
        box4(lon_lat, false)
    );
    if !order_by.is_empty() {
        query.push_str("&orderByFields=");
        query.push_str(&percent_encode(order_by));
    }
    query.push_str("&f=geojson");
    with_query(
        &format!(
            "{}/{}/query",
            trim_slash(&layer.endpoint),
            percent_encode(&layer.layer_name)
        ),
        &query,
    )
}

/// What one page of an ArcGIS query answer says about paging: how many
/// features it held and whether the server says there are more
/// (exceededTransferLimit, at the top level of Esri JSON or in "properties" of
/// its GeoJSON).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueryPage {
    pub features: u64,
    pub exceeded_transfer_limit: bool,
}

fn read_query_page_json(json: &[u8]) -> JsonResult<Result<QueryPage>> {
    let document = match Json::parse(json) {
        Ok(document) => document,
        Err(error) => {
            return Ok(Err(make_error(
                ErrorCode::ParseFailure,
                "the service's answer is not JSON",
                error.what,
            )));
        }
    };
    if document.is_object() && document.contains("error") {
        let error = &document["error"];
        let mut message = if error.is_object() {
            error.value("message", String::new())?
        } else {
            String::new()
        };
        if error.is_object() && error.contains("details") && error["details"].is_array() {
            for detail in error["details"].iter() {
                if let Some(text) = detail.as_str() {
                    message.push(' ');
                    message.push_str(text);
                }
            }
        }
        let context = if message.is_empty() {
            error.dump()
        } else {
            message
        };
        return Ok(Err(make_error(
            ErrorCode::FileImportFailure,
            "the service refused the query",
            context,
        )));
    }
    let features = match document.find("features") {
        Some(features) if features.is_array() => features,
        _ => {
            return Ok(Err(Error::new(
                ErrorCode::ParseFailure,
                "the answer holds no features list",
            )));
        }
    };
    let flag = |object: &Json| {
        object.is_object()
            && object
                .find("exceededTransferLimit")
                .and_then(Json::as_bool)
                .unwrap_or(false)
    };
    Ok(Ok(QueryPage {
        features: features.size() as u64,
        exceeded_transfer_limit: flag(&document) || document.find("properties").is_some_and(flag),
    }))
}

/// The paging of one page of an ArcGIS query answer. An ArcGIS error document
/// ({"error": {...}}) is FileImportFailure with the server's message.
pub fn read_query_page(json: impl AsRef<[u8]>) -> Result<QueryPage> {
    uncaught(read_query_page_json(json.as_ref()))
}

/// Whether another page must be asked for: the server said more exist, or the
/// page came back full (a server that ignores the flag).
pub fn query_has_more(page: &QueryPage, page_size: i32) -> bool {
    if page.features == 0 {
        return false;
    }
    page.exceeded_transfer_limit || page.features >= page_size as u64
}

// ---- OGC --------------------------------------------------------------------------------------

/// WMS GetMap. 1.3.0 names the CRS with CRS= and writes BBOX in the CRS's own
/// axis order (`axis_yx` for EPSG:4326: lat,lon); 1.1.1 names it SRS= and is
/// always x,y. The C++'s default `format` is "image/png".
pub fn wms_get_map_url(
    layer: &OnlineLayer,
    request: &ImageRequest,
    crs: &str,
    axis_yx: bool,
    format: &str,
) -> String {
    let version = if layer.version.is_empty() {
        "1.3.0"
    } else {
        layer.version.as_str()
    };
    let v13 = version.starts_with("1.3");
    let mut query = format!(
        "SERVICE=WMS&VERSION={version}&REQUEST=GetMap&LAYERS={}&STYLES=&{}{}&BBOX={}&WIDTH={}\
         &HEIGHT={}&FORMAT={}&TRANSPARENT=TRUE",
        percent_encode(&layer.layer_name),
        if v13 { "CRS=" } else { "SRS=" },
        percent_encode(crs),
        box4(&request.r#box, v13 && axis_yx),
        request.width,
        request.height,
        percent_encode(format)
    );
    if !layer.time.is_empty() && layer.time != "default" {
        query.push_str("&TIME=");
        query.push_str(&percent_encode(&layer.time));
    }
    with_query(&layer.endpoint, &query)
}

/// WCS 1.0.0 GetCoverage as a GeoTIFF: BBOX is always x,y in 1.0.0.
pub fn wcs_get_coverage_url(layer: &OnlineLayer, request: &ImageRequest, crs: &str) -> String {
    let query = format!(
        "SERVICE=WCS&VERSION=1.0.0&REQUEST=GetCoverage&COVERAGE={}&CRS={}&RESPONSE_CRS={}\
         &BBOX={}&WIDTH={}&HEIGHT={}&FORMAT=GeoTIFF",
        percent_encode(&layer.layer_name),
        percent_encode(crs),
        percent_encode(crs),
        box4(&request.r#box, false),
        request.width,
        request.height
    );
    with_query(&layer.endpoint, &query)
}

/// WFS 2.0.0 GetFeature, one page: TYPENAMES, COUNT, STARTINDEX, and BBOX in
/// `crs`'s axis order with the CRS appended (the 2.0 form); SRSNAME asks for
/// the answer in the same CRS. 1.1.0 uses TYPENAME and MAXFEATURES.
pub fn wfs_get_feature_url(
    layer: &OnlineLayer,
    crs_box: &CrsBox,
    crs: &str,
    axis_yx: bool,
    start_index: u64,
    count: i32,
) -> String {
    let version = if layer.version.is_empty() {
        "2.0.0"
    } else {
        layer.version.as_str()
    };
    let v2 = version.starts_with("2.");
    // A swapped (latitude-first) box is named by the URN, which every WFS 2.0
    // reads in the authority's order; the short EPSG:4326 some servers
    // (GeoServer) read as longitude first, and the box would land elsewhere.
    let name = match crs.strip_prefix("EPSG:") {
        Some(code) if axis_yx => format!("urn:ogc:def:crs:EPSG::{code}"),
        _ => String::from(crs),
    };
    let mut query = format!(
        "SERVICE=WFS&VERSION={version}&REQUEST=GetFeature&{}{}&SRSNAME={}&BBOX={},{}",
        if v2 { "TYPENAMES=" } else { "TYPENAME=" },
        percent_encode(&layer.layer_name),
        percent_encode(&name),
        box4(crs_box, axis_yx),
        percent_encode(&name)
    );
    if v2 {
        query.push_str(&format!("&COUNT={count}&STARTINDEX={start_index}"));
    } else {
        query.push_str(&format!("&MAXFEATURES={count}"));
    }
    with_query(&layer.endpoint, &query)
}

/// OGC API - Features items: bbox in CRS84 longitude/latitude (the default the
/// standard fixes), `limit` per page. Later pages follow the answer's own
/// "next" link (`next_link`).
pub fn oapif_items_url(layer: &OnlineLayer, lon_lat: &CrsBox, limit: i32) -> String {
    with_query(
        &format!(
            "{}/collections/{}/items",
            trim_slash(&layer.endpoint),
            percent_encode(&layer.layer_name)
        ),
        &format!("bbox={}&limit={limit}&f=json", box4(lon_lat, false)),
    )
}

/// The href of the link with rel "next" in an OGC API or STAC answer; empty
/// when there is none (the last page), and for anything that cannot be read.
pub fn next_link(json: impl AsRef<[u8]>) -> String {
    fn read(json: &[u8]) -> JsonResult<String> {
        let document = Json::parse(json)?;
        match document.find("links") {
            Some(links) if links.is_array() => Ok(next_of(links)?.unwrap_or_default()),
            _ => Ok(String::new()),
        }
    }
    read(json.as_ref()).unwrap_or_default()
}

/// A GeoJSON page of an OGC API answer read once: how many features it holds
/// and its "next" link (empty on the last page).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemsPage {
    pub features: u64,
    pub next: String,
}

fn read_items_page_json(json: &[u8]) -> JsonResult<Result<ItemsPage>> {
    let document = match Json::parse(json) {
        Ok(document) => document,
        Err(error) => {
            return Ok(Err(make_error(
                ErrorCode::ParseFailure,
                "the answer is not JSON",
                error.what,
            )));
        }
    };
    let features = match document.find("features") {
        Some(features) if features.is_array() => features,
        _ => {
            return Ok(Err(Error::new(
                ErrorCode::ParseFailure,
                "the answer is not a GeoJSON FeatureCollection",
            )));
        }
    };
    let mut page = ItemsPage {
        features: features.size() as u64,
        next: String::new(),
    };
    if let Some(links) = document.find("links")
        && links.is_array()
        && let Some(next) = next_of(links)?
    {
        page.next = next;
    }
    Ok(Ok(page))
}

/// The features and the "next" link of a GeoJSON page.
pub fn read_items_page(json: impl AsRef<[u8]>) -> Result<ItemsPage> {
    uncaught(read_items_page_json(json.as_ref()))
}

/// Features in a GeoJSON FeatureCollection answer (its "features" array).
pub fn count_geo_json_features(json: impl AsRef<[u8]>) -> Result<u64> {
    let document = Json::parse(json.as_ref()).map_err(|error| {
        make_error(
            ErrorCode::ParseFailure,
            "the answer is not JSON",
            error.what,
        )
    })?;
    match document.find("features") {
        Some(features) if features.is_array() => Ok(features.size() as u64),
        _ => Err(Error::new(
            ErrorCode::ParseFailure,
            "the answer is not a GeoJSON FeatureCollection",
        )),
    }
}

// ---- tiles ------------------------------------------------------------------------------------

/// One tile matrix of a tiled service (OGC WMTS 1.0, section 6.1): the scale,
/// the top-left corner in the matrix set's CRS (traditional order), the tile
/// size in pixels and the matrix size in tiles.
#[derive(Clone, Debug, PartialEq)]
pub struct TileMatrix {
    pub id: String,
    pub scale_denominator: f64,
    pub top_left_x: f64,
    pub top_left_y: f64,
    pub tile_width: i32,
    pub tile_height: i32,
    pub matrix_width: i64,
    pub matrix_height: i64,
}

impl Default for TileMatrix {
    fn default() -> Self {
        Self {
            id: String::new(),
            scale_denominator: 0.0,
            top_left_x: 0.0,
            top_left_y: 0.0,
            tile_width: 256,
            tile_height: 256,
            matrix_width: 1,
            matrix_height: 1,
        }
    }
}

impl TileMatrix {
    /// Ground units per pixel: the scale denominator times the 0.28 mm
    /// "standardized rendering pixel" of WMTS 1.0 (6.1), over the CRS's metres
    /// per unit (1 for metres, 2 pi 6378137 / 360 for degrees).
    pub fn units_per_pixel(&self, metres_per_unit: f64) -> f64 {
        self.scale_denominator * RENDERING_PIXEL_METRES / metres_per_unit
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TileMatrixSet {
    pub id: String,
    pub crs: String,
    pub metres_per_unit: f64,
    /// Coarsest first.
    pub matrices: Vec<TileMatrix>,
}

impl Default for TileMatrixSet {
    fn default() -> Self {
        Self {
            id: String::new(),
            crs: String::new(),
            metres_per_unit: 1.0,
            matrices: Vec::new(),
        }
    }
}

/// The Web Mercator grid every XYZ service uses - GoogleMapsCompatible in the
/// WMTS well-known scale sets (WMTS 1.0, annex E.4) - with levels 0 to
/// `max_zoom` (0 to 30): level z is 2^z tiles square and 559082264.0287178 /
/// 2^z the scale denominator.
pub fn web_mercator_tile_matrix_set(max_zoom: i32) -> TileMatrixSet {
    let mut set = TileMatrixSet {
        id: String::from("GoogleMapsCompatible"),
        crs: String::from("EPSG:3857"),
        metres_per_unit: 1.0,
        matrices: Vec::new(),
    };
    for z in 0..=max_zoom.clamp(0, 30) {
        // 2^z, exactly: std::ldexp(1.0, z) and 1 << z.
        let tiles = 1_i64 << z;
        set.matrices.push(TileMatrix {
            id: format!("{z}"),
            scale_denominator: WEB_MERCATOR_SCALE_0 / tiles as f64,
            top_left_x: -WEB_MERCATOR_HALF,
            top_left_y: WEB_MERCATOR_HALF,
            tile_width: 256,
            tile_height: 256,
            matrix_width: tiles,
            matrix_height: tiles,
        });
    }
    set
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TileIndex {
    /// An index into TileMatrixSet::matrices; the zoom for XYZ.
    pub matrix: i32,
    pub column: i64,
    pub row: i64,
}

/// The coarsest matrix at least as fine as `units_per_pixel`, or the finest
/// there is.
pub fn choose_tile_matrix(set: &TileMatrixSet, units_per_pixel: f64) -> i32 {
    if set.matrices.is_empty() {
        return 0;
    }
    for (index, matrix) in (0_i32..).zip(&set.matrices) {
        // A tolerance of one part in a million, so a request for exactly a
        // level's resolution gets that level and not the next finer one.
        if matrix.units_per_pixel(set.metres_per_unit) <= units_per_pixel * (1.0 + 1e-6) {
            return index;
        }
    }
    set.matrices.len() as i32 - 1
}

/// std::clamp<std::int64_t>(value, 0, high).
fn clamp_index(value: i64, high: i64) -> i64 {
    if value < 0 {
        0
    } else if high < value {
        high
    } else {
        value
    }
}

/// Tiles of matrix `level` meeting `crs_box` (in the set's CRS), clamped to
/// the matrix, row by row from the north-west.
pub fn tiles_covering(set: &TileMatrixSet, level: i32, crs_box: &CrsBox) -> Vec<TileIndex> {
    let mut tiles = Vec::new();
    if level < 0 || level as usize >= set.matrices.len() || !crs_box.valid() {
        return tiles;
    }
    let matrix = &set.matrices[level as usize];
    let pixel = matrix.units_per_pixel(set.metres_per_unit);
    let span_x = pixel * f64::from(matrix.tile_width);
    let span_y = pixel * f64::from(matrix.tile_height);
    // A floor past i64's range is undefined in the C++ and saturates here.
    let column_of = |value: f64| clamp_index(value.floor() as i64, matrix.matrix_width - 1);
    let row_of = |value: f64| clamp_index(value.floor() as i64, matrix.matrix_height - 1);
    // An edge exactly on a tile boundary belongs to the tile it opens, not the
    // one it closes: the max side is nudged inwards by a millionth of a tile,
    // so a box of exactly one tile asks for one tile.
    let first_column = column_of((crs_box.min_x - matrix.top_left_x) / span_x);
    let last_column = column_of((crs_box.max_x - matrix.top_left_x) / span_x - 1e-6);
    let first_row = row_of((matrix.top_left_y - crs_box.max_y) / span_y);
    let last_row = row_of((matrix.top_left_y - crs_box.min_y) / span_y - 1e-6);
    for row in first_row..=last_row {
        for column in first_column..=last_column {
            tiles.push(TileIndex {
                matrix: level,
                column,
                row,
            });
        }
    }
    tiles
}

/// The box a tile covers, in the set's CRS.
pub fn tile_box(set: &TileMatrixSet, tile: &TileIndex) -> CrsBox {
    let matrix = &set.matrices[tile.matrix as usize];
    let pixel = matrix.units_per_pixel(set.metres_per_unit);
    let span_x = pixel * f64::from(matrix.tile_width);
    let span_y = pixel * f64::from(matrix.tile_height);
    let min_x = matrix.top_left_x + tile.column as f64 * span_x;
    let max_y = matrix.top_left_y - tile.row as f64 * span_y;
    CrsBox {
        min_x,
        max_x: min_x + span_x,
        max_y,
        min_y: max_y - span_y,
    }
}

/// A tile URL from a template: {z} {x} {y} (and {TileMatrix} {TileCol}
/// {TileRow}) for the tile, {layer} {time} and anything in `extra` by name.
/// An empty `matrix_id` is the tile's matrix index.
pub fn expand_tile_template(
    pattern: &str,
    tile: &TileIndex,
    matrix_id: &str,
    extra: &BTreeMap<String, String>,
) -> String {
    let z = if matrix_id.is_empty() {
        format!("{}", tile.matrix)
    } else {
        String::from(matrix_id)
    };
    let column = format!("{}", tile.column);
    let row = format!("{}", tile.row);
    let mut url = String::from(pattern);
    url = replace_all(url, "{z}", &z);
    url = replace_all(url, "{TileMatrix}", &z);
    url = replace_all(url, "{x}", &column);
    url = replace_all(url, "{TileCol}", &column);
    url = replace_all(url, "{y}", &row);
    url = replace_all(url, "{TileRow}", &row);
    for (name, value) in extra {
        url = replace_all(url, &format!("{{{name}}}"), &percent_encode(value));
    }
    url
}

// ---- Copernicus DEM one-degree tiles -----------------------------------------------------------

/// The one-degree tiles meeting `lon_lat`, as {lat} and {lon} strings in the
/// naming the Copernicus DEM uses ("S34", "E151": the tile's south-west
/// corner), filled into `pattern`.
pub fn degree_tile_urls(pattern: &str, lon_lat: &CrsBox) -> Vec<String> {
    let mut urls = Vec::new();
    // A degree past i32's range is undefined in the C++ and saturates here.
    let south = lon_lat.min_y.floor() as i32;
    let north = lon_lat.max_y.ceil() as i32 - 1;
    let west = lon_lat.min_x.floor() as i32;
    let east = lon_lat.max_x.ceil() as i32 - 1;
    let mut lat = north.max(south);
    while lat >= south {
        for lon in west..=east.max(west) {
            let lat_text = format!(
                "{}{:02}",
                if lat < 0 { 'S' } else { 'N' },
                lat.unsigned_abs()
            );
            let lon_text = format!(
                "{}{:03}",
                if lon < 0 { 'W' } else { 'E' },
                lon.unsigned_abs()
            );
            let url = replace_all(String::from(pattern), "{lat}", &lat_text);
            urls.push(replace_all(url, "{lon}", &lon_text));
        }
        lat -= 1;
    }
    urls
}

// ---- STAC ---------------------------------------------------------------------------------------

/// A STAC API item search body (STAC API - Item Search 1.0): the collection,
/// the intersected bbox, the datetime interval, and a cloud ceiling through
/// the query extension on eo:cloud_cover, sorted by cloud cover ascending.
pub fn stac_search_body(
    collection: &str,
    lon_lat: &CrsBox,
    from_date: &str,
    to_date: &str,
    max_cloud: Option<f64>,
    limit: i32,
) -> String {
    let mut body = Json::object();
    body["collections"] = Json::array_of([Json::from(collection)]);
    body["bbox"] = Json::array_of([
        Json::from(lon_lat.min_x),
        Json::from(lon_lat.min_y),
        Json::from(lon_lat.max_x),
        Json::from(lon_lat.max_y),
    ]);
    let from = if from_date.is_empty() {
        String::from("..")
    } else {
        format!("{from_date}T00:00:00Z")
    };
    let to = if to_date.is_empty() {
        String::from("..")
    } else {
        format!("{to_date}T23:59:59Z")
    };
    body["datetime"] = Json::from(format!("{from}/{to}"));
    body["limit"] = Json::from(limit);
    if let Some(max_cloud) = max_cloud {
        body["query"] = Json::object_of([(
            "eo:cloud_cover",
            Json::object_of([("lt", Json::from(max_cloud))]),
        )]);
    }
    body["sortby"] = Json::array_of([Json::object_of([
        ("field", Json::from("properties.eo:cloud_cover")),
        ("direction", Json::from("asc")),
    ])]);
    body.dump()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StacItem {
    pub id: String,
    /// As the item gives it.
    pub datetime: String,
    pub cloud_cover: Option<f64>,
    /// Longitude/latitude.
    pub bounds: CrsBox,
    /// key -> href
    pub assets: BTreeMap<String, String>,
}

fn parse_stac_items_json(json: &[u8]) -> JsonResult<Result<Vec<StacItem>>> {
    let document = match Json::parse(json) {
        Ok(document) => document,
        Err(error) => {
            return Ok(Err(make_error(
                ErrorCode::ParseFailure,
                "the STAC answer is not JSON",
                error.what,
            )));
        }
    };
    let mut features: Vec<&Json> = Vec::new();
    if document.is_object()
        && document.value("type", String::new())? == "FeatureCollection"
        && let Some(found) = document.find("features")
        && found.is_array()
    {
        features.extend(found.iter());
    } else if document.is_object() && document.value("type", String::new())? == "Feature" {
        features.push(&document);
    } else {
        return Ok(Err(Error::new(
            ErrorCode::ParseFailure,
            "the STAC answer is neither an item nor an item collection",
        )));
    }
    let mut items = Vec::new();
    for feature in features {
        let mut item = StacItem {
            id: feature.value("id", String::new())?,
            ..StacItem::default()
        };
        if let Some(properties) = feature.find("properties")
            && properties.is_object()
        {
            item.datetime = properties.value("datetime", String::new())?;
            if let Some(cloud) = properties.find("eo:cloud_cover")
                && cloud.is_number()
            {
                item.cloud_cover = Some(cloud.get::<f64>()?);
            }
        }
        if let Some(bbox) = feature.find("bbox")
            && let Some(bounds) = bbox_of(bbox)
        {
            item.bounds = bounds;
        }
        if let Some(assets) = feature.find("assets")
            && assets.is_object()
        {
            for (key, asset) in assets.items() {
                if let Some(href) = asset.find("href").and_then(Json::as_str) {
                    item.assets.insert(key.into_owned(), String::from(href));
                }
            }
        }
        items.push(item);
    }
    Ok(Ok(items))
}

/// The items of a search answer (a FeatureCollection) or of a single item.
/// ParseFailure for anything else.
pub fn parse_stac_items(json: impl AsRef<[u8]>) -> Result<Vec<StacItem>> {
    uncaught(parse_stac_items_json(json.as_ref()))
}

/// The items to mosaic for an area: those of one acquisition day (see
/// `choose_stac_items`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StacChoice {
    pub items: Vec<StacItem>,
    pub day: String,
    pub cloud_cover: f64,
    pub covers_area: bool,
}

/// The first `count` bytes of `text`; a character they cut through is
/// U+FFFD, as all text from outside is decoded (`std::string::substr`).
pub(crate) fn byte_prefix(text: &str, count: usize) -> String {
    String::from_utf8_lossy(&text.as_bytes()[..text.len().min(count)]).into_owned()
}

/// The items to mosaic for `lon_lat`: those of the one acquisition day, among
/// the days whose items together cover the area, with the least cloud (the
/// latest day on a tie). A day that covers only part of the area is used only
/// when no day covers all of it, with a warning. Empty when nothing meets the
/// area at all.
pub fn choose_stac_items(items: &[StacItem], lon_lat: &CrsBox) -> StacChoice {
    // Grouped by acquisition day: the tiles of one pass mosaic without seams
    // of season or light, which tiles of different days do not.
    let mut days: BTreeMap<String, Vec<&StacItem>> = BTreeMap::new();
    for item in items {
        if item.bounds.valid() && intersects(&item.bounds, lon_lat) {
            days.entry(byte_prefix(&item.datetime, 10))
                .or_default()
                .push(item);
        }
    }
    // Whether the day's footprints cover the area, tested on a 5 x 5 grid of
    // points across it: the bounding boxes are what a search returns, and a
    // grid finds the gap between two of them that a corner test would miss.
    let covers = |day_items: &[&StacItem]| {
        for i in 0..=4 {
            for j in 0..=4 {
                let x = lon_lat.min_x + lon_lat.width() * f64::from(i) / 4.0;
                let y = lon_lat.min_y + lon_lat.height() * f64::from(j) / 4.0;
                let inside = day_items.iter().any(|item| {
                    x >= item.bounds.min_x
                        && x <= item.bounds.max_x
                        && y >= item.bounds.min_y
                        && y <= item.bounds.max_y
                });
                if !inside {
                    return false;
                }
            }
        }
        true
    };
    let mut best = StacChoice::default();
    let mut have_best = false;
    for (day, day_items) in &days {
        let mut cloud = 0.0;
        for item in day_items {
            cloud = cpp::max(cloud, item.cloud_cover.unwrap_or(100.0));
        }
        let full = covers(day_items);
        // Covering beats not covering; then less cloud; then the later day
        // (the map iterates days in ascending order, so <= prefers later).
        let better = !have_best
            || (full && !best.covers_area)
            || (full == best.covers_area && cloud <= best.cloud_cover);
        if better {
            best.items = day_items.iter().copied().cloned().collect();
            best.day.clone_from(day);
            best.cloud_cover = cloud;
            best.covers_area = full;
            have_best = true;
        }
    }
    best
}

/// s3://bucket/key hrefs, which some catalogues give, as the bucket's https
/// address; anything else unchanged.
pub fn http_href(href: &str) -> String {
    let Some(rest) = href.strip_prefix("s3://") else {
        return String::from(href);
    };
    match rest.find('/') {
        Some(slash) => format!(
            "https://{}.s3.amazonaws.com{}",
            &rest[..slash],
            &rest[slash..]
        ),
        None => String::from(href),
    }
}

// ---- Overpass ------------------------------------------------------------------------------------

/// Overpass QL for every node, way and relation matching `filter` in
/// `lon_lat`, with its referenced nodes, as OSM XML - what GDAL's OSM driver
/// reads. The filter is a tag test: `key`, `key=value`, `key!=value` or several
/// joined by ';' ("building", "highway=primary", "amenity=school;name").
/// Anything else (quotes, brackets, braces, a second statement) is
/// InvalidArgument, so no text a person types can become more than the one
/// query. [timeout:] and [maxsize:] bound the server's work, as the Overpass
/// commons policy asks.
pub fn overpass_query(
    filter: &str,
    lon_lat: &CrsBox,
    timeout_seconds: i32,
    max_bytes: u64,
) -> Result<String> {
    if !lon_lat.valid() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the area is empty or inverted",
        ));
    }
    let text = trimmed_text(filter);
    if text.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "an OpenStreetMap query needs a tag: tag=building, tag=highway=primary",
        ));
    }
    let mut tests = String::new();
    for piece in text.split(';') {
        let term = trimmed_text(piece);
        if term.is_empty() {
            continue;
        }
        let (key, value, op) = if let Some(not_equal) = term.find("!=") {
            (
                trimmed_text(&term[..not_equal]),
                trimmed_text(&term[not_equal + 2..]),
                "!=",
            )
        } else if let Some(equal) = term.find('=') {
            (
                trimmed_text(&term[..equal]),
                trimmed_text(&term[equal + 1..]),
                "=",
            )
        } else {
            (term, "", "")
        };
        if !plain_tag_text(key) || (!op.is_empty() && !plain_tag_text(value)) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "an OpenStreetMap tag test is key, key=value or key!=value, of letters, digits \
                 and _ : - . only",
                term,
            ));
        }
        if op.is_empty() {
            tests.push_str(&format!("[\"{key}\"]"));
        } else {
            tests.push_str(&format!("[\"{key}\"{op}\"{value}\"]"));
        }
    }
    if tests.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the OpenStreetMap filter holds no tag",
        ));
    }
    // Overpass's bbox is (south, west, north, east).
    let bbox = format!(
        "{},{},{},{}",
        format_number(lon_lat.min_y),
        format_number(lon_lat.min_x),
        format_number(lon_lat.max_y),
        format_number(lon_lat.max_x)
    );
    Ok(format!(
        "[out:xml][timeout:{}][maxsize:{max_bytes}][bbox:{bbox}];\n(\n  node{tests};\n  \
         way{tests};\n  relation{tests};\n);\n(._;>;);\nout body;\n",
        timeout_seconds.max(1)
    ))
}

/// The area of a longitude/latitude box in square kilometres (on the sphere of
/// radius 6371 km), for the Overpass and tile area limits.
pub fn area_km2(lon_lat: &CrsBox) -> f64 {
    let degree = std::f64::consts::PI / 180.0;
    // The area of a latitude band's slice on the sphere: R^2 (lon2 - lon1)
    // (sin lat2 - sin lat1).
    EARTH_RADIUS_KM
        * EARTH_RADIUS_KM
        * (lon_lat.max_x - lon_lat.min_x)
        * degree
        * ((lon_lat.max_y * degree).sin() - (lon_lat.min_y * degree).sin()).abs()
}

// ---- CKAN -----------------------------------------------------------------------------------------

pub fn ckan_search_url(endpoint: &str, words: &str, rows: i32) -> String {
    // Datasets with at least one web-service resource, as CKAN's facet query
    // res_format spells them.
    let formats = "res_format:(WMS OR WFS OR WMTS OR WCS OR \"ESRI REST\" OR \"OGC API\")";
    with_query(
        endpoint,
        &format!(
            "q={}&fq={}&rows={rows}",
            percent_encode(words),
            percent_encode(formats)
        ),
    )
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CkanResource {
    pub dataset: String,
    pub title: String,
    /// WMS, WFS, ESRI REST, ...
    pub format: String,
    pub url: String,
    pub licence: String,
}

fn parse_ckan_search_json(json: &[u8]) -> JsonResult<Result<Vec<CkanResource>>> {
    let document = match Json::parse(json) {
        Ok(document) => document,
        Err(error) => {
            return Ok(Err(make_error(
                ErrorCode::ParseFailure,
                "the catalogue's answer is not JSON",
                error.what,
            )));
        }
    };
    if !document.is_object()
        || !document.value("success", false)?
        || !document.contains("result")
        || !document["result"].contains("results")
    {
        return Ok(Err(Error::new(
            ErrorCode::ParseFailure,
            "the catalogue's answer holds no results",
        )));
    }
    const SERVICE_FORMATS: [&str; 7] = [
        "WMS",
        "WFS",
        "WMTS",
        "WCS",
        "ESRI REST",
        "OGC API",
        "OGC API - FEATURES",
    ];
    let mut resources = Vec::new();
    for dataset in document["result"]["results"].iter() {
        let Some(dataset_resources) = dataset.find("resources") else {
            continue;
        };
        let dataset_title = dataset.value("title", String::new())?;
        let licence = dataset.value("license_title", String::new())?;
        for resource in dataset_resources.iter() {
            if !resource.is_object() {
                continue;
            }
            let format = resource
                .value("format", String::new())?
                .to_ascii_uppercase();
            let url = resource.value("url", String::new())?;
            if !SERVICE_FORMATS.contains(&format.as_str()) || url.is_empty() {
                continue;
            }
            resources.push(CkanResource {
                dataset: dataset_title.clone(),
                title: resource.value("name", dataset_title.clone())?,
                format,
                url,
                licence: licence.clone(),
            });
        }
    }
    Ok(Ok(resources))
}

/// The web-service resources of a package_search answer: WMS, WMTS, WFS, WCS,
/// OGC API and ArcGIS REST, which ONLINE CUSTOM can open.
pub fn parse_ckan_search(json: impl AsRef<[u8]>) -> Result<Vec<CkanResource>> {
    uncaught(parse_ckan_search_json(json.as_ref()))
}
