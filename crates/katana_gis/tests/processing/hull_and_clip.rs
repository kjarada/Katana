//! tests/geo/test_hull_and_clip.cpp: the GisHullClipContract suite, the GDAL
//! arguments GIS HULL and GIS CLIP bind. The file's other suites drive the
//! verbs through the app layer (step 6).

mod gis_hull_clip_contract {
    use katana_gis::processing::ArgType;

    use crate::contract_support::expect_argument;

    #[test]
    fn the_arguments_the_verbs_bind_are_gdals() {
        expect_argument(&["vector", "concave-hull"], "ratio", ArgType::Real, true);
        expect_argument(
            &["vector", "concave-hull"],
            "allow-holes",
            ArgType::Boolean,
            false,
        );
        expect_argument(&["vector", "clip"], "like", ArgType::Dataset, false);
        expect_argument(&["vector", "clip"], "like-layer", ArgType::String, false);
        expect_argument(&["vector", "clip"], "like-where", ArgType::String, false);
    }
}
