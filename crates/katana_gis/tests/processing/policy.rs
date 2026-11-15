//! tests/geo/test_policy.cpp: which GDAL algorithms run without asking
//! (katana_gis::processing's policy, docs/geoprocessing.md "Safety").

mod gdal_policy {
    use katana_core::json::Json;
    use katana_gis::processing::{
        AlgorithmInfo, Policy, catalogue, check_tokens, describe, path_text, reads_heights,
        step_policy, tail_effects,
    };

    use crate::{must, strings};

    fn info_of(path: &[&str]) -> Option<&'static AlgorithmInfo> {
        catalogue().iter().find(|info| info.path == path)
    }

    /// GDAL writes an unbounded default as a bare Infinity, which is not
    /// JSON; the step names are all this reads. Each ":<bare>" becomes
    /// ":null", as the C++'s find-and-replace does.
    fn bare_numbers_as_null(mut text: String) -> String {
        for bare in ["-Infinity", "Infinity", "NaN"] {
            let needle = format!(":{bare}");
            let mut from = 0;
            while let Some(found) = text[from..].find(&needle) {
                let at = from + found;
                text.replace_range(at + 1..at + 1 + bare.len(), "null");
                from = at;
            }
        }
        text
    }

    #[test]
    fn every_leaf_has_a_policy_and_a_reason_for_it() {
        for info in catalogue() {
            if info.container {
                assert_eq!(info.policy_reason, "container", "{}", path_text(&info.path));
                continue;
            }
            let known = info.policy_reason == "listed"
                || info.policy_reason == "update"
                || info.policy_reason == "unclassified";
            assert!(known, "{}: {}", path_text(&info.path), info.policy_reason);
        }
    }

    #[test]
    fn unclassified_leaves_need_confirm_and_are_named() {
        // A leaf a GDAL upgrade adds is Confirm until someone judges it and
        // puts it in policy.rs's table. This lists every one, so the upgrade
        // that brings one says which.
        let mut unclassified = Vec::new();
        for info in catalogue() {
            if !info.container && info.policy_reason == "unclassified" {
                assert_eq!(info.policy, Policy::Confirm, "{}", path_text(&info.path));
                unclassified.push(path_text(&info.path));
            }
        }
        assert!(
            unclassified.is_empty(),
            "not yet judged in policy.rs: {}",
            unclassified.join(", ")
        );
    }

    #[test]
    fn what_changes_or_removes_existing_data_needs_confirm() {
        let paths: [&[&str]; 11] = [
            &["vsi", "delete"],
            &["vsi", "move"],
            &["vsi", "sync"],
            &["vsi", "copy"],
            &["dataset", "delete"],
            &["dataset", "rename"],
            &["raster", "edit"],
            &["raster", "overview", "add"],
            &["raster", "update"],
            &["vector", "update"],
            &["driver", "gpkg", "repack"],
        ];
        for path in paths {
            let info = info_of(path).unwrap_or_else(|| panic!("{}", path.join(" ")));
            assert_eq!(info.policy, Policy::Confirm, "{}", path.join(" "));
        }
    }

    #[test]
    fn an_input_opened_for_update_makes_an_algorithm_confirm_whatever_the_table_says() {
        // raster edit opens its dataset for update: the rule catches it even
        // before the table does.
        let edit = info_of(&["raster", "edit"]).expect("raster edit is in the catalogue");
        assert_eq!(edit.policy_reason, "update");
    }

    #[test]
    fn what_only_reads_or_writes_a_new_output_is_safe() {
        let paths: [&[&str]; 6] = [
            &["raster", "hillshade"],
            &["raster", "info"],
            &["vector", "buffer"],
            &["vector", "grid", "linear"],
            &["pipeline"],
            &["vsi", "list"],
        ];
        for path in paths {
            let info = info_of(path).unwrap_or_else(|| panic!("{}", path.join(" ")));
            assert_eq!(info.policy, Policy::Safe, "{}", path.join(" "));
        }
    }

    #[test]
    fn words_that_print_instead_of_run_are_refused() {
        let hillshade = strings(&["raster", "hillshade"]);
        for word in [
            "--help",
            "-h",
            "--help-doc",
            "--json-usage",
            "--progress",
            "--quiet",
            "--config",
        ] {
            assert!(
                check_tokens(&hillshade, &strings(&[word])).is_err(),
                "{word}"
            );
        }
        assert!(check_tokens(&hillshade, &strings(&["--zfactor=2", "-z", "2"])).is_ok());
    }

    #[test]
    fn a_pipeline_step_named_external_is_refused_wherever_it_stands() {
        assert!(
            check_tokens(
                &strings(&["pipeline"]),
                &strings(&["read", "x", "!", "external", "--command", "y"])
            )
            .is_err()
        );
        assert!(
            check_tokens(
                &strings(&["raster", "pipeline"]),
                &strings(&["EXTERNAL ! write"])
            )
            .is_err()
        );
        // A word "external" that is no step - a value - is not refused.
        assert!(
            check_tokens(
                &strings(&["raster", "pipeline"]),
                &strings(&["read external.tif ! write"])
            )
            .is_ok()
        );
        // Outside a pipeline, "external" is only a word.
        assert!(check_tokens(&strings(&["raster", "info"]), &strings(&["external"])).is_ok());
    }

    #[test]
    fn a_pipeline_step_that_writes_into_an_existing_dataset_needs_confirm_however_it_is_quoted() {
        // Words one by one, one quoted text, and --pipeline=...: the same
        // steps.
        assert_eq!(
            tail_effects(
                &strings(&["vector", "pipeline"]),
                &strings(&["read", "src.gpkg", "!", "update", "dst.gpkg", "--key", "id"])
            )
            .confirm_step,
            "update"
        );
        assert_eq!(
            tail_effects(
                &strings(&["pipeline"]),
                &strings(&["read a.tif ! UPDATE b.tif"])
            )
            .confirm_step,
            "update"
        );
        assert_eq!(
            tail_effects(
                &strings(&["raster", "pipeline"]),
                &strings(&["--pipeline=read a.tif ! update b.tif"])
            )
            .confirm_step,
            "update"
        );
        // Nested in a tee.
        assert_eq!(
            tail_effects(
                &strings(&["raster", "pipeline"]),
                &strings(&["read a.tif ! tee [ update b.tif ] ! write c.tif"])
            )
            .confirm_step,
            "update"
        );
        // A step no one has judged is Confirm, as an unclassified leaf is.
        assert_eq!(
            tail_effects(
                &strings(&["pipeline"]),
                &strings(&["read a ! frobnicate ! write b"])
            )
            .confirm_step,
            "frobnicate"
        );
        // "update" as a value, and as a word outside a pipeline, is no step.
        assert_eq!(
            tail_effects(
                &strings(&["pipeline"]),
                &strings(&["read update.tif ! write update"])
            )
            .confirm_step,
            ""
        );
        assert_eq!(
            tail_effects(&strings(&["vector", "convert"]), &strings(&["update", "x"])).confirm_step,
            ""
        );
        assert_eq!(
            tail_effects(
                &strings(&["raster", "pipeline"]),
                &strings(&["read a.tif ! slope ! write b.tif"])
            )
            .confirm_step,
            ""
        );
    }

    #[test]
    fn words_that_change_an_existing_dataset_are_found_inside_a_quoted_pipeline() {
        assert_eq!(
            tail_effects(
                &strings(&["pipeline"]),
                &strings(&["read a.asc ! write --overwrite b.tif"])
            )
            .overwrite_word,
            "--overwrite"
        );
        assert_eq!(
            tail_effects(
                &strings(&["pipeline"]),
                &strings(&["read", "a.asc", "!", "write", "--append", "b.gpkg"])
            )
            .overwrite_word,
            "--append"
        );
        assert_eq!(
            tail_effects(
                &strings(&["vector", "pipeline"]),
                &strings(&["read a.gpkg ! tee [ write --overwrite-layer=true c.gpkg ] ! write d"])
            )
            .overwrite_word,
            "--overwrite-layer=true"
        );
        assert_eq!(
            tail_effects(
                &strings(&["pipeline"]),
                &strings(&["read a.asc ! write b.tif"])
            )
            .overwrite_word,
            ""
        );
    }

    #[test]
    fn rasterize_add_and_tile_resume_change_an_existing_dataset() {
        // rasterize --add burns into the raster its output names, with no
        // --update (measured with gdal vector rasterize: the file's checksum
        // changed); tile --resume writes into a tile set already there.
        assert_eq!(
            tail_effects(
                &strings(&["vector", "rasterize"]),
                &strings(&["--burn", "5", "--add", "a.gpkg", "b.tif"])
            )
            .overwrite_word,
            "--add"
        );
        assert_eq!(
            tail_effects(
                &strings(&["raster", "tile"]),
                &strings(&["a.tif", "tiles", "--resume"])
            )
            .overwrite_word,
            "--resume"
        );
        // A longer name that begins the same is another option.
        assert_eq!(
            tail_effects(&strings(&["raster", "clip"]), &strings(&["--add-alpha"])).overwrite_word,
            ""
        );
    }

    #[test]
    fn every_step_gdals_pipelines_offer_is_judged() {
        // As every leaf is: a step a GDAL upgrade adds is Confirm until
        // judged, and this names it.
        let mut unjudged = Vec::new();
        let mut steps = 0;
        let paths: [&[&str]; 3] = [
            &["pipeline"],
            &["raster", "pipeline"],
            &["vector", "pipeline"],
        ];
        for path in paths {
            let spec = must(describe(&strings(path)));
            let usage = Json::parse(bare_numbers_as_null(spec.usage_json.clone()))
                .unwrap_or_else(|error| panic!("{}: {error}", path.join(" ")));
            assert!(usage.contains("pipeline_algorithms"), "{}", path.join(" "));
            let algorithms = usage
                .at("pipeline_algorithms")
                .unwrap_or_else(|error| panic!("{error}"));
            for step in algorithms.iter() {
                steps += 1;
                let name = step
                    .at("name")
                    .ok()
                    .and_then(Json::as_str)
                    .expect("a pipeline step has a name");
                let mut reason = String::new();
                let _ = step_policy(name, &mut reason);
                if reason == "unclassified" {
                    unjudged.push(String::from(name));
                }
            }
        }
        assert!(steps > 0);
        assert!(
            unjudged.is_empty(),
            "pipeline steps not yet judged in policy.rs: {}",
            unjudged.join(", ")
        );
        let mut reason = String::new();
        assert_eq!(step_policy("update", &mut reason), Policy::Confirm);
        assert_eq!(step_policy("write", &mut reason), Policy::Safe);
        assert_eq!(step_policy("external", &mut reason), Policy::Confirm);
        assert_eq!(reason, "refused");
    }

    #[test]
    fn only_an_algorithm_that_reads_z_is_given_heights_only() {
        assert!(reads_heights(&strings(&["vector", "grid", "nearest"]), &[]));
        assert!(reads_heights(
            &strings(&["vector", "grid", "linear"]),
            &strings(&["--size", "2,2"])
        ));
        assert!(!reads_heights(
            &strings(&["vector", "grid", "linear"]),
            &strings(&["--zfield=h"])
        ));
        assert!(!reads_heights(&strings(&["vector", "grid", "count"]), &[]));
        assert!(reads_heights(
            &strings(&["vector", "rasterize"]),
            &strings(&["--3d"])
        ));
        assert!(!reads_heights(
            &strings(&["vector", "rasterize"]),
            &strings(&["--burn", "1"])
        ));
        assert!(!reads_heights(
            &strings(&["vector", "buffer"]),
            &strings(&["--distance", "1"])
        ));
        assert!(reads_heights(
            &strings(&["vector", "pipeline"]),
            &strings(&["read x ! grid invdist ! write y"])
        ));
        assert!(!reads_heights(
            &strings(&["pipeline"]),
            &strings(&["read x ! grid invdist --zfield h ! write y"])
        ));
        assert!(!reads_heights(
            &strings(&["pipeline"]),
            &strings(&["read x ! grid count ! write y"])
        ));
        assert!(reads_heights(
            &strings(&["pipeline"]),
            &strings(&["read", "x", "!", "rasterize", "--3d", "!", "write"])
        ));
    }
}
