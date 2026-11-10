//! tests/geo/test_shade_verb.cpp: the ShadeContract suite, the GDAL
//! arguments RASTER SHADE binds. The file's ColourRamps suite is interop's
//! (katana_interop's catalogue target); its other suites drive the verb
//! through the app layer (step 6).

mod shade_contract {
    use katana_gis::processing::ArgType;

    use crate::contract_support::expect_argument;

    #[test]
    fn the_shading_still_finds_the_arguments_it_binds() {
        expect_argument(&["raster", "hillshade"], "azimuth", ArgType::Real, false);
        expect_argument(&["raster", "hillshade"], "altitude", ArgType::Real, false);
        expect_argument(&["raster", "hillshade"], "zfactor", ArgType::Real, false);
        expect_argument(&["raster", "hillshade"], "variant", ArgType::String, false);
        expect_argument(
            &["raster", "color-map"],
            "color-map",
            ArgType::String,
            false,
        );
        expect_argument(
            &["raster", "color-map"],
            "add-alpha",
            ArgType::Boolean,
            false,
        );
        expect_argument(&["raster", "blend"], "input", ArgType::DatasetList, true);
        expect_argument(&["raster", "blend"], "overlay", ArgType::Dataset, true);
        expect_argument(&["raster", "blend"], "operator", ArgType::String, false);
        expect_argument(&["raster", "resize"], "size", ArgType::StringList, true);
        expect_argument(&["raster", "resize"], "resampling", ArgType::String, false);
        expect_argument(&["raster", "slope"], "unit", ArgType::String, false);
    }
}
