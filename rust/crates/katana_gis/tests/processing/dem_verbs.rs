//! tests/geo/test_dem_verbs.cpp: the DemVerbsContract suite, the GDAL
//! arguments the DEM verbs bind. The file's other suites drive the verbs
//! through the app layer (step 6).

mod dem_verbs_contract {
    use katana_gis::processing::{ArgType, describe};

    use crate::contract_support::expect_argument;
    use crate::{must, strings};

    #[test]
    fn gdals_raster_tools_take_the_arguments_the_verbs_bind() {
        expect_argument(&["raster", "mosaic"], "input", ArgType::DatasetList, true);
        expect_argument(&["raster", "mosaic"], "output", ArgType::Dataset, true);
        expect_argument(&["raster", "mosaic"], "resolution", ArgType::String, false);
        expect_argument(
            &["raster", "mosaic"],
            "absolute-path",
            ArgType::Boolean,
            false,
        );
        expect_argument(&["raster", "clip"], "input", ArgType::DatasetList, true);
        expect_argument(&["raster", "clip"], "bbox", ArgType::RealList, false);
        expect_argument(&["raster", "clip"], "like", ArgType::Dataset, false);
        expect_argument(
            &["raster", "fill-nodata"],
            "input",
            ArgType::DatasetList,
            true,
        );
        expect_argument(
            &["raster", "fill-nodata"],
            "max-distance",
            ArgType::Integer,
            false,
        );
        expect_argument(
            &["raster", "fill-nodata"],
            "smoothing-iterations",
            ArgType::Integer,
            false,
        );
        expect_argument(
            &["raster", "fill-nodata"],
            "strategy",
            ArgType::String,
            false,
        );
        expect_argument(
            &["raster", "footprint"],
            "input",
            ArgType::DatasetList,
            true,
        );
        expect_argument(&["raster", "footprint"], "output", ArgType::Dataset, true);
        expect_argument(
            &["raster", "reproject"],
            "input",
            ArgType::DatasetList,
            true,
        );
        expect_argument(
            &["raster", "reproject"],
            "output-crs",
            ArgType::String,
            false,
        );
        expect_argument(
            &["raster", "reproject"],
            "input-crs",
            ArgType::String,
            false,
        );
        expect_argument(&["raster", "reproject"], "like", ArgType::Dataset, false);
        expect_argument(
            &["raster", "reproject"],
            "resampling",
            ArgType::String,
            false,
        );
        expect_argument(
            &["raster", "reproject"],
            "resolution",
            ArgType::RealList,
            false,
        );
        expect_argument(&["raster", "calc"], "input", ArgType::DatasetList, true);
        expect_argument(&["raster", "calc"], "calc", ArgType::StringList, true);
        expect_argument(&["raster", "calc"], "dialect", ArgType::String, false);
        // DIFFERENCE subtracts with the builtin dialect: this GDAL has
        // neither muparser nor ExprTk (docs/geoprocessing.md).
        let calc = must(describe(&strings(&["raster", "calc"])));
        for arg in calc.args.iter().filter(|arg| arg.name == "dialect") {
            assert!(arg.choices.iter().any(|choice| choice == "builtin"));
        }
    }
}
