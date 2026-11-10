//! tests/geo/test_viewshed_and_los.cpp: the ViewshedContract suite, the GDAL
//! arguments VIEWSHED binds. The file's LineOfSight suite is terrain's
//! (katana_terrain's geo_terrain); its other suites drive the verbs through
//! the app layer (step 6).

mod viewshed_contract {
    use katana_gis::processing::{ArgType, describe};

    use crate::contract_support::expect_argument;
    use crate::{must, strings};

    #[test]
    fn the_verb_still_finds_the_arguments_it_binds() {
        expect_argument(&["raster", "viewshed"], "input", ArgType::DatasetList, true);
        expect_argument(
            &["raster", "viewshed"],
            "position",
            ArgType::RealList,
            false,
        );
        // The eye height goes as the position's third value (X,Y,H): GDAL
        // 3.13 ignores --height beside a two-value position. The position
        // must still take three values.
        let viewshed = must(describe(&strings(&["raster", "viewshed"])));
        let three_values = viewshed
            .args
            .iter()
            .any(|arg| arg.name == "position" && arg.max_count >= 3);
        assert!(three_values);
        expect_argument(
            &["raster", "viewshed"],
            "target-height",
            ArgType::Real,
            false,
        );
        expect_argument(
            &["raster", "viewshed"],
            "max-distance",
            ArgType::Real,
            false,
        );
        // The curvature the hand-worked shadows switch off.
        expect_argument(
            &["raster", "viewshed"],
            "curvature-coefficient",
            ArgType::Real,
            false,
        );
        expect_argument(
            &["raster", "viewshed"],
            "visible-value",
            ArgType::Real,
            false,
        );
        expect_argument(
            &["raster", "polygonize"],
            "attribute-name",
            ArgType::String,
            false,
        );
    }
}
