//! tests/interop/test_online_fetch.cpp, its OnlineFetch cases and the
//! Copernicus live case: fetch_online_layer end to end - paging, joining, the
//! cache, cancellation, every limit, keys, and rasters warped into the
//! project's CRS - with NO network. Requests go to a transport the test
//! supplies (OnlineEnvironment::transport), which answers from fixtures and
//! counts what it was asked; a local file goes through the real fetch path as
//! a file:// URL.
//!
//! The file's discovery cases and two of its live ones are in
//! tests/online_requests; its live case that sets GDAL_HTTP_USERPWD is in
//! tests/online_environment. The live case here reaches the network, and runs
//! only with KATANA_ONLINE_TESTS=1, as the C++ one does.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};

use katana_core::error::make_error;
use katana_core::{ErrorCode, Result};
use katana_entity::Geometry;
use katana_entity::entity::property_to_string;
use katana_gis::gdal_adapter::{GdalDataset, RasterExportOptions};
use katana_gis::reproject::{CrsBox, crs_to_wkt, transform_box};
use katana_gis::web_access::{HttpRequest, HttpResponse};
use katana_interop::online_catalogue::{
    OnlineLayer, OnlineLayerKind, OnlineServiceType, built_in_catalogue,
};
use katana_interop::online_fetch::{
    OnlineEnvironment, OnlineImport, OnlineRequestOptions, OnlineTransport, fetch_online_layer,
    online_user_agent, prune_cache,
};

use crate::online_support::{LIVE_SKIP, ONLINE_TEST_DATA, fixture, live_tests_enabled};
use crate::{TempDir, double_eq, must, near, refused, warned};

fn temp_dir(name: &str) -> TempDir {
    TempDir::new("katana-online-", name)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A fixture's bytes, as the C++'s readFile gives them.
fn fixture_bytes(name: &str) -> Vec<u8> {
    fixture(name).into_bytes()
}

/// `file://` and the path, as the C++'s fileUrl writes one.
fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

type RequestHook = Box<dyn Fn(&HttpRequest) + Send + Sync>;

/// Answers requests from a table of (a piece the URL or body must contain ->
/// the body to return), first match wins; counts every request and keeps the
/// URLs in order. `on_request`, when set, runs first - a case uses it to
/// press Cancel part way through. Shared with the environment's transport,
/// which the environment owns, so its tables are behind locks.
#[derive(Default)]
struct FakeTransport {
    answers: Mutex<Vec<(String, Vec<u8>)>>,
    urls: Mutex<Vec<String>>,
    bodies: Mutex<Vec<String>>,
    on_request: Mutex<Option<RequestHook>>,
}

impl FakeTransport {
    fn with(answers: Vec<(&str, Vec<u8>)>) -> Arc<Self> {
        let transport = Arc::new(Self::default());
        transport.set_answers(answers);
        transport
    }

    fn set_answers(&self, answers: Vec<(&str, Vec<u8>)>) {
        *lock(&self.answers) = answers
            .into_iter()
            .map(|(piece, body)| (String::from(piece), body))
            .collect();
    }

    fn urls(&self) -> Vec<String> {
        lock(&self.urls).clone()
    }

    fn bodies(&self) -> Vec<String> {
        lock(&self.bodies).clone()
    }

    fn answer(&self, request: &HttpRequest) -> Result<HttpResponse> {
        if let Some(on_request) = lock(&self.on_request).as_ref() {
            on_request(request);
        }
        lock(&self.urls).push(request.url.clone());
        lock(&self.bodies).push(request.post_body.clone());
        for (piece, body) in lock(&self.answers).iter() {
            if request.url.contains(piece.as_str())
                || (!request.post_body.is_empty() && request.post_body.contains(piece.as_str()))
            {
                return Ok(HttpResponse {
                    status: 200,
                    body: body.clone(),
                    ..HttpResponse::default()
                });
            }
        }
        Err(make_error(
            ErrorCode::NotFound,
            "the service has nothing at this address",
            request.url.clone(),
        ))
    }
}

fn environment_with(transport: &Arc<FakeTransport>, cache: &Path) -> OnlineEnvironment {
    let fake = Arc::clone(transport);
    let sender: OnlineTransport =
        Arc::new(move |request: &HttpRequest, _stop: Option<&AtomicBool>| fake.answer(request));
    OnlineEnvironment {
        cache_directory: cache.to_path_buf(),
        user_agent: online_user_agent("test"),
        transport: Some(sender),
        ..OnlineEnvironment::default()
    }
}

fn built_in_layer(provider: &str, layer: &str) -> OnlineLayer {
    let catalogue = must(built_in_catalogue());
    let found = catalogue
        .find_provider(provider)
        .unwrap_or_else(|| panic!("no provider {provider}"));
    found
        .find_layer(layer)
        .unwrap_or_else(|| panic!("no layer {layer}"))
        .clone()
}

/// The Sydney CBD area of the live checks, in WGS 84.
fn sydney_options(target_crs: &str) -> OnlineRequestOptions {
    OnlineRequestOptions {
        area: CrsBox {
            min_x: 151.2,
            min_y: -33.875,
            max_x: 151.215,
            max_y: -33.865,
        },
        area_crs: String::from("EPSG:4326"),
        target_crs: String::from(target_crs),
        ..OnlineRequestOptions::default()
    }
}

/// A single-band GeoTIFF whose value at (column, row) is 1000 row + column,
/// so any sample says where it was read from.
fn write_grid(
    path: &Path,
    crs: &str,
    origin_x: f64,
    origin_y: f64,
    pixel: f64,
    columns: i32,
    rows: i32,
) -> PathBuf {
    let options = RasterExportOptions {
        width: columns,
        height: rows,
        projection_wkt: must(crs_to_wkt(crs)),
        geotransform: [origin_x, pixel, 0.0, origin_y, 0.0, -pixel],
        no_data_value: Some(-9999.0),
        ..RasterExportOptions::default()
    };
    let mut values = Vec::with_capacity(columns as usize * rows as usize);
    for row in 0..rows {
        for column in 0..columns {
            values.push(1000.0 * f64::from(row) + f64::from(column));
        }
    }
    must(GdalDataset::write_raster(path, &options, &values));
    path.to_path_buf()
}

fn fetch(
    layer: &OnlineLayer,
    options: &OnlineRequestOptions,
    environment: &OnlineEnvironment,
) -> Result<OnlineImport> {
    fetch_online_layer(layer, options, environment, None, None)
}

fn metadata_text(entity: &katana_entity::Entity, key: &str) -> String {
    property_to_string(&entity.metadata[key])
}

/// Every path under `root`, folders too.
fn every_path(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(folder) = pending.pop() {
        let Ok(entries) = fs::read_dir(folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path.clone());
            }
            found.push(path);
        }
    }
    found
}

