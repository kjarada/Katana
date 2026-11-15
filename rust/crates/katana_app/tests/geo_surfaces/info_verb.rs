//! tests/geo/test_info_verb.cpp: INFO as data (info_verb.rs; docs/interop.md,
//! "Dataset information"): what a file, a folder or a /vsi path holds, as
//! records read from GDAL's own info JSON, the JSON itself, statistics
//! computed without a sidecar, and GDAL's check - through the executor, as
//! every front end runs it.
//!
//! The facts, by hand:
//!
//! ```text
//! samples/gis/terrain.asc  ncols 120, nrows 90, cellsize 1.5, lower-left
//!   corner (-5, -5), NODATA_value -9999 (its header); its values are read
//!   here, straight from the text, for the statistics.
//! tests/geo/data/lots.geojson  one layer "lots": three polygons, fields
//!   "kind" and "name" (strings), extent (-10, 0) to (110, 40), EPSG:28356.
//! samples/gis holds parcels.geojson, terrain.asc (with terrain.prj) and
//!   survey_scan.las.
//! ```

mod info_verb {
    use std::collections::BTreeMap;
    use std::path::Path;

    use katana_app::geo::handles;
    use katana_core::ErrorCode;
    use katana_core::json::Json;
    use katana_core::text::parse_finite_double;
    use katana_gis::zip_container::write_zip;
    use katana_math::cpp;

    use crate::geo_fixture::{GeoFixture, generic, holds, must, near, refused, shared_data};
    use crate::surface_support::{data, field, number_in, quoted, records_of, samples};

    /// The C++ fixture's: its scratch folder named after the test.
    fn fixture(name: &str) -> GeoFixture {
        GeoFixture::new("katana info verb ", name)
    }

    fn parse(text: &str) -> Json {
        Json::parse(text).unwrap_or_else(|error| panic!("{error}: {text}"))
    }

    /// The sample terrain's values, read straight from the text grid - an
    /// independent reader - leaving out the no-data value its header
    /// declares.
    fn terrain_values() -> Vec<f64> {
        let text = std::fs::read_to_string(format!("{}/terrain.asc", samples()))
            .expect("the sample terrain");
        let mut words = text.split_ascii_whitespace();
        let mut no_data = 0.0;
        for _ in 0..6 {
            let key = words.next().unwrap_or_default();
            let word = words.next().unwrap_or_default();
            if key.eq_ignore_ascii_case("nodata_value") {
                no_data = parse_finite_double(word).expect("the header's no-data value");
            }
        }
        words
            .map(|word| parse_finite_double(word).expect("a value of the grid"))
            .filter(|&value| value != no_data)
            .collect()
    }

    #[test]
    fn info_of_lots_geojson_lists_its_layer_and_fields() {
        let _shared = shared_data();
        let mut fixture = fixture("InfoOfLotsGeojsonListsItsLayerAndFields");
        let reply = must(fixture.run(&format!(
            "INFO {}",
            quoted(&format!("{}/lots.geojson", data()))
        )));
        let dataset = records_of(&reply, "dataset");
        assert_eq!(dataset.len(), 1, "{reply}");
        assert_eq!(field(&dataset[0], "kind"), "vector");
        assert_eq!(field(&dataset[0], "driver"), "GeoJSON");
        holds(&field(&dataset[0], "crs"), "EPSG:28356");
        let layers = records_of(&reply, "layer");
        assert_eq!(layers.len(), 1);
        assert_eq!(field(&layers[0], "name"), "lots");
        assert_eq!(field(&layers[0], "features"), "3");
        assert_eq!(field(&layers[0], "geometry"), "Polygon");
        assert_eq!(field(&layers[0], "bounds"), "-10,0,110,40");
        assert_eq!(field(&layers[0], "fields"), "2");
        let fields = records_of(&reply, "field");
        assert_eq!(fields.len(), 2);
        assert_eq!(field(&fields[0], "layer"), "lots");
        assert_eq!(field(&fields[0], "name"), "kind");
        assert_eq!(field(&fields[0], "type"), "String");
        assert_eq!(field(&fields[1], "name"), "name");
        assert!(records_of(&reply, "band").is_empty());
    }

