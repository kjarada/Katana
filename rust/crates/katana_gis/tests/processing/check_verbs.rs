//! tests/geo/test_check_verbs.cpp: the GisCheckContract suite, the GDAL
//! arguments the GIS CHECK verbs bind. The file's other suites drive the
//! verbs through the app layer (step 6).

mod gis_check_contract {
    use katana_gis::processing::ArgType;

    use crate::contract_support::expect_argument;

    #[test]
    fn the_arguments_the_verbs_bind_are_gdals() {
        expect_argument(
            &["vector", "check-geometry"],
            "include-field",
            ArgType::StringList,
            false,
        );
        expect_argument(&["vector", "make-valid"], "method", ArgType::String, false);
        expect_argument(
            &["vector", "check-coverage"],
            "include-valid",
            ArgType::Boolean,
            false,
        );
        expect_argument(
            &["vector", "check-coverage"],
            "maximum-gap-width",
            ArgType::Real,
            false,
        );
        expect_argument(
            &["vector", "clean-coverage"],
            "maximum-gap-width",
            ArgType::Real,
            false,
        );
        expect_argument(
            &["vector", "clean-coverage"],
            "snapping-distance",
            ArgType::Real,
            false,
        );
        expect_argument(
            &["vector", "clean-coverage"],
            "merge-strategy",
            ArgType::String,
            false,
        );
    }
}
