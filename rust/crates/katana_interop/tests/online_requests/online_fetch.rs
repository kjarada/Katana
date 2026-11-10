//! tests/interop/test_online_fetch.cpp, its discovery cases and two of its
//! live ones: discovery from committed capabilities fixtures, with NO network,
//! and - only with KATANA_ONLINE_TESTS=1 - every built-in endpoint and one
//! exchange on the wire.
//!
//! The file's OnlineFetch cases and its Copernicus live case are wave M's
//! (online_fetch); its live case that sets GDAL_HTTP_USERPWD is in the target
//! tests/online_environment.

use std::cell::RefCell;
use std::collections::BTreeMap;

use katana_core::{Error, ErrorCode, Result};
use katana_gis::reproject::CrsBox;
use katana_gis::web_access::{HttpRequest, http_exchange, http_fetch, redact_url};
use katana_interop::online_catalogue::{
    OnlineCatalogue, OnlineLayerKind, OnlineServiceType, built_in_catalogue, validate_catalogue,
};
use katana_interop::online_discovery::{
    custom_provider_id, description_url, discover_service, guess_service_type,
    parse_arcgis_description, parse_oapif_collections, parse_stac_description,
    parse_wcs_capabilities, parse_wfs_capabilities, parse_wms_capabilities,
    parse_wmts_capabilities,
};
use katana_interop::online_requests::{TileIndex, degree_tile_urls, expand_tile_template};

use crate::online_support::{
    DEFAULT_USER_AGENT, LIVE_SKIP, fixture, live_tests_enabled, must, near, refused,
};

// ---- discovery from fixtures --------------------------------------------------------------

mod online_discovery {
    use super::*;

    #[test]
    fn the_url_says_what_kind_of_service_it_is() {
        assert_eq!(
            guess_service_type("https://a/arcgis/rest/services/X/MapServer"),
            Some(OnlineServiceType::ArcgisExport)
        );
        assert_eq!(
            guess_service_type("https://a/arcgis/rest/services/X/MapServer/9"),
            Some(OnlineServiceType::ArcgisExport)
        );
        assert_eq!(
            guess_service_type("https://a/arcgis/rest/services/X/ImageServer"),
            Some(OnlineServiceType::ArcgisExportImage)
        );
        assert_eq!(
            guess_service_type("https://a/arcgis/rest/services/X/FeatureServer/0"),
            Some(OnlineServiceType::ArcgisQuery)
        );
        assert_eq!(
            guess_service_type(
                "https://a/arcgis/services/X/MapServer/WMSServer?request=GetCapabilities"
            ),
            Some(OnlineServiceType::Wms)
        );
        assert_eq!(
            guess_service_type("https://a/ows?service=WFS"),
            Some(OnlineServiceType::Wfs)
        );
        assert_eq!(
            guess_service_type("https://a/wmts/1.0.0/WMTSCapabilities.xml"),
            Some(OnlineServiceType::Wmts)
        );
        assert_eq!(
            guess_service_type("https://a/dem.tif"),
            Some(OnlineServiceType::Cog)
        );
        assert_eq!(
            guess_service_type("https://a/stac/v1"),
            Some(OnlineServiceType::Stac)
        );
        assert_eq!(
            guess_service_type("https://a/item.json"),
            Some(OnlineServiceType::Stac)
        );
        assert!(guess_service_type("https://a/geoserver/ows").is_none());
        // A vendor's own parameter (MapServer's map=) is kept; the request's
        // are replaced.
        assert_eq!(
            description_url(
                OnlineServiceType::Wms,
                "https://a/mapserv?map=/d/x.map&service=WMS&request=GetMap"
            ),
            "https://a/mapserv?map=/d/x.map&SERVICE=WMS&REQUEST=GetCapabilities"
        );
        assert_eq!(
            description_url(OnlineServiceType::ArcgisQuery, "https://a/FeatureServer/"),
            "https://a/FeatureServer/?f=json"
        );
        // The host, and which service on it: two services of one server are two
        // providers (one host alone let the second replace the first), and the
        // same service asked with another query is the same one.
        let a = custom_provider_id("https://maps.example.org/arcgis/rest/services/A/MapServer");
        let b = custom_provider_id("https://maps.example.org/arcgis/rest/services/B/MapServer");
        assert!(a.starts_with("custom-maps-example-org-"), "{a}");
        assert_ne!(a, b);
        assert_eq!(
            a,
            custom_provider_id("https://maps.example.org/arcgis/rest/services/A/MapServer?f=json")
        );
        // A key in a discovered address is taken out into the settings, and the
        // address keeps {key}. That is online_fetch's extractKey, which wave M
        // ports; the C++ case's three assertions of it are made live there:
        //
        //   let (endpoint, secret) =
        //       extract_key("https://tiles.example.org/{z}/{x}/{y}.png?api=SECRET&style=topo");
        //   assert_eq!(endpoint, "https://tiles.example.org/{z}/{x}/{y}.png?api={key}&style=topo");
        //   assert_eq!(secret, "SECRET");
        //   assert!(extract_key("https://a.org/x?layer=1").1.is_empty());
        assert_eq!(
            redact_url("https://a.org/x?api=S&subscription-key=T&auth=U"),
            "https://a.org/x?api=***&subscription-key=***&auth=***"
        );
    }