// ---- vectors -----------------------------------------------------------------------------------

mod online_fetch {
    use super::*;

    #[test]
    fn arcgis_pages_are_joined_without_loss_or_repeat_and_moved_into_the_project_crs() {
        let cache = temp_dir("arcgis-paging");
        let transport = FakeTransport::with(vec![
            (
                "resultOffset=0&",
                fixture_bytes("arcgis_query_page1.geojson"),
            ),
            (
                "resultOffset=4&",
                fixture_bytes("arcgis_query_page2.geojson"),
            ),
            (
                "resultOffset=8&",
                fixture_bytes("arcgis_query_page3.geojson"),
            ),
        ]);
        let environment = environment_with(&transport, cache.path());
        let mut lots = built_in_layer("nsw-spatial", "lots");
        lots.page_size = 4;

        let imported = must(fetch(&lots, &sydney_options("EPSG:7856"), &environment));
        let vectors = imported.vectors.as_ref().expect("vectors");
        // Pages of 4, 4 and 3 where the second repeats one of the first: ten
        // lots, each once.
        assert_eq!(vectors.entities.len(), 10);
        assert_eq!(imported.stats.duplicates, 1);
        assert_eq!(imported.stats.pages, 3);
        assert_eq!(imported.stats.requests, 3);
        let mut ids = BTreeSet::new();
        for entity in &vectors.entities {
            ids.insert(property_to_string(&entity.properties["cadid"]));
            assert_eq!(entity.layer, "online/nsw-spatial/lots");
            assert_eq!(metadata_text(entity, "online.licence"), "CC BY 4.0");
            assert!(metadata_text(entity, "online.attribution").contains("Spatial Services"));
            assert_eq!(metadata_text(entity, "online.source"), lots.endpoint);
        }
        assert_eq!(ids.len(), 10);
        assert_eq!(
            vectors.layers_needed,
            vec![String::from("online/nsw-spatial/lots")]
        );

        // Lot 1's first vertex is 151.2050, -33.8700; gdaltransform -s_srs
        // EPSG:4326 -t_srs EPSG:7856 puts it at 333973.178351782
        // 6250808.33086044.
        let Geometry::Polyline(polygon) = &vectors.entities[0].geometry else {
            panic!("the first lot is a polyline");
        };
        near(polygon.vertices[0].x, 333973.178351782, 1e-3);
        near(polygon.vertices[0].y, 6250808.33086044, 1e-3);

        // The requests were the builder's: WGS 84 envelope, offsets by the
        // count each page actually held.
        let urls = transport.urls();
        assert_eq!(urls.len(), 3);
        assert!(
            urls[0].contains("/NSW_Cadastre/MapServer/9/query?"),
            "{}",
            urls[0]
        );
        assert!(
            urls[1].contains("resultOffset=4&resultRecordCount=4"),
            "{}",
            urls[1]
        );

        // Again: every page from the cache, nothing asked.
        let again = must(fetch(&lots, &sydney_options("EPSG:7856"), &environment));
        assert_eq!(again.stats.requests, 0);
        assert_eq!(again.stats.cache_hits, 3);
        assert_eq!(again.vectors.as_ref().expect("vectors").entities.len(), 10);
        assert_eq!(transport.urls().len(), 3);
    }

