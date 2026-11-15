//! tests/interop/test_online_catalogue.cpp: the online provider catalogue
//! (docs/gis_online.md, "The catalogue").
//!
//! The built-in catalogue is data compiled into the program, so these tests
//! are what stops a bad edit to resources/online/online_sources.json from
//! shipping: every layer must parse into a complete description - a licence
//! and an attribution to record, a well-formed https endpoint whose template
//! has the placeholders its type needs, a valid longitude/latitude coverage -
//! and the ids the ONLINE verbs name must be unique and typeable. No network.

mod online_catalogue {
    use std::collections::BTreeSet;

    use katana_core::ErrorCode;
    use katana_interop::online_catalogue::{
        LonLatBox, OnlineCatalogue, OnlineLayerKind, OnlineServiceType, built_in_catalogue,
        layer_kind_from_string, merge_catalogues, parse_catalogue, provider_to_json,
        service_type_from_string, to_string, validate_catalogue,
    };

    fn built_in() -> OnlineCatalogue {
        match built_in_catalogue() {
            Ok(catalogue) => catalogue,
            Err(error) => panic!("{}", error.describe()),
        }
    }

    #[test]
    fn the_built_in_catalogue_parses_and_passes_its_own_validation() {
        let catalogue = built_in();
        assert!(!catalogue.providers.is_empty());
        let problems = validate_catalogue(&catalogue);
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn every_layer_carries_what_a_request_and_a_record_need() {
        for provider in &built_in().providers {
            for layer in provider.layers() {
                let at = format!("{}/{}", provider.id, layer.id);
                assert!(!layer.title.is_empty(), "{at}");
                assert!(!layer.licence.is_empty(), "{at}");
                assert!(!layer.attribution.is_empty(), "{at}");
                assert!(
                    layer.endpoint.starts_with("https://"),
                    "{at} {}",
                    layer.endpoint
                );
                assert!(!layer.group.is_empty(), "{at}");
                assert_eq!(layer.provider_id, provider.id, "{at}");
                // Something says where the address and terms were checked:
                // the date it answered, or the documentation it was taken
                // from.
                assert!(
                    !layer.verified.is_empty() || !layer.evidence.is_empty(),
                    "{at}"
                );
                assert!(layer.coverage[0] < layer.coverage[2], "{at}");
                assert!(layer.coverage[1] < layer.coverage[3], "{at}");
            }
        }
    }

    #[test]
    fn the_groups_are_australia_by_state_and_global() {
        let mut groups = BTreeSet::new();
        for provider in &built_in().providers {
            groups.insert(provider.group.clone());
            assert!(
                provider.group == "Global" || provider.group.starts_with("Australia/"),
                "{} is in {}",
                provider.id,
                provider.group
            );
        }
        for wanted in [
            "Australia/NSW",
            "Australia/QLD",
            "Australia/National",
            "Global",
        ] {
            assert!(groups.contains(wanted), "{wanted}");
        }
    }

    #[test]
    fn it_offers_the_layers_the_owner_asked_for() {
        let catalogue = built_in();
        let has =
            |provider: &str, layer: &str, kind: OnlineLayerKind, r#type: OnlineServiceType| {
                let found = catalogue
                    .find_provider(provider)
                    .unwrap_or_else(|| panic!("{provider}"));
                let entry = found
                    .find_layer(layer)
                    .unwrap_or_else(|| panic!("{provider}/{layer}"));
                assert_eq!(entry.kind, kind, "{provider}/{layer}");
                assert_eq!(entry.r#type, r#type, "{provider}/{layer}");
            };
        has(
            "nsw-spatial",
            "imagery",
            OnlineLayerKind::Imagery,
            OnlineServiceType::XyzTiles,
        );
        has(
            "nsw-spatial",
            "lots",
            OnlineLayerKind::Vector,
            OnlineServiceType::ArcgisQuery,
        );
        has(
            "nsw-spatial",
            "roads",
            OnlineLayerKind::Vector,
            OnlineServiceType::ArcgisQuery,
        );
        has(
            "nsw-spatial",
            "dem5m",
            OnlineLayerKind::Elevation,
            OnlineServiceType::ArcgisExportImage,
        );
        has(
            "osm",
            "buildings",
            OnlineLayerKind::Vector,
            OnlineServiceType::Overpass,
        );
        has(
            "osm",
            "standard",
            OnlineLayerKind::Imagery,
            OnlineServiceType::XyzTiles,
        );
        has(
            "copernicus",
            "dem",
            OnlineLayerKind::Elevation,
            OnlineServiceType::Cog,
        );
        has(
            "sentinel2",
            "truecolour",
            OnlineLayerKind::Imagery,
            OnlineServiceType::Stac,
        );
        has(
            "nasa-gibs",
            "modis-terra",
            OnlineLayerKind::Imagery,
            OnlineServiceType::Wmts,
        );
        has(
            "natural-earth",
            "countries",
            OnlineLayerKind::Vector,
            OnlineServiceType::File,
        );
        has(
            "ga",
            "dem-h",
            OnlineLayerKind::Elevation,
            OnlineServiceType::Wcs,
        );
        has(
            "dea",
            "geomad",
            OnlineLayerKind::Imagery,
            OnlineServiceType::Wms,
        );
    }

    #[test]
    fn the_road_layer_is_called_roads() {
        let catalogue = built_in();
        let roads = catalogue
            .find_provider("nsw-spatial")
            .expect("nsw-spatial is in the catalogue")
            .find_layer("roads")
            .expect("nsw-spatial has roads");
        assert_eq!(roads.title, "Roads");
    }

    #[test]
    fn the_open_street_map_tile_policy_is_in_the_data() {
        // operations.osmfoundation.org/policies/tiles: small areas only, cache
        // for at least seven days, attribution "© OpenStreetMap
        // contributors".
        let catalogue = built_in();
        let tiles = catalogue
            .find_provider("osm")
            .expect("osm is in the catalogue")
            .find_layer("standard")
            .expect("osm has standard");
        assert!(tiles.max_tiles <= 64);
        assert!(tiles.cache_days >= 7);
        assert_eq!(tiles.attribution, "© OpenStreetMap contributors");
        assert_eq!(tiles.licence, "ODbL 1.0");
    }

    #[test]
    fn inheritance_fills_a_layer_from_its_service_and_provider() {
        let json = r#"{"providers": [{
        "id": "p", "title": "Provider", "group": "Global", "licence": "CC BY 4.0",
        "attribution": "© Provider", "coverage": [100, -40, 160, -10],
        "services": [{"id": "s", "title": "S", "type": "arcgis-query",
                      "endpoint": "https://example.org/arcgis/rest/services/X/MapServer",
                      "pageSize": 500, "crs": "EPSG:3857",
                      "layers": [{"id": "a", "title": "A", "kind": "vector", "layer": "3"},
                                 {"id": "b", "title": "B", "kind": "vector", "layer": "4",
                                  "pageSize": 50, "licence": "CC BY 3.0 AU"}]}]}]}"#;
        let catalogue = match parse_catalogue(json, false) {
            Ok(catalogue) => catalogue,
            Err(error) => panic!("{}", error.describe()),
        };
        let provider = &catalogue.providers[0];
        let a = provider.find_layer("a").expect("a is there");
        let b = provider
            .find_layer("B")
            .expect("ids match without regard to case");
        assert_eq!(a.page_size, 500);
        assert_eq!(a.licence, "CC BY 4.0");
        assert_eq!(a.attribution, "© Provider");
        assert_eq!(a.crs, "EPSG:3857");
        let expected: LonLatBox = [100.0, -40.0, 160.0, -10.0];
        assert_eq!(a.coverage, expected);
        assert_eq!(
            a.endpoint,
            "https://example.org/arcgis/rest/services/X/MapServer"
        );
        assert_eq!(b.page_size, 50);
        assert_eq!(b.licence, "CC BY 3.0 AU");
        assert_eq!(b.layer_name, "4");
        assert!(validate_catalogue(&catalogue).is_empty());
    }

    #[test]
    fn validation_names_each_problem() {
        let json = r#"{"providers": [{
        "id": "bad", "title": "Bad", "group": "Global",
        "services": [
          {"id": "t", "title": "T", "type": "xyz", "endpoint": "ftp://tiles/{z}/{x}.png",
           "layers": [{"id": "one", "title": "One", "kind": "imagery"}]},
          {"id": "q", "title": "Q", "type": "arcgis-query", "endpoint": "https://example.org/x",
           "coverage": [10, 10, 5, 5], "licence": "L", "attribution": "A",
           "layers": [{"id": "one", "title": "Again", "kind": "vector"},
                      {"id": "has space", "title": "S", "kind": "imagery", "layer": "1"}]}]}]}"#;
        let catalogue = match parse_catalogue(json, false) {
            Ok(catalogue) => catalogue,
            Err(error) => panic!("{}", error.describe()),
        };
        let problems = validate_catalogue(&catalogue);
        let mentions = |text: &str| problems.iter().any(|problem| problem.contains(text));
        assert!(mentions("bad/one: no licence"), "{problems:#?}");
        assert!(mentions("bad/one: no attribution"), "{problems:#?}");
        assert!(mentions("not an http or https URL"), "{problems:#?}");
        assert!(mentions("has no {y}"), "{problems:#?}");
        assert!(mentions("crs must be EPSG:3857"), "{problems:#?}");
        assert!(mentions("coverage is not a valid"), "{problems:#?}");
        assert!(mentions("used twice"), "{problems:#?}");
        assert!(mentions("needs the layer id"), "{problems:#?}");
        assert!(mentions("may not hold spaces"), "{problems:#?}");
        assert!(
            mentions("cannot come from a arcgis-query service"),
            "{problems:#?}"
        );
    }

    #[test]
    fn a_document_that_is_not_a_catalogue_is_refused_with_where() {
        assert_eq!(
            parse_catalogue("{", false)
                .expect_err("not JSON is refused")
                .code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            parse_catalogue("{\"version\": 1}", false)
                .expect_err("no providers is refused")
                .code,
            ErrorCode::ParseFailure
        );
        let unknown = parse_catalogue(
            r#"{"providers": [{"id": "p", "services": [
        {"id": "s", "type": "gopher", "endpoint": "https://x", "layers": [{"id": "l", "kind": "vector"}]}]}]}"#,
            false,
        )
        .expect_err("an unknown service type is refused");
        assert!(unknown.message.contains("gopher"), "{}", unknown.message);
        assert_eq!(unknown.context, "p/s");
    }

    #[test]
    fn a_user_catalogue_replaces_a_provider_by_id_and_adds_new_ones() {
        let base = built_in();
        let user = match parse_catalogue(
            r#"{"providers": [
        {"id": "OSM", "title": "My OSM mirror", "group": "Global", "licence": "ODbL 1.0",
         "attribution": "© OpenStreetMap contributors",
         "services": [{"id": "t", "title": "T", "type": "xyz", "crs": "EPSG:3857",
                       "endpoint": "https://tiles.example.org/{z}/{x}/{y}.png",
                       "layers": [{"id": "standard", "title": "Mirror", "kind": "imagery"}]}]},
        {"id": "mine", "title": "Mine", "group": "Custom", "licence": "L", "attribution": "A",
         "services": [{"id": "c", "title": "C", "type": "cog",
                       "endpoint": "https://example.org/dem.tif",
                       "layers": [{"id": "dem", "title": "DEM", "kind": "elevation"}]}]}]}"#,
            true,
        ) {
            Ok(user) => user,
            Err(error) => panic!("{}", error.describe()),
        };
        let merged = merge_catalogues(base.clone(), &user);
        assert_eq!(merged.providers.len(), base.providers.len() + 1);
        let osm = merged.find_provider("osm").expect("osm is still there");
        assert_eq!(osm.title, "My OSM mirror");
        assert!(osm.user_defined);
        // Replaced in place: the tree keeps its order.
        let position = |catalogue: &OnlineCatalogue, id: &str| {
            catalogue
                .providers
                .iter()
                .position(|provider| provider.id == id || provider.id == "OSM")
                .unwrap_or(catalogue.providers.len())
        };
        assert_eq!(position(&merged, "osm"), position(&base, "osm"));
        assert!(merged.find_provider("mine").is_some());
    }

    #[test]
    fn a_provider_written_as_json_reads_back_to_the_same_layers() {
        let base = built_in();
        for id in ["nsw-spatial", "sentinel2", "nasa-gibs"] {
            let provider = base
                .find_provider(id)
                .unwrap_or_else(|| panic!("{id} is in the catalogue"));
            let again = match parse_catalogue(provider_to_json(provider), false) {
                Ok(again) => again,
                Err(error) => panic!("{}", error.describe()),
            };
            assert_eq!(again.providers.len(), 1);
            let before = provider.layers();
            let after = again.providers[0].layers();
            assert_eq!(before.len(), after.len(), "{id}");
            for (was, now) in before.iter().zip(&after) {
                assert_eq!(was.id, now.id);
                assert_eq!(was.endpoint, now.endpoint);
                assert_eq!(was.r#type, now.r#type);
                assert_eq!(was.kind, now.kind);
                assert_eq!(was.layer_name, now.layer_name);
                assert_eq!(was.licence, now.licence);
                assert_eq!(was.attribution, now.attribution);
                assert_eq!(was.coverage, now.coverage);
                assert_eq!(was.crs, now.crs);
                assert_eq!(was.asset, now.asset);
            }
        }
    }

    #[test]
    fn filter_matches_every_word_against_providers_and_their_layers() {
        let catalogue = built_in();
        assert_eq!(catalogue.filter("").len(), catalogue.providers.len());
        let nsw = catalogue.filter("nsw lots");
        assert_eq!(nsw.len(), 1);
        assert_eq!(nsw[0].id, "nsw-spatial");
        let elevation = catalogue.filter("ELEVATION");
        assert!(elevation.len() >= 3); // NSW, GA, Copernicus at least
        assert!(catalogue.filter("no-such-thing-anywhere").is_empty());
    }

    #[test]
    fn enum_names_round_trip() {
        for r#type in [
            OnlineServiceType::ArcgisExport,
            OnlineServiceType::ArcgisExportImage,
            OnlineServiceType::ArcgisQuery,
            OnlineServiceType::Wms,
            OnlineServiceType::Wmts,
            OnlineServiceType::Wcs,
            OnlineServiceType::Wfs,
            OnlineServiceType::OgcApiFeatures,
            OnlineServiceType::XyzTiles,
            OnlineServiceType::Stac,
            OnlineServiceType::Cog,
            OnlineServiceType::Overpass,
            OnlineServiceType::File,
            OnlineServiceType::Ckan,
        ] {
            assert_eq!(
                service_type_from_string(to_string(r#type)),
                Some(r#type),
                "{}",
                to_string(r#type)
            );
        }
        for kind in [
            OnlineLayerKind::Imagery,
            OnlineLayerKind::Elevation,
            OnlineLayerKind::Vector,
            OnlineLayerKind::Catalogue,
        ] {
            assert_eq!(layer_kind_from_string(to_string(kind)), Some(kind));
        }
    }
}
