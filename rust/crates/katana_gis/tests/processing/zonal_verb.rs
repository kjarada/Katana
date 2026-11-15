//! tests/geo/test_zonal_verb.cpp: the ZonalContract suite, the GDAL arguments
//! RASTER ZONAL binds. The file's other suites drive the verb through the
//! app layer (step 6).

mod zonal_contract {
    use katana_gis::processing::ArgType;

    use crate::contract_support::expect_argument;

    #[test]
    fn the_verb_still_finds_the_arguments_it_binds() {
        expect_argument(
            &["raster", "zonal-stats"],
            "input",
            ArgType::DatasetList,
            true,
        );
        expect_argument(&["raster", "zonal-stats"], "zones", ArgType::Dataset, true);
        expect_argument(
            &["raster", "zonal-stats"],
            "stat",
            ArgType::StringList,
            true,
        );
        expect_argument(
            &["raster", "zonal-stats"],
            "include-field",
            ArgType::StringList,
            false,
        );
        expect_argument(&["raster", "zonal-stats"], "pixels", ArgType::String, false);
    }
}