    #[test]
    fn a_server_that_ignores_paging_ends_with_a_warning_not_a_loop() {
        let cache = temp_dir("arcgis-no-paging");
        let transport = FakeTransport::with(vec![(
            "/query?",
            fixture_bytes("arcgis_query_page1.geojson"),
        )]);
        let mut lots = built_in_layer("nsw-spatial", "lots");
        lots.page_size = 4;
        let imported = must(fetch(
            &lots,
            &sydney_options("EPSG:7856"),
            &environment_with(&transport, cache.path()),
        ));
        assert_eq!(
            imported.vectors.as_ref().expect("vectors").entities.len(),
            4
        );
        assert_eq!(transport.urls().len(), 2);
        assert!(warned(&imported.warnings, "may not support paging"));
    }

    #[test]
    fn the_feature_limit_is_a_refusal_that_says_what_to_do() {
        let cache = temp_dir("arcgis-limit");
        let transport = FakeTransport::with(vec![
            (
                "resultOffset=0&",
                fixture_bytes("arcgis_query_page1.geojson"),
            ),
            (
                "resultOffset=4&",
                fixture_bytes("arcgis_query_page2.geojson"),
            ),
        ]);
        let environment = OnlineEnvironment {
            max_features: 5,
            ..environment_with(&transport, cache.path())
        };
        let mut lots = built_in_layer("nsw-spatial", "lots");
        lots.page_size = 4;
        let error = refused(
            fetch(&lots, &sydney_options("EPSG:7856"), &environment),
            "more features than the limit",
        );
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            error
                .message
                .contains("more than 5 features; choose a smaller area"),
            "{}",
            error.message
        );
    }

    #[test]
    fn cancelling_between_pages_stops_with_nothing_imported() {
        let cache = temp_dir("arcgis-cancel");
        let cancel = Arc::new(AtomicBool::new(false));
        let transport = FakeTransport::with(vec![
            (
                "resultOffset=0&",
                fixture_bytes("arcgis_query_page1.geojson"),
            ),
            (
                "resultOffset=4&",
                fixture_bytes("arcgis_query_page2.geojson"),
            ),
        ]);
        // The person presses Cancel while the first page downloads.
        let pressed = Arc::clone(&cancel);
        *lock(&transport.on_request) = Some(Box::new(move |_: &HttpRequest| {
            pressed.store(true, Ordering::Release);
        }));
        let mut lots = built_in_layer("nsw-spatial", "lots");
        lots.page_size = 4;
        let error = refused(
            fetch_online_layer(
                &lots,
                &sydney_options("EPSG:7856"),
                &environment_with(&transport, cache.path()),
                Some(cancel.as_ref()),
                None,
            ),
            "a cancelled import",
        );
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert_eq!(error.message, "cancelled");
        assert_eq!(transport.urls().len(), 1);
    }

    #[test]
    fn a_service_refusal_carries_the_services_own_words() {
        let cache = temp_dir("arcgis-refusal");
        let transport = FakeTransport::with(vec![("/query?", fixture_bytes("arcgis_error.json"))]);
        let environment = environment_with(&transport, cache.path());
        let error = refused(
            fetch(
                &built_in_layer("nsw-spatial", "lots"),
                &sydney_options("EPSG:7856"),
                &environment,
            ),
            "a service refusal",
        );
        assert_eq!(error.code, ErrorCode::FileImportFailure);
        assert!(
            error.context.contains("'resultOffset' is not supported"),
            "{}",
            error.context
        );
        // A refusal is not cached as though it were an answer.
        transport.set_answers(vec![(
            "/query?",
            fixture_bytes("arcgis_query_page3.geojson"),
        )]);
        let retried = must(fetch(
            &built_in_layer("nsw-spatial", "lots"),
            &sydney_options("EPSG:7856"),
            &environment,
        ));
        assert_eq!(retried.vectors.as_ref().expect("vectors").entities.len(), 3);
    }

    #[test]
    fn wfs_and_ogc_api_pages_are_followed() {
        let cache = temp_dir("wfs-oapif");
        // Two GeoJSON pages for the WFS (a server may answer GetFeature with
        // GeoJSON; GDAL reads it whatever the file is called). A WFS 2.0 page
        // shorter than COUNT is not the last: it is asked again until a page
        // comes back empty.
        let transport = FakeTransport::with(vec![
            ("STARTINDEX=0", fixture_bytes("arcgis_query_page1.geojson")),
            ("STARTINDEX=4", fixture_bytes("arcgis_query_page3.geojson")),
            (
                "STARTINDEX=7",
                br#"{"type":"FeatureCollection","features":[]}"#.to_vec(),
            ),
            ("items?bbox=", fixture_bytes("oapif_items_page1.json")),
            ("offset=2", fixture_bytes("arcgis_query_page3.geojson")),
        ]);
        let environment = environment_with(&transport, cache.path());
        let mut wfs = built_in_layer("vic-vicmap", "parcels");
        wfs.page_size = 4;
        wfs.coverage = [140.0, -40.0, 155.0, -28.0];
        let via_wfs = must(fetch(&wfs, &sydney_options("EPSG:7856"), &environment));
        assert_eq!(via_wfs.vectors.as_ref().expect("vectors").entities.len(), 7);
        // EPSG:4326 in WFS 2.0 is latitude first.
        let urls = transport.urls();
        assert!(
            urls[0].contains(
                "&BBOX=-33.875,151.2,-33.865,151.215,urn%3Aogc%3Adef%3Acrs%3AEPSG%3A%3A4326&"
            ),
            "{}",
            urls[0]
        );

        let oapif = OnlineLayer {
            provider_id: String::from("custom-example-invalid"),
            provider_title: String::from("Example"),
            id: String::from("buildings"),
            title: String::from("Buildings"),
            kind: OnlineLayerKind::Vector,
            r#type: OnlineServiceType::OgcApiFeatures,
            endpoint: String::from("https://example.invalid/ogc"),
            layer_name: String::from("buildings"),
            licence: String::from("L"),
            attribution: String::from("A"),
            ..OnlineLayer::default()
        };
        let via_api = must(fetch(&oapif, &sydney_options("EPSG:7856"), &environment));
        // 2 then 3, by the next link.
        assert_eq!(via_api.vectors.as_ref().expect("vectors").entities.len(), 5);
        assert_eq!(via_api.stats.pages, 2);
    }

    #[test]
    fn overpass_is_asked_by_post_and_its_answer_read_by_gdals_osm_driver() {
        let cache = temp_dir("overpass");
        let transport =
            FakeTransport::with(vec![("data=", fixture_bytes("overpass_buildings.osm"))]);
        let environment = environment_with(&transport, cache.path());
        let imported = must(fetch(
            &built_in_layer("osm", "buildings"),
            &sydney_options("EPSG:7856"),
            &environment,
        ));
        let bodies = transport.bodies();
        assert_eq!(bodies.len(), 1);
        assert!(bodies[0].starts_with("data="));
        // ["building"]
        assert!(bodies[0].contains("%5B%22building%22%5D"), "{}", bodies[0]);
        let entities = &imported.vectors.as_ref().expect("vectors").entities;
        // The cafe point and the office outline.
        assert!(entities.len() >= 2);
        let mut office = false;
        for entity in entities {
            if entity
                .properties
                .get("name")
                .is_some_and(|name| property_to_string(name) == "Test House")
            {
                office = true;
                assert_eq!(metadata_text(entity, "online.licence"), "ODbL 1.0");
            }
        }
        assert!(office);
        // A big area is refused before anything is asked.
        let big = OnlineRequestOptions {
            area: CrsBox {
                min_x: 150.0,
                min_y: -35.0,
                max_x: 152.0,
                max_y: -33.0,
            },
            ..sydney_options("EPSG:7856")
        };
        let error = refused(
            fetch(&built_in_layer("osm", "buildings"), &big, &environment),
            "a big area",
        );
        assert!(
            error.message.contains("limited to 25 km2"),
            "{}",
            error.message
        );
        assert_eq!(transport.urls().len(), 1);
    }

    #[test]
    fn a_local_geo_json_goes_through_the_same_fetch_path_as_a_file_url() {
        let cache = temp_dir("file-url");
        let mut layer = built_in_layer("natural-earth", "countries");
        layer.endpoint = file_url(&Path::new(ONLINE_TEST_DATA).join("arcgis_query_page1.geojson"));
        let environment = OnlineEnvironment {
            cache_directory: cache.path().to_path_buf(),
            ..OnlineEnvironment::default()
        };
        let imported = must(fetch(&layer, &sydney_options("EPSG:3857"), &environment));
        let entities = &imported.vectors.as_ref().expect("vectors").entities;
        assert_eq!(entities.len(), 4);
        assert_eq!(imported.stats.requests, 1);
        let Geometry::Polyline(polygon) = &entities[0].geometry else {
            panic!("the first lot is a polyline");
        };
        // Web Mercator from its formula (x = R lambda, y = R ln tan(pi/4 +
        // phi/2)). The file is clipped to the area before it is read, which
        // may start the ring at another corner: the corner is looked for, not
        // assumed first.
        let pi = std::f64::consts::PI;
        let x = 6378137.0 * 151.2050 * pi / 180.0;
        let y = 6378137.0 * (pi / 4.0 + -33.8700 * pi / 360.0).tan().ln();
        assert!(
            polygon
                .vertices
                .iter()
                .any(|vertex| (vertex.x - x).abs() < 1e-4 && (vertex.y - y).abs() < 1e-4),
            "{:?}",
            polygon.vertices
        );
        assert_eq!(polygon.vertices.len(), 4);
        // The area filter: an area away from the lots imports none of them.
        let elsewhere = OnlineRequestOptions {
            area: CrsBox {
                min_x: 150.0,
                min_y: -35.0,
                max_x: 150.01,
                max_y: -34.99,
            },
            ..sydney_options("EPSG:3857")
        };
        let none = must(fetch(&layer, &elsewhere, &environment));
        assert!(none.vectors.as_ref().expect("vectors").entities.is_empty());
    }

    #[test]
    fn a_key_is_sent_but_never_recorded() {
        let cache = temp_dir("key");
        let transport = FakeTransport::with(vec![(
            "data.geojson",
            fixture_bytes("arcgis_query_page3.geojson"),
        )]);
        let mut environment = environment_with(&transport, cache.path());
        let mut layer = built_in_layer("natural-earth", "countries");
        layer.endpoint = String::from("https://example.invalid/data.geojson?key={key}");
        layer.key_required = true;
        layer.key_name = String::from("example");
        let missing = refused(
            fetch(&layer, &sydney_options("EPSG:7856"), &environment),
            "a layer without its key",
        );
        assert_eq!(missing.code, ErrorCode::NotFound);
        assert!(
            missing.message.contains("ONLINE KEY example <key>"),
            "{}",
            missing.message
        );
        assert!(transport.urls().is_empty());

        environment
            .keys
            .insert(String::from("example"), String::from("S3CR3T value"));
        let imported = must(fetch(&layer, &sydney_options("EPSG:7856"), &environment));
        let urls = transport.urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("key=S3CR3T%20value"), "{}", urls[0]);
        assert_eq!(
            imported.source_url,
            "https://example.invalid/data.geojson?key=***"
        );
        for entity in &imported.vectors.as_ref().expect("vectors").entities {
            for (name, value) in &entity.metadata {
                assert!(!property_to_string(value).contains("S3CR3T"), "{name}");
            }
        }
        // Nor in the cache's file names.
        for path in every_path(cache.path()) {
            assert!(
                !path.to_string_lossy().contains("S3CR3T"),
                "{}",
                path.display()
            );
        }
    }

    #[test]
    fn an_area_outside_the_providers_coverage_is_refused_by_name() {
        let environment = OnlineEnvironment::default();
        let perth = OnlineRequestOptions {
            area: CrsBox {
                min_x: 115.8,
                min_y: -32.0,
                max_x: 115.9,
                max_y: -31.9,
            },
            ..sydney_options("EPSG:7850")
        };
        let error = refused(
            fetch(&built_in_layer("nsw-spatial", "lots"), &perth, &environment),
            "an area outside the coverage",
        );
        assert!(
            error
                .message
                .contains("outside what NSW Spatial Services covers"),
            "{}",
            error.message
        );
        let local = sydney_options("");
        let no_crs = refused(
            fetch(&built_in_layer("nsw-spatial", "lots"), &local, &environment),
            "a project without a CRS",
        );
        assert_eq!(no_crs.code, ErrorCode::InvalidCrs);
    }

    // ---- rasters ------------------------------------------------------------------------

    #[test]
    fn a_local_geo_tiff_is_warped_into_the_project_with_its_values_kept() {
        let work = temp_dir("cog");
        // A 5 m grid in MGA 56 over the Sydney CBD: 400 x 300 cells from
        // (333000, 6252000).
        let dem = write_grid(
            &work.file("dem.tif"),
            "EPSG:7856",
            333000.0,
            6252000.0,
            5.0,
            400,
            300,
        );
        let mut layer = built_in_layer("copernicus", "dem");
        layer.endpoint = file_url(&dem);
        layer.tiling.clear();
        let environment = OnlineEnvironment {
            cache_directory: work.file("cache"),
            ..OnlineEnvironment::default()
        };
        // An area on the grid's own cell edges, at its own resolution: every
        // warped cell centre is a source cell centre, so bilinear gives the
        // source's value exactly.
        let options = OnlineRequestOptions {
            area: CrsBox {
                min_x: 334000.0,
                min_y: 6251000.0,
                max_x: 334500.0,
                max_y: 6251400.0,
            },
            area_crs: String::from("EPSG:7856"),
            target_crs: String::from("EPSG:7856"),
            resolution: Some(5.0),
            ..OnlineRequestOptions::default()
        };
        let imported = must(fetch(&layer, &options, &environment));
        let raster = imported.raster.as_ref().expect("a raster");
        assert_eq!(raster.licence, layer.licence);
        assert_eq!(raster.attribution, layer.attribution);
        assert!(raster.has_geotransform);
        double_eq(raster.geotransform[0], 334000.0);
        double_eq(raster.geotransform[3], 6251400.0);

        let warped = must(GdalDataset::open(&imported.file));
        let info = must(warped.raster_info());
        assert_eq!(info.width, 100);
        assert_eq!(info.height, 80);
        let band = must(warped.read_band(1));
        // Output cell (0, 0) is source cell (200, 120): 1000 * 120 + 200.
        double_eq(band[0], 120200.0);
        // Output cell (99, 79) is source (299, 199).
        double_eq(band[79 * 100 + 99], 199299.0);

        // Asked again: the finished raster itself comes from the cache.
        let again = must(fetch(&layer, &options, &environment));
        assert!(again.stats.product_from_cache);
        assert_eq!(again.file, imported.file);
    }

    #[test]
    fn tiles_are_fetched_mosaicked_and_warped_and_cached_for_the_next_time() {
        let work = temp_dir("tiles");
        // Every tile is the same 256-pixel grey GeoTIFF; where it lies comes
        // from the tile index, not from the file.
        let tile = write_grid(
            &work.file("tile.tif"),
            "EPSG:3857",
            0.0,
            256.0,
            1.0,
            256,
            256,
        );
        let transport = FakeTransport::with(vec![(
            "https://tiles.example.invalid/",
            fs::read(tile).expect("the tile is read"),
        )]);
        let environment = environment_with(&transport, &work.file("cache"));
        let mut layer = built_in_layer("osm", "standard");
        layer.endpoint = String::from("https://tiles.example.invalid/{z}/{x}/{y}.tif");
        let mut options = OnlineRequestOptions {
            resolution: Some(2.0),
            ..sydney_options("EPSG:7856")
        };
        let imported = must(fetch(&layer, &options, &environment));
        assert!(imported.stats.requests > 0);
        assert!(imported.stats.requests <= layer.max_tiles);
        let raster = imported.raster.as_ref().expect("a raster");
        assert_eq!(raster.attribution, "\u{a9} OpenStreetMap contributors");
        // The raster lies over the area in MGA 56: its corners are the
        // area's.
        let area = must(transform_box(&options.area, "EPSG:4326", "EPSG:7856"));
        near(raster.geotransform[0], area.min_x, 1e-6);
        near(raster.geotransform[3], area.max_y, 2.0);
        // Every tile asked for is a real tile of the zoom the resolution
        // needs: 2 m on the ground at 33.87 S is 2.41 Web Mercator metres,
        // level 16 (2.39 m) the first at or below it.
        for url in transport.urls() {
            assert!(
                url.starts_with("https://tiles.example.invalid/16/"),
                "{url}"
            );
        }
        let asked = transport.urls().len();
        let again = must(fetch(&layer, &options, &environment));
        assert!(again.stats.product_from_cache);
        assert_eq!(transport.urls().len(), asked);
        // A different resolution is a different product, but the same tiles
        // come from the cache.
        options.resolution = Some(2.2);
        let third = must(fetch(&layer, &options, &environment));
        assert!(!third.stats.product_from_cache);
        assert_eq!(third.stats.requests, 0);
        assert!(third.stats.cache_hits > 0);
    }

    #[test]
    fn raster_limits_are_refused_before_any_request_with_the_resolution_that_fits() {
        let work = temp_dir("raster-limits");
        let transport = FakeTransport::with(Vec::new());
        let mut environment = OnlineEnvironment {
            max_pixels: 1_000_000,
            ..environment_with(&transport, work.path())
        };
        let mut options = OnlineRequestOptions {
            // 2773 x 2224 pixels.
            resolution: Some(0.5),
            ..sydney_options("EPSG:7856")
        };
        let error = refused(
            fetch(
                &built_in_layer("nsw-spatial", "imagery"),
                &options,
                &environment,
            ),
            "more pixels than allowed",
        );
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.message.contains("use res="), "{}", error.message);
        assert!(transport.urls().is_empty());
        // A tile service's own budget: the OpenStreetMap standard map allows
        // 64. (The pixel limit is lifted so that it is the tiles that
        // refuse.)
        environment.max_pixels = 1 << 34;
        options.area = CrsBox {
            min_x: 151.0,
            min_y: -34.0,
            max_x: 151.3,
            max_y: -33.7,
        };
        options.resolution = Some(1.0);
        let too_many_tiles = refused(
            fetch(&built_in_layer("osm", "standard"), &options, &environment),
            "more tiles than allowed",
        );
        assert!(
            too_many_tiles.message.contains("more than the 64 tiles"),
            "{}",
            too_many_tiles.message
        );
        assert!(transport.urls().is_empty());
    }

    #[test]
    fn an_image_service_that_answers_with_a_message_is_reported_not_warped() {
        let work = temp_dir("service-exception");
        let transport = FakeTransport::with(vec![(
            "GetMap",
            b"<?xml version=\"1.0\"?><ServiceExceptionReport><ServiceException code=\"LayerNotDefined\">Layer ga_ls_fc_3 is not defined</ServiceException></ServiceExceptionReport>".to_vec(),
        )]);
        let error = refused(
            fetch(
                &built_in_layer("dea", "fractional-cover"),
                &sydney_options("EPSG:7856"),
                &environment_with(&transport, work.path()),
            ),
            "a service exception",
        );
        assert_eq!(error.code, ErrorCode::FileImportFailure);
        assert!(
            error.context.contains("Layer ga_ls_fc_3 is not defined"),
            "{}",
            error.context
        );
        // The WMS was asked in Web Mercator, x,y.
        let urls = transport.urls();
        assert!(!urls.is_empty());
        assert!(
            urls[0].contains("CRS=EPSG%3A3857&BBOX=16831"),
            "{}",
            urls[0]
        );
    }

    #[test]
    fn a_stac_items_visual_asset_is_read_as_a_cog() {
        let work = temp_dir("stac");
        // A scene in the item's own UTM zone (56 S, EPSG:32756), as
        // Sentinel-2 COGs are, covering the Sydney area at 10 m.
        let visual = write_grid(
            &work.file("TCI.tif"),
            "EPSG:32756",
            332000.0,
            6253000.0,
            10.0,
            400,
            400,
        );
        let original = "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/56/H/LH/2026/1/S2B_56HLH_20260103_0_L2A/TCI.tif";
        let fixture_item = fixture("stac_item_sentinel2.json");
        assert!(fixture_item.contains(original));
        let item = fixture_item.replacen(original, &file_url(&visual), 1);
        // Read from a file, the item may name a file; served over the
        // network, the same item is refused (a service cannot make Katana
        // open a local file).
        fs::write(work.file("item.json"), &item).expect("the item is written");
        let mut layer = built_in_layer("sentinel2", "truecolour");
        layer.endpoint = file_url(&work.file("item.json"));
        let environment = OnlineEnvironment {
            cache_directory: work.file("cache"),
            ..OnlineEnvironment::default()
        };
        let imported = must(fetch(&layer, &sydney_options("EPSG:7856"), &environment));
        let transport = FakeTransport::with(vec![("item.json", item.into_bytes())]);
        let network = environment_with(&transport, &work.file("cache2"));
        let remote = OnlineLayer {
            endpoint: String::from("https://example.invalid/item.json"),
            ..layer.clone()
        };
        let error = refused(
            fetch(&remote, &sydney_options("EPSG:7856"), &network),
            "a network item naming a file",
        );
        assert!(
            error.message.contains("names a file on this computer"),
            "{}",
            error.message
        );
        assert!(warned(&imported.warnings, "scene date 2026-01-03"));
        let raster = imported.raster.as_ref().expect("a raster");
        assert_eq!(
            raster.attribution,
            "Contains modified Copernicus Sentinel data"
        );
        assert!(raster.width > 0);
    }

    #[test]
    fn the_cache_is_pruned_oldest_first_to_its_size() {
        let work = temp_dir("prune");
        for i in 0..4_u64 {
            let file = work
                .path()
                .join("http")
                .join("ab")
                .join(format!("f{i}.bin"));
            fs::create_dir_all(file.parent().expect("a folder")).expect("the folder is made");
            fs::write(&file, vec![b'x'; 1000]).expect("the file is written");
            let written = SystemTime::now() - Duration::from_secs(3600 * (4 - i));
            fs::File::options()
                .write(true)
                .open(file)
                .and_then(|opened| opened.set_modified(written))
                .expect("the file is dated");
        }
        assert_eq!(prune_cache(work.path(), 30, 2500), 2000);
        let file = |name: &str| work.path().join("http").join("ab").join(name);
        assert!(!file("f0.bin").exists());
        assert!(!file("f1.bin").exists());
        assert!(file("f3.bin").exists());
        // Everything is older than 0 days.
        assert_eq!(prune_cache(work.path(), 0, 1 << 30), 2000);
    }
}

