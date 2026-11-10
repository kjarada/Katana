//! tests/geo/test_processing_contract.cpp: what the foundation binds of
//! GDAL's algorithms, pinned (contract_support.rs, docs/geoprocessing.md
//! "Contract tests"). A GDAL upgrade that renames or reshapes one of these
//! fails here, naming it.

mod gdal_contract {
    use katana_core::text::equals_ignoring_case;
    use katana_gis::processing::{ArgType, catalogue, describe, path_text};

    use crate::contract_support::expect_argument;
    use crate::strings;

    #[test]
    fn the_foundations_algorithms_take_the_arguments_it_binds() {
        // Every raster input became a dataset list in 3.13.
        expect_argument(
            &["raster", "hillshade"],
            "input",
            ArgType::DatasetList,
            true,
        );
        expect_argument(&["raster", "hillshade"], "output", ArgType::Dataset, true);
        expect_argument(
            &["raster", "hillshade"],
            "output-format",
            ArgType::String,
            false,
        );
        expect_argument(
            &["raster", "hillshade"],
            "overwrite",
            ArgType::Boolean,
            false,
        );
        expect_argument(
            &["raster", "hillshade"],
            "creation-option",
            ArgType::StringList,
            false,
        );
        expect_argument(&["vector", "buffer"], "input", ArgType::DatasetList, true);
        expect_argument(&["vector", "buffer"], "distance", ArgType::Real, true);
        expect_argument(&["raster", "info"], "output-string", ArgType::String, false);
        expect_argument(
            &["dataset", "check"],
            "return-code",
            ArgType::Integer,
            false,
        );
        expect_argument(&["raster", "calc"], "input", ArgType::DatasetList, true);
        expect_argument(&["pipeline"], "pipeline", ArgType::String, false);
        expect_argument(&["pipeline"], "input", ArgType::DatasetList, false);
    }

    #[test]
    fn no_dataset_argument_is_named_like_a_source_keyword() {
        // FROM <arg> <source>: an argument named like a source or a scope
        // word would be read as the source, and FROM could not name it.
        let keywords = [
            "raster",
            "surface",
            "file",
            "selection",
            "sel",
            "drawing",
            "all",
            "view",
            "area",
            "layers",
            "layer",
            "where",
            "from",
            "to",
            "confirm",
            "overwrite",
            "preview",
        ];
        for info in catalogue() {
            if info.container {
                continue;
            }
            let spec = describe(&info.path)
                .unwrap_or_else(|error| panic!("{}: {}", path_text(&info.path), error.describe()));
            for arg in spec.args.iter().filter(|arg| arg.is_dataset()) {
                let names = std::iter::once(&arg.name).chain(&arg.aliases);
                for name in names {
                    for keyword in keywords {
                        assert!(
                            !equals_ignoring_case(name.as_bytes(), keyword.as_bytes()),
                            "{} {name}",
                            path_text(&info.path)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn name_only_inputs_are_the_ones_the_bridge_stages() {
        // The measured exceptions to "a name or an object": these take names
        // only, which is why a grid in memory is staged under /vsimem for
        // them.
        let paths: [&[&str]; 3] = [
            &["raster", "calc"],
            &["raster", "index"],
            &["vector", "index"],
        ];
        for path in paths {
            let path = strings(path);
            let spec = describe(&path)
                .unwrap_or_else(|error| panic!("{}: {}", path_text(&path), error.describe()));
            let mut found = false;
            for arg in spec.args.iter().filter(|arg| arg.name == "input") {
                found = true;
                assert!(
                    arg.accepts_name && !arg.accepts_object,
                    "{}",
                    path_text(&path)
                );
            }
            assert!(found, "{}", path_text(&path));
        }
    }
}
