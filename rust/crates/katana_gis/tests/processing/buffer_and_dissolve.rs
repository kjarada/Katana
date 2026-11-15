//! tests/geo/test_buffer_and_dissolve.cpp: the GisBufferContract suite, the
//! GDAL arguments GIS BUFFER and GIS DISSOLVE bind. The file's other suites
//! drive the verbs through the app layer (step 6).

mod gis_buffer_contract {
    use katana_gis::processing::{ArgType, describe};

    use crate::contract_support::expect_argument;
    use crate::{must, strings};

    #[test]
    fn the_arguments_the_verbs_bind_are_gdals() {
        expect_argument(&["vector", "buffer"], "input", ArgType::DatasetList, true);
        expect_argument(&["vector", "buffer"], "distance", ArgType::Real, true);
        expect_argument(
            &["vector", "buffer"],
            "endcap-style",
            ArgType::String,
            false,
        );
        expect_argument(&["vector", "buffer"], "join-style", ArgType::String, false);
        expect_argument(&["vector", "buffer"], "side", ArgType::String, false);
        expect_argument(
            &["vector", "buffer"],
            "quadrant-segments",
            ArgType::Integer,
            false,
        );
        expect_argument(
            &["vector", "combine"],
            "group-by",
            ArgType::StringList,
            false,
        );
        expect_argument(
            &["vector", "combine"],
            "add-extra-fields",
            ArgType::String,
            false,
        );
        expect_argument(&["vector", "dissolve"], "input", ArgType::DatasetList, true);
        // The words the verbs pass are among GDAL's choices.
        let buffer = must(describe(&strings(&["vector", "buffer"])));
        for arg in &buffer.args {
            if arg.name == "join-style" {
                assert!(arg.choices.iter().any(|choice| choice == "mitre"));
            }
            if arg.name == "side" {
                assert!(arg.choices.iter().any(|choice| choice == "left"));
            }
        }
    }
}