// ---- live: only with KATANA_ONLINE_TESTS=1 -------------------------------------------------------

mod online_live {
    use super::*;

    #[test]
    fn copernicus_dem_at_sydney_is_about_sea_level_to_fifty_metres() {
        if !live_tests_enabled() {
            eprintln!("{LIVE_SKIP}");
            return;
        }
        let work = temp_dir("live-copernicus");
        let environment = OnlineEnvironment {
            cache_directory: work.path().to_path_buf(),
            ..OnlineEnvironment::default()
        };
        let imported = must(fetch(
            &built_in_layer("copernicus", "dem"),
            &sydney_options("EPSG:7856"),
            &environment,
        ));
        let dataset = must(GdalDataset::open(&imported.file));
        let band = must(dataset.read_band(1));
        let mut low = 1e9;
        let mut high = -1e9;
        for &value in &band {
            if value > -1000.0 {
                low = katana_math::cpp::min(low, value);
                high = katana_math::cpp::max(high, value);
            }
        }
        // GLO-30 is a SURFACE model: the ground runs from the harbour (0 m)
        // to about 40 m on the ridges, and the towers of the CBD stand on it -
        // the tallest, Sydney Tower, 309 m above the street. So: nothing below
        // the sea, something well above the ground, nothing above the tower.
        assert!(low > -5.0, "{low}");
        assert!(high > 40.0, "{high}");
        assert!(high < 350.0, "{high}");
    }
}
