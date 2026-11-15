//! tests/geo/test_contour_verb.cpp: the ContourContract suite, the GDAL
//! arguments CONTOUR binds. The file's ContourEntities suite is interop's
//! (katana_interop's drawing_bindings); its other suites drive the verb
//! through the app layer (step 6).

mod contour_contract {
    use katana_gis::processing::ArgType;

    use crate::contract_support::expect_argument;

    #[test]
    fn the_raster_engine_still_finds_the_arguments_it_binds() {
        expect_argument(&["raster", "contour"], "input", ArgType::DatasetList, true);
        expect_argument(&["raster", "contour"], "interval", ArgType::Real, false);
        expect_argument(&["raster", "contour"], "offset", ArgType::Real, false);
        expect_argument(
            &["raster", "contour"],
            "elevation-name",
            ArgType::String,
            false,
        );
        expect_argument(&["raster", "contour"], "3d", ArgType::Boolean, false);
        expect_argument(&["raster", "clip"], "bbox", ArgType::RealList, false);
        expect_argument(
            &["raster", "neighbors"],
            "kernel",
            ArgType::StringList,
            true,
        );
        expect_argument(&["raster", "neighbors"], "size", ArgType::Integer, false);
    }
}
