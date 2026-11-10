//! tests/geo/test_slope_verbs.cpp: the SlopeContract suite, the GDAL
//! arguments the slope and aspect analysis binds. The file's other suites
//! drive the verbs through the app layer (step 6).

mod slope_contract {
    use katana_gis::processing::ArgType;

    use crate::contract_support::expect_argument;

    #[test]
    fn the_analysis_still_finds_the_arguments_it_binds() {
        expect_argument(&["raster", "slope"], "unit", ArgType::String, false);
        expect_argument(&["raster", "aspect"], "convention", ArgType::String, false);
        expect_argument(&["raster", "reclassify"], "mapping", ArgType::String, true);
        expect_argument(
            &["raster", "reclassify"],
            "output-data-type",
            ArgType::String,
            false,
        );
        expect_argument(
            &["raster", "sieve"],
            "size-threshold",
            ArgType::Integer,
            false,
        );
        expect_argument(
            &["raster", "polygonize"],
            "attribute-name",
            ArgType::String,
            false,
        );
        expect_argument(&["raster", "clip"], "geometry", ArgType::String, false);
    }
}