    #[test]
    fn wms130_capabilities_give_named_layers_with_inherited_extent() {
        let provider = must(parse_wms_capabilities(
            fixture("wms_capabilities_130.xml"),
            "https://ows.dea.ga.gov.au/?service=WMS",
        ));
        assert_eq!(provider.title, "Digital Earth Australia - OGC Web Services");
        assert_eq!(provider.licence, "CC BY 4.0");
        let layers = provider.layers();
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].layer_name, "ga_ls8cls9c_gm_cyear_3");
        assert_eq!(layers[0].title, "DEA GeoMAD (Landsat 8 & 9)");
        assert_eq!(layers[0].endpoint, "https://ows.dea.ga.gov.au/wms");
        assert_eq!(layers[0].version, "1.3.0");
        // Preferred over 4326.
        assert_eq!(layers[0].crs, "EPSG:3857");
        // From its parent.
        assert_eq!(layers[0].coverage, [112.0, -44.0, 154.0, -9.0]);
        assert_eq!(layers[1].coverage, [113.0, -43.5, 153.5, -10.0]);
        assert!(layers[0].user_defined);
        let catalogue = OnlineCatalogue {
            providers: vec![provider.clone()],
        };
        let problems = validate_catalogue(&catalogue);
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn wms111_lists_srs_and_lat_lon_box() {
        let provider = must(parse_wms_capabilities(
            fixture("wms_capabilities_111.xml"),
            "https://old.example.org/wms",
        ));
        let layer = provider.layers()[0];
        assert_eq!(layer.version, "1.1.1");
        assert_eq!(layer.crs, "EPSG:4326");
        assert_eq!(layer.coverage, [140.0, -38.0, 154.0, -28.0]);
        // "none" is not a licence.
        assert!(
            provider.licence.contains("check the publisher"),
            "{}",
            provider.licence
        );
    }

    #[test]
    fn wmts_layers_on_the_web_mercator_grid_become_tile_templates() {
        let provider = must(parse_wmts_capabilities(
            fixture("wmts_capabilities.xml"),
            "https://gibs.earthdata.nasa.gov/wmts/epsg3857/best/1.0.0/WMTSCapabilities.xml",
        ));
        let layers = provider.layers();
        // The national-grid-only layer is left out.
        assert_eq!(layers.len(), 1);
        let layer = layers[0];
        assert_eq!(
            layer.layer_name,
            "MODIS_Terra_CorrectedReflectance_TrueColor"
        );
        assert_eq!(layer.max_zoom, 9);
        assert_eq!(layer.crs, "EPSG:3857");
        assert_eq!(
            layer.endpoint,
            concat!(
                "https://gibs.earthdata.nasa.gov/wmts/epsg3857/best/MODIS_Terra_CorrectedReflectance_TrueColor/",
                "default/{time}/GoogleMapsCompatible_Level9/EPSG:3857:{z}/{y}/{x}.jpg"
            )
        );
        let catalogue = OnlineCatalogue {
            providers: vec![provider.clone()],
        };
        let problems = validate_catalogue(&catalogue);
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn wfs_feature_types_keep_their_default_crs() {
        let provider = must(parse_wfs_capabilities(
            fixture("wfs_capabilities_200.xml"),
            "https://opendata.maps.vic.gov.au/geoserver/ows",
        ));
        let layers = provider.layers();
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].layer_name, "open-data-platform:vmprop_parcel_mp");
        assert_eq!(layers[0].crs, "EPSG:7844");
        assert_eq!(layers[0].kind, OnlineLayerKind::Vector);
        assert_eq!(layers[1].crs, "EPSG:3111");
        assert_eq!(
            provider.licence,
            "Creative Commons Attribution 4.0 International"
        );
    }

    #[test]
    fn wcs_coverages_are_elevation() {
        let provider = must(parse_wcs_capabilities(
            fixture("wcs_capabilities_100.xml"),
            "https://services.ga.gov.au/x/WCSServer",
        ));
        let layer = provider.layers()[0];
        assert_eq!(layer.layer_name, "1");
        assert_eq!(layer.kind, OnlineLayerKind::Elevation);
        assert_eq!(layer.coverage, [112.99, -44.0, 153.99, -10.0]);
    }

    #[test]
    fn arcgis_descriptions_give_the_map_and_each_feature_layer() {
        let url = "https://maps.six.nsw.gov.au/arcgis/rest/services/public/NSW_Cadastre/MapServer";
        let map = must(parse_arcgis_description(
            fixture("arcgis_mapserver.json"),
            url,
        ));
        assert_eq!(map.attribution, "© Spatial Services");
        let names: Vec<String> = map
            .layers()
            .iter()
            .map(|layer| {
                if layer.layer_name.is_empty() {
                    String::from("(map)")
                } else {
                    layer.layer_name.clone()
                }
            })
            .collect();
        // The map image, then the feature layers - not the group, not the
        // annotation.
        assert_eq!(names, ["(map)", "8", "9"]);
        let lots = map.find_layer("lot").expect("the lot layer");
        assert_eq!(lots.r#type, OnlineServiceType::ArcgisQuery);
        assert_eq!(lots.page_size, 1000);
        // The Web Mercator extent back in degrees: 15695000 m east is 140.99 E.
        near(
            lots.coverage[0],
            15695000.0 / 20037508.342789244 * 180.0,
            1e-6,
        );

        let one = must(parse_arcgis_description(
            fixture("arcgis_layer.json"),
            &format!("{url}/9"),
        ));
        let layers = one.layers();
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].endpoint, url);
        assert_eq!(layers[0].layer_name, "9");
        assert_eq!(layers[0].page_size, 2000);
        assert_eq!(layers[0].id_field, "OBJECTID_1");

        let image = must(parse_arcgis_description(
            fixture("arcgis_imageserver.json"),
            "https://maps.six.nsw.gov.au/arcgis/rest/services/public/NSW_5M_Elevation/ImageServer",
        ));
        let dem = image.layers()[0];
        // One F32 band.
        assert_eq!(dem.kind, OnlineLayerKind::Elevation);
        assert_eq!(dem.r#type, OnlineServiceType::ArcgisExportImage);
        let resolution = dem.resolution.expect("the image's resolution");
        near(resolution, 4.5e-05 * 111320.0, 1e-9);
    }

    #[test]
    fn ogc_api_and_stac_descriptions_are_read() {
        let features = must(parse_oapif_collections(
            fixture("oapif_collections.json"),
            "https://api.example.org/ogc/collections",
        ));
        let layers = features.layers();
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].endpoint, "https://api.example.org/ogc");
        assert_eq!(layers[0].coverage, [150.5, -34.2, 151.5, -33.4]);

        let item = must(parse_stac_description(
            fixture("stac_item_sentinel2.json"),
            "https://sentinel-cogs.s3.us-west-2.amazonaws.com/x/S2B_56HLH_20260103_0_L2A.json",
        ));
        let layer = item.layers()[0];
        assert_eq!(layer.asset, "visual");
        assert_eq!(
            layer.attribution,
            "Contains modified Copernicus Sentinel data"
        );
        assert_eq!(layer.r#type, OnlineServiceType::Stac);
    }

    #[test]
    fn discovery_tries_the_candidates_when_the_url_does_not_say() {
        // A bare address: WMS is asked first, and a WFS answer to the WMS
        // question is not a WMS - so WMTS and then WFS are asked.
        let asked = RefCell::new(Vec::<String>::new());
        let fetch = |url: &str| -> Result<String> {
            asked.borrow_mut().push(String::from(url));
            if url.contains("SERVICE=WFS") {
                return Ok(fixture("wfs_capabilities_200.xml"));
            }
            Err(Error::new(ErrorCode::NotFound, "no"))
        };
        let provider = must(discover_service(
            "https://opendata.example.org/geoserver/ows",
            &fetch,
            None,
        ));
        assert_eq!(provider.services[0].r#type, OnlineServiceType::Wfs);
        assert_eq!(asked.borrow().len(), 3);
        let nothing = |_: &str| -> Result<String> { Err(Error::new(ErrorCode::NotFound, "no")) };
        let none = refused(
            discover_service("https://nothing.example.org/", &nothing, None),
            "an address that answers nothing",
        );
        assert_eq!(none.code, ErrorCode::Unsupported);
        assert!(
            none.message.contains("tried wms, wmts, wfs"),
            "{}",
            none.message
        );
        assert_eq!(
            refused(discover_service("ftp://x", &fetch, None), "an ftp address").code,
            ErrorCode::InvalidArgument
        );
    }
}

