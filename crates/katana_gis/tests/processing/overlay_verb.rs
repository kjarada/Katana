//! tests/geo/test_overlay_verb.cpp: the GisOverlayContract suite, the GDAL
//! arguments GIS OVERLAY binds. The file's other suites drive the verb
//! through the app layer (step 6).

mod gis_overlay_contract {
    use katana_gis::processing::{ArgType, describe};

    use crate::contract_support::expect_argument;
    use crate::{must, strings};

    #[test]
    fn the_arguments_the_verb_binds_are_gdals() {
        expect_argument(
            &["vector", "layer-algebra"],
            "operation",
            ArgType::String,
            true,
        );
        expect_argument(
            &["vector", "layer-algebra"],
            "input",
            ArgType::Dataset,
            true,
        );
        expect_argument(
            &["vector", "layer-algebra"],
            "method",
            ArgType::Dataset,
            true,
        );
        expect_argument(
            &["vector", "layer-algebra"],
            "input-field",
            ArgType::StringList,
            false,
        );
        expect_argument(
            &["vector", "layer-algebra"],
            "method-field",
            ArgType::StringList,
            false,
        );
        expect_argument(&["vector", "filter"], "where", ArgType::String, false);
        let spec = must(describe(&strings(&["vector", "layer-algebra"])));
        for arg in spec.args.iter().filter(|arg| arg.name == "operation") {
            for operation in [
                "intersection",
                "erase",
                "union",
                "sym-difference",
                "identity",
                "update",
                "clip",
            ] {
                assert!(
                    arg.choices.iter().any(|choice| choice == operation),
                    "{operation}"
                );
            }
        }
    }
}
