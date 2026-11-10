//! tests/geo/test_grid_verb.cpp: the RasterGridContract suite, the GDAL
//! arguments RASTER GRID binds. The file's other suites drive the verb
//! through the app layer (step 6).

mod raster_grid_contract {
    use katana_gis::processing::{ArgType, describe};

    use crate::contract_support::expect_argument;
    use crate::{must, strings};

    #[test]
    fn vector_grid_takes_the_arguments_the_verb_binds() {
        let linear = ["vector", "grid", "linear"];
        expect_argument(&linear, "input", ArgType::DatasetList, true);
        expect_argument(&linear, "output", ArgType::Dataset, true);
        expect_argument(&linear, "extent", ArgType::RealList, false);
        expect_argument(&linear, "resolution", ArgType::RealList, false);
        expect_argument(&linear, "size", ArgType::IntegerList, false);
        expect_argument(&linear, "zfield", ArgType::String, false);
        expect_argument(&linear, "nodata", ArgType::Real, false);
        expect_argument(&linear, "input-layer", ArgType::StringList, false);
        expect_argument(&linear, "radius", ArgType::Real, false);
        expect_argument(
            &["vector", "grid", "invdist"],
            "power",
            ArgType::Real,
            false,
        );
        // The output takes a name only: the bridge stages it
        // (docs/geoprocessing.md, "Names only: staging"), which the verb
        // relies on.
        let spec = must(describe(&strings(&linear)));
        for arg in spec.args.iter().filter(|arg| arg.name == "output") {
            assert!(arg.accepts_name);
            assert!(!arg.accepts_object);
        }
    }
}