// ---- live: only with KATANA_ONLINE_TESTS=1 -----------------------------------------------------

/// `text.substr(0, text.find_last_of('/'))`: all of it when it has no slash.
fn before_last_slash(text: &str) -> &str {
    text.rfind('/').map_or(text, |slash| &text[..slash])
}

mod online_live {
    use super::*;

    #[test]
    fn every_built_in_endpoint_answers() {
        if !live_tests_enabled() {
            eprintln!("{LIVE_SKIP}");
            return;
        }
        // Each service's description, or for templates one tile of the area, so
        // a dead address shows here by name rather than in a person's import.
        let catalogue = must(built_in_catalogue());
        let mut dead = Vec::new();
        for provider in &catalogue.providers {
            for service in &provider.services {
                let layer = &service.layers[0];
                let url = match service.r#type {
                    OnlineServiceType::XyzTiles | OnlineServiceType::Wmts => {
                        let extra = BTreeMap::from([
                            (String::from("layer"), layer.layer_name.clone()),
                            (String::from("time"), String::from("default")),
                        ]);
                        let tile = TileIndex {
                            matrix: 2,
                            column: 3,
                            row: 2,
                        };
                        expand_tile_template(&layer.endpoint, &tile, "", &extra)
                    }
                    OnlineServiceType::Cog => {
                        let area = CrsBox {
                            min_x: 151.2,
                            min_y: -33.9,
                            max_x: 151.21,
                            max_y: -33.89,
                        };
                        degree_tile_urls(&layer.endpoint, &area)[0].clone()
                    }
                    OnlineServiceType::File => {
                        let at = layer
                            .endpoint
                            .find("{layer}")
                            .expect("a file layer's endpoint names {layer}");
                        let mut url = layer.endpoint.clone();
                        url.replace_range(at..at + 7, &layer.layer_name);
                        url
                    }
                    // .../api/interpreter takes only queries; .../api/status is
                    // the server's own report of itself.
                    OnlineServiceType::Overpass => {
                        format!("{}/status", before_last_slash(&layer.endpoint))
                    }
                    OnlineServiceType::Stac | OnlineServiceType::Ckan => {
                        String::from(before_last_slash(&layer.endpoint))
                    }
                    _ => description_url(service.r#type, &layer.endpoint),
                };
                let request = HttpRequest {
                    url,
                    user_agent: String::from(DEFAULT_USER_AGENT),
                    max_bytes: 64 << 20,
                    retries: 1,
                    ..HttpRequest::default()
                };
                match http_fetch(&request, None, None) {
                    Ok(_) => {}
                    // A service that answers only from its own country is alive
                    // - the network in front of it answered - and refuses only
                    // where this runs; it is named, not failed.
                    Err(error) if error.message.contains("own country") => println!(
                        "{}/{}: blocked from this network, not tested here",
                        provider.id, service.id
                    ),
                    // EXPECT_TRUE: every dead endpoint is named, not just the first.
                    Err(error) => dead.push(format!(
                        "{}/{}: {}",
                        provider.id,
                        service.id,
                        error.describe()
                    )),
                }
            }
        }
        assert!(dead.is_empty(), "{dead:#?}");
    }

    #[test]
    fn an_exchange_hands_back_a_redirect_and_does_not_follow_it() {
        if !live_tests_enabled() {
            eprintln!("{LIVE_SKIP}");
            return;
        }
        // http_exchange is the one request the update engine follows redirects
        // with by hand, so that its access token goes to one host and no further
        // (web_access): a redirect must come back as an answer. github.com
        // answers plain http with a 301 to its https address; were the redirect
        // followed, this would be a 200 and a page of several hundred kilobytes.
        let request = HttpRequest {
            url: String::from("http://github.com/"),
            user_agent: String::from(DEFAULT_USER_AGENT),
            max_bytes: 1 << 20,
            retries: 1,
            ..HttpRequest::default()
        };
        let answer = must(http_exchange(&request, None, None));
        assert_eq!(answer.status, 301);
        assert_eq!(answer.location, "https://github.com/");
        assert!(
            answer.body.len() <= 64 * 1024,
            "the body of a redirect is kept to 64 KiB"
        );
    }
}
