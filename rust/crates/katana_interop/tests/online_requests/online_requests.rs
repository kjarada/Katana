//! tests/interop/test_online_requests.cpp: requests for every kind of web
//! service (docs/gis_online.md, "How each service is asked"), and the pieces
//! under them: the web access helpers, the XML reader, the coordinate
//! transformation with its axis order, the tile arithmetic, the STAC choice and
//! the Overpass query. No network: every request is compared with what the
//! service's own specification says it must be, and every number with one
//! worked out here from the specification's formula - never with what the code
//! printed.

use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::sync::atomic::AtomicBool;

use katana_core::{Error, ErrorCode};
use katana_gis::gdal_adapter::{GeoPoint, GeometryKind, VectorFeature, VectorGeometry};
use katana_gis::reproject::{
    CrsBox, crs_axis_is_yx, crs_epsg_code, reproject_features, transform_box, transform_point,
};
use katana_gis::web_access::{
    HttpFailure, HttpRequest, carries_credentials, explain_http_failure, http_exchange, http_fetch,
    parse_xml, percent_encode, redact_url, sha256_hex,
};
use katana_interop::online_catalogue::{OnlineLayer, OnlineLayerKind, OnlineServiceType};
use katana_interop::online_requests::{
    ImageRequest, TileIndex, arcgis_export_image_url, arcgis_export_url, arcgis_query_url,
    area_km2, choose_stac_items, choose_tile_matrix, ckan_search_url, count_geo_json_features,
    degree_tile_urls, expand_tile_template, format_number, http_href, next_link, oapif_items_url,
    overpass_query, parse_ckan_search, parse_stac_items, plan_image_requests, query_has_more,
    read_query_page, stac_search_body, tile_box, tiles_covering, wcs_get_coverage_url,
    web_mercator_tile_matrix_set, wfs_get_feature_url, with_query, wms_get_map_url,
};

use crate::online_support::{ONLINE_TEST_DATA, double_eq, fixture, holds, must, near, refused};

fn layer_of(r#type: OnlineServiceType, endpoint: &str, name: &str) -> OnlineLayer {
    OnlineLayer {
        r#type,
        endpoint: String::from(endpoint),
        layer_name: String::from(name),
        ..OnlineLayer::default()
    }
}

fn request_for(url: &str) -> HttpRequest {
    HttpRequest {
        url: String::from(url),
        ..HttpRequest::default()
    }
}