    #[test]
    fn info_of_typed_geojson_lists_each_field_as_gdal_types_it() {
        // GDAL's GeoJSON driver types a field from its values: a whole number
        // Integer, a fraction Real, true and false Integer of subtype
        // Boolean, and an ISO 8601 date Date (the driver's documentation,
        // "Field types"; DATE_AS_STRING is not set).
        let mut fixture = fixture("InfoOfTypedGeojsonListsEachFieldAsGdalTypesIt");
        let file = fixture.scratch.file("typed.geojson");
        std::fs::write(
            &file,
            r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","properties":{"name":"P1","count":3,"area":12.5,"surveyed":"2026-09-26","checked":true},
         "geometry":{"type":"Point","coordinates":[1,2]}}]}"#,
        )
        .expect("the typed GeoJSON written");
        let reply = must(fixture.run(&format!("INFO {}", quoted(&file))));
        let mut types = BTreeMap::new();
        for record in records_of(&reply, "field") {
            let subtype = field(&record, "subtype");
            let typed = if subtype.is_empty() {
                field(&record, "type")
            } else {
                format!("{}/{subtype}", field(&record, "type"))
            };
            types.insert(field(&record, "name"), typed);
        }
        let type_of = |name: &str| types.get(name).cloned().unwrap_or_default();
        assert_eq!(type_of("name"), "String");
        assert_eq!(type_of("count"), "Integer");
        assert_eq!(type_of("area"), "Real");
        assert_eq!(type_of("surveyed"), "Date");
        assert_eq!(type_of("checked"), "Integer/Boolean");
    }

    #[test]
    fn info_of_the_sample_terrain_describes_its_raster_and_band() {
        let _shared = shared_data();
        let mut fixture = fixture("InfoOfTheSampleTerrainDescribesItsRasterAndBand");
        let reply = must(fixture.run(&format!(
            "INFO {}",
            quoted(&format!("{}/terrain.asc", samples()))
        )));
        let dataset = records_of(&reply, "dataset");
        assert_eq!(dataset.len(), 1, "{reply}");
        assert_eq!(field(&dataset[0], "kind"), "raster");
        assert_eq!(field(&dataset[0], "driver"), "AAIGrid");
        let raster = records_of(&reply, "raster");
        assert_eq!(raster.len(), 1);
        assert_eq!(field(&raster[0], "width"), "120");
        assert_eq!(field(&raster[0], "height"), "90");
        assert_eq!(field(&raster[0], "bands"), "1");
        assert_eq!(field(&raster[0], "cell"), "1.5,1.5");
        assert_eq!(field(&raster[0], "bounds"), "-5,-5,175,130");
        let bands = records_of(&reply, "band");
        assert_eq!(bands.len(), 1);
        assert_eq!(field(&bands[0], "band"), "1");
        assert_eq!(field(&bands[0], "nodata"), "-9999");
        // Not computed, so not given: absent, not zero.
        assert_eq!(field(&bands[0], "min"), "");
        assert_eq!(field(&bands[0], "max"), "");
    }

    #[test]
    fn info_stats_of_the_sample_terrain_gives_the_min_and_max_of_its_values() {
        // A copy, so that a sidecar written beside it would be seen - and
        // none may be: the bridge puts back whatever --stats would have left.
        let mut fixture = fixture("InfoStatsOfTheSampleTerrainGivesTheMinAndMaxOfItsValues");
        let scratch = fixture.scratch.path().to_path_buf();
        std::fs::copy(
            format!("{}/terrain.asc", samples()),
            scratch.join("terrain.asc"),
        )
        .expect("the sample terrain copied");
        std::fs::copy(
            format!("{}/terrain.prj", samples()),
            scratch.join("terrain.prj"),
        )
        .expect("the sample terrain's system copied");
        let copy = fixture.scratch.file("terrain.asc");
        let reply = must(fixture.run(&format!("INFO {} STATS", quoted(&copy))));
        let bands = records_of(&reply, "band");
        assert_eq!(bands.len(), 1, "{reply}");

        let values = terrain_values();
        assert_eq!(values.len(), 120 * 90);
        let low = values.iter().copied().fold(f64::INFINITY, cpp::min);
        let high = values.iter().copied().fold(f64::NEG_INFINITY, cpp::max);
        let mut sum = 0.0;
        for &value in &values {
            sum += value;
        }
        let mean = sum / values.len() as f64;
        let mut squares = 0.0;
        for &value in &values {
            squares += (value - mean) * (value - mean);
        }
        let std_dev = (squares / values.len() as f64).sqrt();
        // GDAL reads an ASCII grid as Float32: every value below 64 is
        // rounded by at most half an ulp, 2^-19 = 1.9e-6, and so are the
        // minimum and maximum. The mean of values each off by at most 1.9e-6
        // is off by at most that, and a standard deviation moves by no more
        // than the largest change of any value. GDAL's is the population
        // deviation (N, not N - 1), which for 10 800 values differs from the
        // other by 4.6e-5 relative - ten times this tolerance at a deviation
        // of 3.
        const FLOAT32: f64 = 2e-6;
        near(number_in(&bands[0], "min"), low, FLOAT32);
        near(number_in(&bands[0], "max"), high, FLOAT32);
        near(number_in(&bands[0], "mean"), mean, FLOAT32);
        near(number_in(&bands[0], "stddev"), std_dev, FLOAT32);
        near(low, 24.892, 1e-12);
        near(high, 38.819, 1e-12);

        assert!(!scratch.join("terrain.asc.aux.xml").exists());
    }

    #[test]
    fn info_json_is_gdals_json() {
        let _shared = shared_data();
        let mut fixture = fixture("InfoJsonIsGdalsJson");
        let terrain = format!("{}/terrain.asc", samples());
        let json = parse(&must(
            fixture.run(&format!("INFO {} JSON", quoted(&terrain))),
        ));
        assert_eq!(json["path"], Json::from(terrain.as_str()));
        // GDAL's own raster info, verbatim: its keys, its driver.
        assert_eq!(json["raster"]["driverShortName"], Json::from("AAIGrid"));
        assert_eq!(
            json["raster"]["size"],
            Json::array_of([Json::from(120), Json::from(90)])
        );
        assert!(json["vector"].is_null());

        let lots = parse(&must(fixture.run(&format!(
            "INFO {} JSON",
            quoted(&format!("{}/lots.geojson", data()))
        ))));
        assert_eq!(lots["vector"]["driverShortName"], Json::from("GeoJSON"));
        assert_eq!(lots["vector"]["layers"][0]["featureCount"], Json::from(3));
        assert!(lots["raster"].is_null());
    }

    #[test]
    fn info_of_a_folder_identifies_each_file() {
        let _shared = shared_data();
        let mut fixture = fixture("InfoOfAFolderIdentifiesEachFile");
        let reply = must(fixture.run(&format!("INFO {}", quoted(&samples()))));
        let dataset = records_of(&reply, "dataset");
        assert_eq!(dataset.len(), 1, "{reply}");
        assert_eq!(field(&dataset[0], "kind"), "folder");
        assert_eq!(field(&dataset[0], "datasets"), "3");
        let mut drivers = BTreeMap::new();
        for found in records_of(&reply, "found") {
            let file = field(&found, "file");
            let name = Path::new(&file)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            drivers.insert(name, field(&found, "driver"));
        }
        let driver_of = |name: &str| drivers.get(name).cloned().unwrap_or_default();
        assert_eq!(driver_of("parcels.geojson"), "GeoJSON");
        assert_eq!(driver_of("terrain.asc"), "AAIGrid");
        assert_eq!(driver_of("survey_scan.las"), "readers.las");

        let parsed = parse(&must(
            fixture.run(&format!("INFO {} JSON", quoted(&samples()))),
        ));
        assert_eq!(parsed["identify"].size(), 2); // GDAL's, which reads no LAS
        assert_eq!(parsed["pointclouds"].size(), 1);
    }

    #[test]
    fn info_check_of_a_valid_file_returns_zero() {
        let _shared = shared_data();
        let mut fixture = fixture("InfoCheckOfAValidFileReturnsZero");
        let reply = must(fixture.run(&format!(
            "INFO {} CHECK",
            quoted(&format!("{}/terrain.asc", samples()))
        )));
        let check = records_of(&reply, "check");
        assert_eq!(check.len(), 1, "{reply}");
        assert_eq!(field(&check[0], "code"), "0");
        assert_eq!(field(&check[0], "problems"), "0");
        assert!(records_of(&reply, "problem").is_empty());
    }

    #[test]
    fn info_of_a_vsizip_path_works() {
        let mut fixture = fixture("InfoOfAVsizipPathWorks");
        let bytes = std::fs::read(format!("{}/lots.geojson", data())).expect("lots.geojson");
        let archive = fixture.scratch.path().join("lots.zip");
        must(write_zip(&archive, "lots.geojson", &bytes));
        let inside = format!("/vsizip/{}/lots.geojson", generic(&archive));
        let reply = must(fixture.run(&format!("INFO {}", quoted(&inside))));
        let layers = records_of(&reply, "layer");
        assert_eq!(layers.len(), 1, "{reply}");
        assert_eq!(field(&layers[0], "name"), "lots");
        assert_eq!(field(&layers[0], "features"), "3");
    }

    #[test]
    fn info_layer_names_one_layer_and_an_unknown_one_is_refused() {
        let _shared = shared_data();
        let mut fixture = fixture("InfoLayerNamesOneLayerAndAnUnknownOneIsRefused");
        let lots = quoted(&format!("{}/lots.geojson", data()));
        let one = must(fixture.run(&format!("INFO {lots} LAYER lots")));
        assert_eq!(records_of(&one, "layer").len(), 1);
        let none = fixture.run(&format!("INFO {lots} LAYER roads"));
        assert!(none.is_err());
    }

    #[test]
    fn info_of_a_netcdf_gives_its_multidimensional_json() {
        // A netCDF made here from the plane fixture, by GDAL itself; a GDAL
        // without the driver cannot make one, and the test says so.
        let _shared = shared_data();
        let mut fixture = fixture("InfoOfANetcdfGivesItsMultidimensionalJson");
        let nc = fixture.scratch.file("plane.nc");
        let made = fixture.run(&format!(
            "GDAL raster convert {} {}",
            quoted(&format!("{}/plane.asc", data())),
            quoted(&nc)
        ));
        if let Err(error) = &made {
            eprintln!("no netCDF driver: {}", error.describe());
            return;
        }
        let json = parse(&must(fixture.run(&format!("INFO {} JSON", quoted(&nc)))));
        assert_eq!(json["raster"]["driverShortName"], Json::from("netCDF"));
        assert!(json["multidim"].is_object(), "{}", json.dump());
        assert_eq!(json["multidim"]["type"], Json::from("group"));
    }

    #[test]
    fn info_refuses_what_it_cannot_read() {
        let mut fixture = fixture("InfoRefusesWhatItCannotRead");
        let usage = refused(fixture.run("INFO"), "INFO alone");
        assert_eq!(usage.code, ErrorCode::InvalidArgument);
        let word = refused(
            fixture.run(&format!(
                "INFO {} SHUFFLE",
                quoted(&format!("{}/terrain.asc", samples()))
            )),
            "a word INFO does not take",
        );
        assert_eq!(word.code, ErrorCode::InvalidArgument);
        let absent = fixture.scratch.file("absent.tif");
        let missing = refused(
            fixture.run(&format!("INFO {}", quoted(&absent))),
            "a file that is not there",
        );
        assert_eq!(missing.code, ErrorCode::NotFound);
        // A file with blanks in its name, unquoted, as INFO has always read
        // it.
        std::fs::copy(
            format!("{}/lots.geojson", data()),
            fixture.scratch.path().join("site lots.geojson"),
        )
        .expect("lots.geojson copied");
        let unquoted = fixture.scratch.file("site lots.geojson");
        if let Err(error) = fixture.run(&format!("INFO {unquoted}")) {
            panic!("{}", error.describe());
        }
    }

    #[test]
    fn info_of_an_entity_id_still_describes_the_entity() {
        // The interpreter's INFO: the executor leaves it alone.
        let mut fixture = fixture("InfoOfAnEntityIdStillDescribesTheEntity");
        fixture.type_line("RECT 0,0 10,5");
        assert!(!handles("INFO 1"));
        assert!(!handles("INFO #1"));
        let entity = must(fixture.interpreter.run(&mut fixture.document, "INFO 1"));
        assert_eq!(
            entity,
            "1  Polyline  layer=0  vertices=4  closed  length=30  area=50"
        );
    }
}