/// `CrsBox{min_x, min_y, max_x, max_y}`.
fn crs_box(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> CrsBox {
    CrsBox {
        min_x,
        min_y,
        max_x,
        max_y,
    }
}

// Web Mercator (EPSG:3857, the spherical form of EPSG guidance note 7-2,
// 1.3.3.2) worked from its formula: x = R lambda, y = R ln tan(pi/4 + phi/2).
fn mercator_x(lon: f64) -> f64 {
    6378137.0 * lon * PI / 180.0
}

fn mercator_y(lat: f64) -> f64 {
    6378137.0 * (PI / 4.0 + lat * PI / 360.0).tan().ln()
}

/// `std::ldexp(1.0, z)`.
fn two_to(z: i32) -> f64 {
    (1_i64 << z) as f64
}

// ---- web access helpers ---------------------------------------------------------------

mod online_web {
    use super::*;

    #[test]
    fn sha256_matches_the_standards_vectors() {
        // FIPS 180-2, appendix B: "abc", the empty string, and the 448-bit message.
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // A million 'a's crosses many blocks (B.3).
        assert_eq!(
            sha256_hex("a".repeat(1_000_000)),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn redact_url_hides_keys_and_credentials_and_nothing_else() {
        assert_eq!(
            redact_url("https://a.org/x?layer=1&key=SECRET&f=json"),
            "https://a.org/x?layer=1&key=***&f=json"
        );
        assert_eq!(
            redact_url("https://a.org/x?apiKey=S&access_token=T&token=U&sig=V"),
            "https://a.org/x?apiKey=***&access_token=***&token=***&sig=***"
        );
        assert_eq!(redact_url("https://user:pass@a.org/x"), "https://a.org/x");
        assert_eq!(
            redact_url("https://a.org/tiles/{z}/{x}/{y}.png"),
            "https://a.org/tiles/{z}/{x}/{y}.png"
        );
        // A parameter whose name merely contains "key" at its start is not a key.
        assert_eq!(
            redact_url("https://a.org/x?keyword=roads"),
            "https://a.org/x?keyword=roads"
        );
    }

    #[test]
    fn percent_encode_keeps_only_the_unreserved_set() {
        // RFC 3986, 2.3: ALPHA DIGIT - . _ ~ stay; everything else is %XX, and a
        // space is %20.
        assert_eq!(percent_encode("AZaz09-._~"), "AZaz09-._~");
        assert_eq!(
            percent_encode("1=1 & show:9/é"),
            "1%3D1%20%26%20show%3A9%2F%C3%A9"
        );
    }

    #[test]
    fn a_file_url_is_read_without_the_network_and_its_limit_holds() {
        let missing = refused(
            http_fetch(&request_for("file:///no/such/file.json"), None, None),
            "a file URL to nothing",
        );
        assert_eq!(missing.code, ErrorCode::NotFound);
        let wrong = refused(
            http_fetch(&request_for("gopher://example.org/x"), None, None),
            "a gopher URL",
        );
        assert_eq!(wrong.code, ErrorCode::InvalidArgument);

        let mut request = request_for(&format!("file://{ONLINE_TEST_DATA}/ckan_search.json"));
        let read = must(http_fetch(&request, None, None));
        assert!(holds(&read.body, "package_search"));
        assert_eq!(read.content_type, "application/json");
        request.max_bytes = 10;
        let too_big = refused(http_fetch(&request, None, None), "a file past the limit");
        assert_eq!(too_big.code, ErrorCode::Unsupported);
        assert!(
            too_big.message.contains("smaller area"),
            "{}",
            too_big.message
        );
    }

    #[test]
    fn a_stopped_fetch_ends_as_cancelled_before_any_request() {
        let stop = AtomicBool::new(true);
        let stopped = refused(
            http_fetch(
                &request_for("https://example.invalid/never"),
                Some(&stop),
                None,
            ),
            "a stopped fetch",
        );
        assert_eq!(stopped.code, ErrorCode::InvalidState);
        assert_eq!(stopped.message, "cancelled");
    }

    #[test]
    fn an_exchange_refuses_what_it_cannot_send_before_any_connection_is_made() {
        // http_exchange is one GET that follows no redirect (web_access). What it
        // does on the wire needs a server and is the update system's end-to-end
        // tests'; what it refuses, it refuses before libcurl is asked for
        // anything, so these reach no network. The host is RFC 2606's .invalid,
        // which no resolver answers, should one ever get that far.
        //
        // Unlike http_fetch it reads no file: a caller that decides hop by hop
        // where its headers go has no use for one, and a file:// address in a
        // Location must not be an answer.
        for url in [
            "file:///no/such/file.json",
            "gopher://example.invalid/x",
            "example.invalid/x",
            "",
        ] {
            let error = refused(http_exchange(&request_for(url), None, None), url);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{url}");
        }
        // A GET only.
        let post = HttpRequest {
            post_body: String::from("{}"),
            ..request_for("https://example.invalid/search")
        };
        let with_body = refused(http_exchange(&post, None, None), "an exchange with a body");
        assert_eq!(with_body.code, ErrorCode::InvalidArgument);

        // A stop asked for before the start is "cancelled", with no request made;
        // and neither the request's headers nor a secret in its address is in the
        // error.
        let stop = AtomicBool::new(true);
        let request = HttpRequest {
            headers: vec![String::from("Authorization: Bearer SECRET-IN-HEADER")],
            ..request_for("https://example.invalid/asset?token=SECRET-IN-QUERY")
        };
        let stopped = refused(
            http_exchange(&request, Some(&stop), None),
            "a stopped exchange",
        );
        assert_eq!(stopped.code, ErrorCode::InvalidState);
        assert_eq!(stopped.message, "cancelled");
        let said = stopped.describe();
        assert!(!said.contains("SECRET-IN-QUERY"), "{said}");
        assert!(!said.contains("SECRET-IN-HEADER"), "{said}");
    }

    #[test]
    fn a_request_carries_credentials_when_a_header_is_an_authorization() {
        // The rule by which an exchange keeps the certificate check on whatever
        // GDAL_HTTP_UNSAFESSL says (web_access). RFC 9110: field names are
        // case-insensitive (5.1); Authorization (11.6.2) and Proxy-Authorization
        // (11.7.2) are the two that carry credentials.
        let headers = |lines: &[&str]| -> Vec<String> {
            lines.iter().map(|line| String::from(*line)).collect()
        };
        assert!(carries_credentials(&headers(&[
            "Authorization: Bearer abc"
        ])));
        assert!(carries_credentials(&headers(&[
            "Accept: */*",
            "authorization:Basic abc"
        ])));
        assert!(
            carries_credentials(&headers(&["AUTHORIZATION : x"])),
            "blanks about the name"
        );
        assert!(carries_credentials(&headers(&[
            "Proxy-Authorization: Basic abc"
        ])));
        assert!(
            carries_credentials(&headers(&["Authorization"])),
            "malformed, and not let through for it"
        );

        assert!(!carries_credentials(&headers(&[])));
        assert!(!carries_credentials(&headers(&[
            "Accept: application/octet-stream",
            "Range: bytes=0-99",
            "X-GitHub-Api-Version: 2022-11-28",
        ])));
        // The name must BE the field, not hold it or be named in a value.
        assert!(!carries_credentials(&headers(&[
            "X-Authorization-Hint: none"
        ])));
        assert!(!carries_credentials(&headers(&["Accept: Authorization"])));
        assert!(!carries_credentials(&headers(&[
            "WWW-Authenticate: Bearer"
        ])));
    }

    #[test]
    fn a_failure_is_explained_by_what_actually_stopped_it() {
        let explain = |status: i32, curl_code: i32, error: &str, body: &str| {
            let failure = HttpFailure {
                status,
                curl_code,
                error: String::from(error),
                body: body.as_bytes().to_vec(),
                timeout_seconds: 120,
            };
            explain_http_failure(&failure, "https://a.org/x")
        };
        let says = |error: &Error, words: &str| error.message.contains(words);

        // The certificates https:// is checked against are missing - what every
        // request from the deployed program did while the deploy left out
        // etc/ssl/certs/ca-bundle.crt, reported by libcurl in these words.
        let no_bundle = explain(
            0,
            77,
            "error adding trust anchors from file: C:/k/bin/../etc/ssl/certs/ca-bundle.crt",
            "",
        );
        assert_eq!(no_bundle.code, ErrorCode::FileImportFailure);
        assert!(says(&no_bundle, "ca-bundle.crt"), "{}", no_bundle.message);
        // It names the file libcurl was given, as libcurl wrote it - a path
        // mangled by a code page shows as it arrived - and no longer says to
        // reinstall as if that were the cure for every cause (an install under a
        // folder named outside the ANSI code page was not helped by it).
        assert!(
            says(&no_bundle, "C:/k/bin/../etc/ssl/certs/ca-bundle.crt"),
            "{}",
            no_bundle.message
        );
        assert!(says(&no_bundle, "CURL_CA_BUNDLE"), "{}", no_bundle.message);
        assert!(
            !says(&no_bundle, "reinstall or rebuild"),
            "{}",
            no_bundle.message
        );
        assert!(says(
            &explain(0, 35, "error adding trust anchors from file: x", ""),
            "certificates"
        ));
        assert!(says(
            &explain(0, 60, "SSL certificate problem", ""),
            "could not be verified"
        ));

        // CloudFront's own 403 page: the network in front of the service
        // refused, typically a service that answers only from its own country.
        let blocked = explain(
            403,
            22,
            "HTTP error code : 403",
            "<HTML><TITLE>ERROR: The request could not be satisfied</TITLE>\
             <H1>403 ERROR</H1> Request blocked. We can't connect",
        );
        assert!(says(&blocked, "own country"), "{}", blocked.message);
        // Any other 403 is the service's own refusal.
        let service_refused = explain(
            403,
            22,
            "HTTP error code : 403",
            "{\"error\":\"forbidden\"}",
        );
        assert!(
            says(&service_refused, "ONLINE KEY"),
            "{}",
            service_refused.message
        );
        assert!(!says(&service_refused, "own country"));

        assert_eq!(
            explain(404, 22, "HTTP error code : 404", "").code,
            ErrorCode::NotFound
        );
        assert!(says(&explain(429, 22, "", ""), "limiting"));
        assert!(says(
            &explain(0, 6, "Could not resolve host", ""),
            "host name"
        ));
        assert!(says(
            &explain(0, 28, "Operation timed out", ""),
            "within 120 s"
        ));
        assert!(says(
            &explain(0, 56, "CONNECT tunnel failed, response 403", ""),
            "proxy"
        ));
        let other = explain(0, 7, "Failed to connect", "");
        assert_eq!(other.message, "the request failed");
        assert_eq!(other.context, "https://a.org/x");
    }

    #[test]
    fn xml_is_read_into_elements_by_local_name() {
        let root = must(parse_xml(concat!(
            r#"<?xml version="1.0"?><!-- c --><a:Root xmlns:a="urn:x" a:version="2"><a:Child>one</a:Child>"#,
            r#"<Child xlink:href="h"> two </Child><Other/></a:Root>"#
        )));
        assert_eq!(root.name, "a:Root");
        assert_eq!(root.local_name(), "Root");
        assert_eq!(root.attribute("version"), "2");
        assert_eq!(root.children_named("Child").len(), 2);
        assert_eq!(root.child_text("Child"), "one");
        assert_eq!(root.children_named("Child")[1].text, "two");
        assert_eq!(root.children_named("Child")[1].attribute("href"), "h");
        assert!(root.child("Missing").is_none());
        assert_eq!(
            refused(parse_xml("not xml at all <"), "text that is not XML").code,
            ErrorCode::ParseFailure
        );
    }
}

// ---- coordinate systems and axis order -------------------------------------------------

mod online_crs {
    use super::*;

    #[test]
    fn axis_order_is_the_authoritys_not_the_traditional_one() {
        // EPSG:4326 and GDA2020 geographic are latitude first by the EPSG
        // registry; CRS84, Web Mercator and MGA are easting first.
        assert!(must(crs_axis_is_yx("EPSG:4326")));
        assert!(must(crs_axis_is_yx("EPSG:7844")));
        assert!(!must(crs_axis_is_yx("OGC:CRS84")));
        assert!(!must(crs_axis_is_yx("EPSG:3857")));
        assert!(!must(crs_axis_is_yx("EPSG:7856")));
        assert_eq!(
            refused(crs_axis_is_yx("EPSG:999999"), "an unknown code").code,
            ErrorCode::InvalidCrs
        );
        assert_eq!(crs_epsg_code("EPSG:28356"), Some(28356));
    }

    #[test]
    fn points_go_where_the_formula_and_an_independent_tool_put_them() {
        // Web Mercator from its formula.
        let mercator = must(transform_point(
            151.2053,
            -33.8697,
            "EPSG:4326",
            "EPSG:3857",
        ));
        near(mercator[0], mercator_x(151.2053), 1e-6);
        near(mercator[1], mercator_y(-33.8697), 1e-6);
        // MGA zone 56 (GDA2020): gdaltransform -s_srs EPSG:7844 -t_srs EPSG:7856
        // gives 334000.349071675 6250842.08445121 for this point - the command
        // line tool, outside Katana. Traditional order in, easting first out.
        let mga = must(transform_point(
            151.2053,
            -33.8697,
            "EPSG:7844",
            "EPSG:7856",
        ));
        near(mga[0], 334000.349071675, 1e-3);
        near(mga[1], 6250842.08445121, 1e-3);
    }

    #[test]
    fn a_box_is_densified_so_it_encloses_its_curved_image() {
        // One degree around Sydney into MGA 56: the box must hold every edge
        // point's image, which a corners-only transform does not guarantee.
        let lon_lat = crs_box(150.5, -34.5, 151.5, -33.5);
        let image = must(transform_box(&lon_lat, "EPSG:7844", "EPSG:7856"));
        for i in 0..=20 {
            let t = f64::from(i) / 20.0;
            for (x, y) in [
                (150.5 + t, -34.5),
                (150.5 + t, -33.5),
                (150.5, -34.5 + t),
                (151.5, -34.5 + t),
            ] {
                let p = must(transform_point(x, y, "EPSG:7844", "EPSG:7856"));
                assert!(p[0] >= image.min_x - 1e-6, "{p:?} {image:?}");
                assert!(p[0] <= image.max_x + 1e-6, "{p:?} {image:?}");
                assert!(p[1] >= image.min_y - 1e-6, "{p:?} {image:?}");
                assert!(p[1] <= image.max_y + 1e-6, "{p:?} {image:?}");
            }
        }
        assert_eq!(
            refused(
                transform_box(&crs_box(1.0, 1.0, 0.0, 2.0), "EPSG:4326", "EPSG:3857"),
                "an inverted box"
            )
            .code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn features_are_moved_vertex_by_vertex_with_heights_kept() {
        let feature = VectorFeature {
            geometry: VectorGeometry {
                kind: GeometryKind::LineString,
                has_z: true,
                parts: vec![vec![
                    GeoPoint {
                        x: 151.2053,
                        y: -33.8697,
                        z: 12.5,
                    },
                    GeoPoint {
                        x: 151.21,
                        y: -33.87,
                        z: 13.0,
                    },
                ]],
                ..VectorGeometry::default()
            },
            attributes: BTreeMap::new(),
        };
        let moved = must(reproject_features(
            vec![feature.clone()],
            "EPSG:7844",
            "EPSG:7856",
        ));
        let first = moved[0].geometry.parts[0][0];
        near(first.x, 334000.349071675, 1e-3);
        near(first.y, 6250842.08445121, 1e-3);
        double_eq(first.z, 12.5);
        assert_eq!(moved[0].attributes, feature.attributes);
    }
}

// ---- image sizing, ArcGIS REST, OGC and the small helpers -------------------------------

mod online_requests {
    use super::*;

    #[test]
    fn an_image_request_is_grown_to_whole_pixels_and_split_at_the_service_limit() {
        let one = must(plan_image_requests(
            &crs_box(0.0, 0.0, 1000.0, 500.0),
            1.0,
            2048,
            64,
        ));
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].width, 1000);
        assert_eq!(one[0].height, 500);
        // 1000.5 units at 1 per pixel is 1001 pixels: the box grows to match.
        let grown = must(plan_image_requests(
            &crs_box(0.0, 0.0, 1000.5, 10.0),
            1.0,
            2048,
            64,
        ));
        assert_eq!(grown[0].width, 1001);
        double_eq(grown[0].r#box.max_x, 1001.0);
        // 5000 x 3000 pixels at a 2048 limit: 3 across, 2 down, north-west
        // first, the pieces tiling the area exactly.
        let split = must(plan_image_requests(
            &crs_box(0.0, 0.0, 5000.0, 3000.0),
            1.0,
            2048,
            64,
        ));
        assert_eq!(split.len(), 6);
        assert_eq!(split[0].width, 2048);
        assert_eq!(split[2].width, 5000 - 4096);
        assert_eq!(split[3].height, 3000 - 2048);
        double_eq(split[0].r#box.max_y, 3000.0);
        double_eq(split[0].r#box.min_x, 0.0);
        double_eq(split[5].r#box.min_y, 0.0);
        double_eq(split[5].r#box.max_x, 5000.0);
        let mut area = 0.0;
        for request in &split {
            area += request.r#box.width() * request.r#box.height();
            // `request.width * 1.0` in the C++: the pixel count as a double.
            double_eq(request.r#box.width(), f64::from(request.width));
        }
        double_eq(area, 5000.0 * 3000.0);
    }

    #[test]
    fn too_many_image_requests_say_which_resolution_would_fit() {
        let too_many = refused(
            plan_image_requests(&crs_box(0.0, 0.0, 100000.0, 100000.0), 1.0, 1000, 64),
            "a grid of 10000 requests",
        );
        assert_eq!(too_many.code, ErrorCode::InvalidArgument);
        assert!(
            too_many.message.contains("10000 image requests"),
            "{}",
            too_many.message
        );
        assert!(
            too_many.message.contains("resolution of 12.5"),
            "{}",
            too_many.message
        );
    }

    #[test]
    fn arcgis_export_asks_for_exactly_the_box_and_size() {
        let layer = layer_of(
            OnlineServiceType::ArcgisExport,
            "https://maps.example.org/arcgis/rest/services/Topo/MapServer/",
            "3",
        );
        let request = ImageRequest {
            r#box: crs_box(16830000.0, -4011000.0, 16832000.0, -4010000.0),
            width: 2000,
            height: 1000,
        };
        assert_eq!(
            arcgis_export_url(&layer, &request, 3857),
            concat!(
                "https://maps.example.org/arcgis/rest/services/Topo/MapServer/export?",
                "bbox=16830000,-4011000,16832000,-4010000&bboxSR=3857&imageSR=3857&size=2000,1000",
                "&dpi=96&format=png32&transparent=true&layers=show%3A3&f=image"
            )
        );
    }

    #[test]
    fn arcgis_export_image_asks_for_values_for_elevation_and_a_picture_otherwise() {
        let mut layer = layer_of(
            OnlineServiceType::ArcgisExportImage,
            "https://maps.example.org/arcgis/rest/services/DEM/ImageServer",
            "",
        );
        layer.kind = OnlineLayerKind::Elevation;
        let request = ImageRequest {
            r#box: crs_box(0.0, 0.0, 100.0, 50.0),
            width: 20,
            height: 10,
        };
        assert_eq!(
            arcgis_export_image_url(&layer, &request, 3857),
            concat!(
                "https://maps.example.org/arcgis/rest/services/DEM/ImageServer/exportImage?",
                "bbox=0,0,100,50&bboxSR=3857&imageSR=3857&size=20,10&format=tiff&pixelType=F32",
                "&noData=-32767&interpolation=RSP_BilinearInterpolation&f=image"
            )
        );
        layer.kind = OnlineLayerKind::Imagery;
        assert!(
            arcgis_export_image_url(&layer, &request, 3857).contains("&format=png&pixelType=U8")
        );
    }

    #[test]
    fn arcgis_query_pages_by_offset_in_wgs84() {
        let layer = layer_of(
            OnlineServiceType::ArcgisQuery,
            "https://maps.example.org/arcgis/rest/services/Cadastre/MapServer",
            "9",
        );
        assert_eq!(
            arcgis_query_url(
                &layer,
                &crs_box(151.2, -33.875, 151.215, -33.865),
                2000,
                1000,
                "objectid"
            ),
            concat!(
                "https://maps.example.org/arcgis/rest/services/Cadastre/MapServer/9/query?",
                "where=1%3D1&geometry=151.2,-33.875,151.215,-33.865&geometryType=esriGeometryEnvelope",
                "&inSR=4326&spatialRel=esriSpatialRelIntersects&outFields=*&returnGeometry=true",
                "&outSR=4326&resultOffset=2000&resultRecordCount=1000&orderByFields=objectid&f=geojson"
            )
        );
    }

    #[test]
    fn a_query_page_says_whether_more_follow() {
        let page = must(read_query_page(
            r#"{"type":"FeatureCollection","features":[{},{}],"properties":{"exceededTransferLimit":true}}"#,
        ));
        assert_eq!(page.features, 2);
        assert!(page.exceeded_transfer_limit);
        assert!(query_has_more(&page, 1000));
        // Esri JSON puts the flag at the top.
        let esri = must(read_query_page(
            r#"{"features":[{}],"exceededTransferLimit":true}"#,
        ));
        assert!(esri.exceeded_transfer_limit);
        // No flag, but a full page: a server that omits it is asked again.
        let full = must(read_query_page(r#"{"features":[{},{},{}]}"#));
        assert!(query_has_more(&full, 3));
        assert!(!query_has_more(&full, 4));
        assert!(!query_has_more(
            &must(read_query_page(r#"{"features":[]}"#)),
            4
        ));
        // The server's refusal, with its own words.
        let server_refused = refused(
            read_query_page(
                r#"{"error":{"code":400,"message":"Invalid parameters.","details":["'resultOffset' is not supported"]}}"#,
            ),
            "an ArcGIS error document",
        );
        assert_eq!(server_refused.code, ErrorCode::FileImportFailure);
        assert!(
            server_refused.context.contains("resultOffset"),
            "{}",
            server_refused.context
        );
    }

    #[test]
    fn wms130_writes_a_geographic_box_latitude_first() {
        let mut layer = layer_of(
            OnlineServiceType::Wms,
            "https://ows.example.org/wms?",
            "a b",
        );
        layer.version = String::from("1.3.0");
        let request = ImageRequest {
            r#box: crs_box(150.0, -34.0, 151.0, -33.0),
            width: 512,
            height: 512,
        };
        // WMS 1.3.0, 6.7.4: BBOX in the CRS's axis order - lat,lon for EPSG:4326.
        assert_eq!(
            wms_get_map_url(&layer, &request, "EPSG:4326", true, "image/png"),
            concat!(
                "https://ows.example.org/wms?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS=a%20b",
                "&STYLES=&CRS=EPSG%3A4326&BBOX=-34,150,-33,151&WIDTH=512&HEIGHT=512",
                "&FORMAT=image%2Fpng&TRANSPARENT=TRUE"
            )
        );
        // In Web Mercator the order is x,y.
        let mercator = ImageRequest {
            r#box: crs_box(1.0, 2.0, 3.0, 4.0),
            width: 8,
            height: 8,
        };
        assert!(
            wms_get_map_url(&layer, &mercator, "EPSG:3857", false, "image/png")
                .contains("&BBOX=1,2,3,4&")
        );
        // WMS 1.1.1 is always x,y and says SRS.
        layer.version = String::from("1.1.1");
        let old = wms_get_map_url(&layer, &request, "EPSG:4326", true, "image/png");
        assert!(
            old.contains("&SRS=EPSG%3A4326&BBOX=150,-34,151,-33&"),
            "{old}"
        );
        layer.version = String::from("1.3.0");
        layer.time = String::from("2026-01-01");
        assert!(
            wms_get_map_url(&layer, &request, "EPSG:3857", false, "image/png")
                .contains("&TIME=2026-01-01")
        );
    }

    #[test]
    fn wcs_asks_for_a_geo_tiff_of_the_box() {
        let layer = layer_of(
            OnlineServiceType::Wcs,
            "https://services.example.org/DEM/MapServer/WCSServer",
            "1",
        );
        let request = ImageRequest {
            r#box: crs_box(151.0, -34.0, 151.5, -33.5),
            width: 1800,
            height: 1800,
        };
        assert_eq!(
            wcs_get_coverage_url(&layer, &request, "EPSG:4326"),
            concat!(
                "https://services.example.org/DEM/MapServer/WCSServer?SERVICE=WCS&VERSION=1.0.0",
                "&REQUEST=GetCoverage&COVERAGE=1&CRS=EPSG%3A4326&RESPONSE_CRS=EPSG%3A4326",
                "&BBOX=151,-34,151.5,-33.5&WIDTH=1800&HEIGHT=1800&FORMAT=GeoTIFF"
            )
        );
    }

    #[test]
    fn wfs20_pages_and_follows_the_authoritys_axis_order() {
        let mut layer = layer_of(
            OnlineServiceType::Wfs,
            "https://wfs.example.org/ows",
            "ns:parcels",
        );
        assert_eq!(
            wfs_get_feature_url(
                &layer,
                &crs_box(144.9, -37.9, 145.0, -37.8),
                "EPSG:4326",
                true,
                1000,
                500
            ),
            concat!(
                "https://wfs.example.org/ows?SERVICE=WFS&VERSION=2.0.0&REQUEST=GetFeature",
                "&TYPENAMES=ns%3Aparcels&SRSNAME=urn%3Aogc%3Adef%3Acrs%3AEPSG%3A%3A4326",
                "&BBOX=-37.9,144.9,-37.8,145,urn%3Aogc%3Adef%3Acrs%3AEPSG%3A%3A4326",
                "&COUNT=500&STARTINDEX=1000"
            )
        );
        layer.version = String::from("1.1.0");
        let old = wfs_get_feature_url(
            &layer,
            &crs_box(1.0, 2.0, 3.0, 4.0),
            "EPSG:3111",
            false,
            0,
            50,
        );
        assert!(old.contains("&TYPENAME=ns%3Aparcels&"), "{old}");
        assert!(old.contains("&MAXFEATURES=50"), "{old}");
        assert!(!old.contains("STARTINDEX"), "{old}");
    }

    #[test]
    fn ogc_api_features_asks_in_crs84_and_follows_next_links() {
        let layer = layer_of(
            OnlineServiceType::OgcApiFeatures,
            "https://api.example.org/ogc/",
            "buildings",
        );
        assert_eq!(
            oapif_items_url(&layer, &crs_box(151.2, -33.9, 151.3, -33.8), 500),
            concat!(
                "https://api.example.org/ogc/collections/buildings/items?",
                "bbox=151.2,-33.9,151.3,-33.8&limit=500&f=json"
            )
        );
        assert_eq!(
            next_link(r#"{"links":[{"rel":"self","href":"a"},{"rel":"next","href":"b"}]}"#),
            "b"
        );
        assert_eq!(next_link(r#"{"links":[{"rel":"self","href":"a"}]}"#), "");
        // A next link that must be POSTed (a STAC search's) is not followed as a
        // GET.
        assert_eq!(
            next_link(r#"{"links":[{"rel":"next","href":"b","method":"POST"}]}"#),
            ""
        );
        assert_eq!(
            must(count_geo_json_features(
                r#"{"type":"FeatureCollection","features":[{},{}]}"#
            )),
            2
        );
    }

    #[test]
    fn numbers_are_shortest_exact() {
        assert_eq!(format_number(151.2), "151.2");
        assert_eq!(format_number(-33.875), "-33.875");
        assert_eq!(format_number(16830000.0), "16830000");
        assert_eq!(format_number(0.1 + 0.2).parse::<f64>(), Ok(0.1 + 0.2));
        assert_eq!(with_query("https://a/b", "x=1"), "https://a/b?x=1");
        assert_eq!(with_query("https://a/b?", "x=1"), "https://a/b?x=1");
        assert_eq!(with_query("https://a/b?k=v", "x=1"), "https://a/b?k=v&x=1");
    }
}

// ---- tiles ------------------------------------------------------------------------------------

mod online_tiles {
    use super::*;

    #[test]
    fn the_web_mercator_grid_is_the_well_known_scale_set() {
        let set = web_mercator_tile_matrix_set(19);
        assert_eq!(set.matrices.len(), 20);
        // WMTS 1.0 annex E.4: level 0 is 156543.0339280410 m per pixel.
        near(
            set.matrices[0].units_per_pixel(1.0),
            156543.0339280410,
            1e-6,
        );
        near(
            set.matrices[19].units_per_pixel(1.0),
            156543.0339280410 / 524288.0,
            1e-9,
        );
        assert_eq!(set.matrices[19].matrix_width, 524288);
        // 0.597 m is the first at or below 1 m.
        assert_eq!(choose_tile_matrix(&set, 1.0), 18);
        // Finer than the service: its finest.
        assert_eq!(choose_tile_matrix(&set, 0.001), 19);
        assert_eq!(choose_tile_matrix(&set, 156543.0339280410), 0);
    }

    #[test]
    fn the_tiles_of_an_area_are_the_slippy_map_tiles() {
        // The OpenStreetMap wiki's slippy map formula, worked here:
        // x = floor((lon + 180) / 360 * 2^z), y = floor((1 - asinh(tan(lat)) / pi) / 2 * 2^z).
        let tile_x = |lon: f64, z: i32| ((lon + 180.0) / 360.0 * two_to(z)).floor() as i64;
        let tile_y = |lat: f64, z: i32| {
            let r = lat * PI / 180.0;
            ((1.0 - katana_math::libm::asinh(r.tan()) / PI) / 2.0 * two_to(z)).floor() as i64
        };
        let set = web_mercator_tile_matrix_set(19);
        let lon_lat = crs_box(151.20, -33.875, 151.215, -33.865);
        let area = crs_box(
            mercator_x(lon_lat.min_x),
            mercator_y(lon_lat.min_y),
            mercator_x(lon_lat.max_x),
            mercator_y(lon_lat.max_y),
        );
        let tiles = tiles_covering(&set, 16, &area);
        let x0 = tile_x(151.20, 16);
        let x1 = tile_x(151.215, 16);
        let y0 = tile_y(-33.865, 16);
        let y1 = tile_y(-33.875, 16);
        assert_eq!(tiles.len(), ((x1 - x0 + 1) * (y1 - y0 + 1)) as usize);
        assert_eq!(tiles[0].column, x0);
        assert_eq!(tiles[0].row, y0);
        assert_eq!(tiles[tiles.len() - 1].column, x1);
        assert_eq!(tiles[tiles.len() - 1].row, y1);
        // A tile's box is where the formula puts its corner.
        let corner = tile_box(&set, &tiles[0]);
        near(
            corner.min_x,
            -20037508.342789244 + x0 as f64 * 40075016.68557849 / 65536.0,
            1e-6,
        );
        near(
            corner.max_y,
            20037508.342789244 - y0 as f64 * 40075016.68557849 / 65536.0,
            1e-6,
        );
        // Exactly one tile's box asks for exactly that tile.
        let exact = tiles_covering(&set, 16, &corner);
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0].column, x0);
        assert_eq!(exact[0].row, y0);
    }

    #[test]
    fn templates_fill_every_placeholder() {
        let tile = TileIndex {
            matrix: 5,
            column: 29,
            row: 19,
        };
        let none = BTreeMap::new();
        assert_eq!(
            expand_tile_template("https://t.org/{z}/{x}/{y}.png", &tile, "", &none),
            "https://t.org/5/29/19.png"
        );
        assert_eq!(
            expand_tile_template("https://t.org/tile/{z}/{y}/{x}", &tile, "", &none),
            "https://t.org/tile/5/19/29"
        );
        let extra = BTreeMap::from([
            (String::from("layer"), String::from("MODIS_Terra")),
            (String::from("time"), String::from("2026-09-24")),
        ]);
        assert_eq!(
            expand_tile_template(
                "https://g.org/{layer}/default/{time}/Set/{z}/{y}/{x}.jpg",
                &tile,
                "",
                &extra
            ),
            "https://g.org/MODIS_Terra/default/2026-09-24/Set/5/19/29.jpg"
        );
        assert_eq!(
            expand_tile_template(
                "https://w.org/?M={TileMatrix}&R={TileRow}&C={TileCol}",
                &tile,
                "L5",
                &none
            ),
            "https://w.org/?M=L5&R=19&C=29"
        );
    }

    #[test]
    fn copernicus_tiles_are_named_by_their_south_west_corner() {
        let pattern = "https://c.org/Copernicus_DSM_COG_10_{lat}_00_{lon}_00_DEM.tif";
        let one = degree_tile_urls(pattern, &crs_box(151.2, -33.875, 151.215, -33.865));
        assert_eq!(one.len(), 1);
        assert_eq!(
            one[0],
            "https://c.org/Copernicus_DSM_COG_10_S34_00_E151_00_DEM.tif"
        );
        let four = degree_tile_urls(pattern, &crs_box(-0.5, -0.5, 0.5, 0.5));
        assert_eq!(four.len(), 4);
        assert_eq!(
            four[0],
            "https://c.org/Copernicus_DSM_COG_10_N00_00_W001_00_DEM.tif"
        );
        assert_eq!(
            four[3],
            "https://c.org/Copernicus_DSM_COG_10_S01_00_E000_00_DEM.tif"
        );
    }
}

// ---- STAC ------------------------------------------------------------------------------------

mod online_stac {
    use super::*;

    #[test]
    fn the_search_body_is_item_search_with_the_query_extension() {
        assert_eq!(
            stac_search_body(
                "sentinel-2-l2a",
                &crs_box(151.2, -33.9, 151.3, -33.8),
                "2026-01-01",
                "2026-03-31",
                Some(20.0),
                100
            ),
            concat!(
                r#"{"bbox":[151.2,-33.9,151.3,-33.8],"collections":["sentinel-2-l2a"],"#,
                r#""datetime":"2026-01-01T00:00:00Z/2026-03-31T23:59:59Z","limit":100,"#,
                r#""query":{"eo:cloud_cover":{"lt":20.0}},"#,
                r#""sortby":[{"direction":"asc","field":"properties.eo:cloud_cover"}]}"#
            )
        );
        assert!(
            stac_search_body("c", &crs_box(0.0, 0.0, 1.0, 1.0), "", "", None, 5)
                .contains(r#""datetime":"../..""#)
        );
    }

    #[test]
    fn a_real_earth_search_item_is_read() {
        // stac_item_sentinel2.json: the item the sentinel-cogs bucket holds for
        // S2B_56HLH_20260103_0_L2A, trimmed to the fields Katana reads.
        let items = must(parse_stac_items(fixture("stac_item_sentinel2.json")));
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.id, "S2B_56HLH_20260103_0_L2A");
        assert_eq!(&item.datetime[..10], "2026-01-03");
        let cloud = item.cloud_cover.expect("the item's cloud cover");
        near(cloud, 93.501586, 1e-6);
        near(item.bounds.min_x, 150.82399, 1e-9);
        near(item.bounds.max_y, -33.420387, 1e-9);
        assert!(item.assets["visual"].ends_with("/TCI.tif"));
        assert!(item.assets.contains_key("red"));
    }

    #[test]
    fn the_chosen_day_covers_the_area_with_the_least_cloud() {
        let items = must(parse_stac_items(fixture("stac_search.json")));
        assert_eq!(items.len(), 5);
        // 2026-01-03 covers the area in two halves at 12 % at worst; 2026-01-08
        // covers it in one at 5 %; 2026-01-13 is clearer but covers only the
        // east.
        let choice = choose_stac_items(&items, &crs_box(151.0, -34.0, 151.4, -33.6));
        assert_eq!(choice.day, "2026-01-08");
        assert!(choice.covers_area);
        assert_eq!(choice.items.len(), 1);
        assert_eq!(choice.items[0].id, "S2B_FULL_20260108");
        // A corner the 8th and the 13th both cover: the clearer, later 13th.
        let east = choose_stac_items(&items, &crs_box(151.45, -34.05, 151.55, -33.95));
        assert!(east.covers_area);
        // It covers this corner, with 1 %.
        assert_eq!(east.day, "2026-01-13");
        assert!(
            choose_stac_items(&items, &crs_box(10.0, 10.0, 11.0, 11.0))
                .items
                .is_empty()
        );
        assert_eq!(
            http_href("s3://sentinel-cogs/a/b.tif"),
            "https://sentinel-cogs.s3.amazonaws.com/a/b.tif"
        );
    }
}

// ---- Overpass --------------------------------------------------------------------------------

mod online_overpass {
    use super::*;

    #[test]
    fn the_query_is_one_statement_with_its_limits() {
        let query = must(overpass_query(
            "building",
            &crs_box(151.2, -33.875, 151.215, -33.865),
            90,
            1000000,
        ));
        assert_eq!(
            query,
            concat!(
                "[out:xml][timeout:90][maxsize:1000000][bbox:-33.875,151.2,-33.865,151.215];\n",
                "(\n  node[\"building\"];\n  way[\"building\"];\n  relation[\"building\"];\n);\n",
                "(._;>;);\nout body;\n"
            )
        );
        let two = must(overpass_query(
            "highway=primary; name",
            &crs_box(0.0, 0.0, 1.0, 1.0),
            25,
            5,
        ));
        assert!(
            two.contains("way[\"highway\"=\"primary\"][\"name\"];"),
            "{two}"
        );
        let negated = must(overpass_query(
            "building!=no",
            &crs_box(0.0, 0.0, 1.0, 1.0),
            25,
            5,
        ));
        assert!(negated.contains("[\"building\"!=\"no\"]"), "{negated}");
    }

    #[test]
    fn nothing_typed_can_become_a_second_statement() {
        for hostile in [
            "building\"];out;",
            "a[b]",
            "x{y}",
            "name=\"x\"",
            "a\\b",
            "",
            ";",
        ] {
            let error = refused(
                overpass_query(hostile, &crs_box(0.0, 0.0, 1.0, 1.0), 25, 5),
                hostile,
            );
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{hostile}");
        }
    }

    #[test]
    fn area_is_on_the_sphere() {
        // One degree square on the equator: R^2 (pi/180) sin(1 deg) = 12 363.7
        // km2, worked from the spherical zone formula with R = 6371 km.
        near(
            area_km2(&crs_box(0.0, 0.0, 1.0, 1.0)),
            6371.0 * 6371.0 * (PI / 180.0) * (PI / 180.0).sin(),
            1e-6,
        );
        // The Sydney CBD box of the live checks: 0.015 degrees of longitude at
        // 33.87 S (1.386 km) by 0.01 degrees of latitude (1.112 km), 1.54 km2.
        near(
            area_km2(&crs_box(151.2, -33.875, 151.215, -33.865)),
            1.386 * 1.112,
            0.01,
        );
    }
}

// ---- CKAN ---------------------------------------------------------------------------------------

mod online_ckan {
    use super::*;

    #[test]
    fn the_search_finds_web_services_only() {
        assert_eq!(
            ckan_search_url(
                "https://data.gov.au/data/api/3/action/package_search",
                "nsw cadastre",
                50
            ),
            concat!(
                "https://data.gov.au/data/api/3/action/package_search?q=nsw%20cadastre&fq=res_format%3A%28WMS",
                "%20OR%20WFS%20OR%20WMTS%20OR%20WCS%20OR%20%22ESRI%20REST%22%20OR%20%22OGC%20API%22%29&rows=50"
            )
        );
        let found = must(parse_ckan_search(fixture("ckan_search.json")));
        // The ZIP download is not a service.
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].format, "WMS");
        assert_eq!(found[1].format, "ESRI REST");
        assert_eq!(found[1].licence, "Creative Commons Attribution 4.0");
    }
}
