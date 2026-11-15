//! tests/geo/test_gdal_verbs.cpp: the one geoprocessing executor and the GDAL
//! verb, driven through the executor's own context, as the window and the
//! session drive it. Its GeoExecutor cases that read the reference data, keep
//! a raster as a reference raster or bind a RASTER or SURFACE source
//! (AFileOnlyGdalLineWritesTheFile, ARasterResultBecomesADerivedReferenceRaster,
//! ASourceOfAKindTheArgumentDoesNotReadIsRefusedBeforeItRuns,
//! ARasterSourceIsTheReferenceRastersOwnFile, ASurfaceIsSampledAtItsCellCentres)
//! and its GeoSession case are step 6's wave 2 (A).

mod geo_executor {
    use std::sync::atomic::{AtomicBool, Ordering};

    use katana_app::geo::bindings::{
        SourceKind, TargetKind, parse_source, parse_target, tokenize, unchanged_since,
    };
    use katana_app::geo::replies::parse_records;
    use katana_app::geo::{handles, help_text};
    use katana_cad::Document;
    use katana_core::ErrorCode;
    use katana_core::json::Json;
    use katana_entity::Geometry;
    use katana_gis::GdalDataset;
    use katana_gis::processing::describe;

    use crate::geo_fixture::{
        GeoFixture, bytes_of, geo_data, holds, lacks, must, near, refused, shared_data, strings,
    };

    /// The C++ fixture's: its scratch folder named after the test.
    fn fixture(name: &str) -> GeoFixture {
        GeoFixture::new("katana-geo-verbs-", name)
    }

    /// A file of tests/geo/data (KATANA_GEO_TEST_DATA).
    fn data(name: &str) -> String {
        format!("{}/{name}", geo_data())
    }

    /// How many times `word` is in `text`, a match starting one byte after
    /// the last.
    fn count(text: &str, word: &str) -> usize {
        let mut found = 0;
        let mut from = 0;
        while let Some(at) = text[from..].find(word) {
            found += 1;
            from += at + 1;
        }
        found
    }

    /// The polylines on `layer`, and their area: the C++ fixture's areaOn.
    fn area_on(fixture: &GeoFixture, layer: &str) -> f64 {
        let mut total = 0.0;
        fixture.document.model().entities.for_each(|entity| {
            if entity.layer == layer
                && let Geometry::Polyline(line) = &entity.geometry
            {
                total += line.area();
            }
        });
        total
    }

    fn parse_json(text: &str) -> Json {
        Json::parse(text).unwrap_or_else(|error| panic!("{error}: {text}"))
    }

    #[test]
    fn the_executor_takes_the_gdal_verb_and_nothing_else() {
        assert!(handles("GDAL VERSION"));
        assert!(handles("  gdal list raster"));
        assert!(!handles("GDALX VERSION"));
        assert!(!handles("\"GDAL\" VERSION"));
        assert!(!handles("LINE 0,0 1,1"));
        holds(&help_text(), "GDAL VERSION");
    }

    #[test]
    fn gdal_version_names_the_library() {
        let mut fixture = fixture("GdalVersionNamesTheLibrary");
        let reply = must(fixture.run("GDAL VERSION"));
        let records = parse_records(&reply);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, "gdal");
        assert!(records[0].get("version").unwrap_or("").starts_with("3."));
        assert!(records[0].get("algorithms").is_some());
    }

    #[test]
    fn gdal_list_filters_by_group() {
        let mut fixture = fixture("GdalListFiltersByGroup");
        let reply = must(fixture.run("GDAL LIST vector grid"));
        let records = parse_records(&reply);
        let mut algorithms = 0;
        for record in &records {
            if record.kind == "algorithm" {
                algorithms += 1;
                assert!(record.get("path").unwrap_or("").starts_with("vector grid "));
            }
        }
        // GDAL 3.13's gridding methods: average, average-distance,
        // average-distance-points, count, invdist, invdistnn, linear,
        // maximum, minimum, nearest, range.
        assert_eq!(algorithms, 11);
        let last = records.last().expect("the listing's summary");
        assert_eq!(last.kind, "listed");
        assert_eq!(last.get("algorithms"), Some("11"));
    }

    #[test]
    fn gdal_list_finds_words_in_the_descriptions() {
        let mut fixture = fixture("GdalListFindsWordsInTheDescriptions");
        let reply = must(fixture.run("GDAL LIST shaded"));
        holds(&reply, "path=\"raster hillshade\"");
        let json = must(fixture.run("GDAL LIST hillshade JSON"));
        let list = parse_json(&json);
        assert!(list.is_array());
        assert_eq!(list[0]["name"], Json::from("raster hillshade"));
    }

    #[test]
    fn gdal_help_prints_one_record_per_argument() {
        let mut fixture = fixture("GdalHelpPrintsOneRecordPerArgument");
        let reply = must(fixture.run("GDAL HELP raster hillshade"));
        let spec = must(describe(&strings(&["raster", "hillshade"])));
        assert_eq!(count(&reply, "\narg name="), spec.args.len());
        holds(&reply, "arg name=altitude");
        holds(&reply, "max=90 max_inclusive=yes");
        holds(
            &reply,
            "binding arg=input kinds=raster accepts=name,object sources=raster,surface,file",
        );
    }

    #[test]
    fn gdal_help_json_carries_the_arguments_schema() {
        let mut fixture = fixture("GdalHelpJsonCarriesTheArgumentsSchema");
        let reply = must(fixture.run("GDAL HELP raster hillshade JSON"));
        let described = parse_json(&reply);
        let zfactor = &described["arguments_schema"]["properties"]["zfactor"];
        assert_eq!(zfactor["type"], Json::from("number"));
        assert_eq!(zfactor["exclusiveMinimum"], Json::from(0.0));
        let altitude = &described["arguments_schema"]["properties"]["altitude"];
        assert_eq!(altitude["minimum"], Json::from(0.0));
        assert_eq!(altitude["maximum"], Json::from(90.0));
        assert_eq!(
            described["arguments_schema"]["properties"]["variant"]["enum"].size(),
            4
        );
        assert!(described["inputs_schema"]["properties"].contains("input"));
        assert!(described["gdal_usage"].is_object());
    }

    /// The properties GDAL HELP <algorithm> JSON offers its input.
    fn input_properties(fixture: &mut GeoFixture, algorithm: &str) -> Json {
        let reply = fixture.run(&format!("GDAL HELP {algorithm} JSON"));
        assert!(reply.is_ok());
        match reply {
            Ok(reply) => {
                parse_json(&reply)["inputs_schema"]["properties"]["input"]["properties"].clone()
            }
            Err(_) => Json::Null,
        }
    }

    // inputs_schema offers an argument only the sources it reads (its kinds,
    // as GDAL declares them): hillshade's raster input no drawing scope,
    // buffer's vector input no raster or surface; a file is offered to both.
    #[test]
    fn the_inputs_schema_offers_only_the_sources_an_argument_reads() {
        let mut fixture = fixture("TheInputsSchemaOffersOnlyTheSourcesAnArgumentReads");
        let raster = input_properties(&mut fixture, "raster hillshade");
        for key in ["raster", "surface", "cell", "file", "layer"] {
            assert!(raster.contains(key), "{key}");
        }
        for key in ["scope", "area", "layers", "only", "where"] {
            assert!(!raster.contains(key), "{key}");
        }
        let vector = input_properties(&mut fixture, "vector buffer");
        for key in ["scope", "area", "layers", "only", "where", "file", "layer"] {
            assert!(vector.contains(key), "{key}");
        }
        for key in ["raster", "surface", "cell"] {
            assert!(!vector.contains(key), "{key}");
        }
    }

    #[test]
    fn gdal_run_says_what_the_scope_took() {
        let mut fixture = fixture("GdalRunSaysWhatTheScopeTook");
        fixture.type_line("LINE 0,0 100,0");
        fixture.type_line("TEXT 5,5 2.5 \"LOT 7\"");
        let reply = must(fixture.run(
            "GDAL vector buffer --distance=1 --endcap-style=flat FROM DRAWING TO LAYER gis/buffer",
        ));
        holds(
            &reply,
            "scope arg=input scope=drawing matched=2 used=1 points=0 lines=1 polygons=0 \
             skipped.text=1",
        );
        holds(
            &reply,
            "output arg=output kind=vector target=layer layer=gis/buffer created=1",
        );
        // 100 m by 2 m with flat caps.
        near(area_on(&fixture, "gis/buffer"), 200.0, 1e-9);
    }

    #[test]
    fn a_result_is_one_undo_step() {
        let mut fixture = fixture("AResultIsOneUndoStep");
        fixture.type_line("LINE 0,0 100,0");
        let steps = fixture.document.history().undo_count();
        must(fixture.run("GDAL vector buffer distance=1 FROM DRAWING"));
        assert_eq!(fixture.document.history().undo_count(), steps + 1);
        // Without TO, features go to gis/<the algorithm's last word>.
        assert!(fixture.document.model().layers.contains("gis/buffer"));
        must(fixture.document.undo());
        assert_eq!(fixture.document.model().entities.size(), 1);
        assert!(!fixture.document.model().layers.contains("gis/buffer"));
    }

    #[test]
    fn view_is_refused_headless_naming_area() {
        let mut fixture = fixture("ViewIsRefusedHeadlessNamingArea");
        fixture.type_line("LINE 0,0 100,0");
        let error = refused(
            fixture.run("GDAL vector buffer distance=1 FROM VIEW"),
            "VIEW headless",
        );
        assert_eq!(error.code, ErrorCode::InvalidState);
        holds(&error.message, "AREA");
    }

    #[test]
    fn an_area_takes_what_lies_in_it() {
        let mut fixture = fixture("AnAreaTakesWhatLiesInIt");
        fixture.type_line("LINE 0,0 10,0");
        fixture.type_line("LINE 100,100 110,100");
        let reply = must(
            fixture.run("GDAL vector buffer distance=1 FROM AREA -5,-5,20,20 TO LAYER gis/near"),
        );
        holds(&reply, "scope=area area=-5,-5,20,20 matched=1");
    }

    #[test]
    fn an_empty_scope_is_reported_not_an_error() {
        let mut fixture = fixture("AnEmptyScopeIsReportedNotAnError");
        fixture.type_line("LINE 0,0 10,0");
        let reply =
            must(fixture.run("GDAL vector buffer distance=1 FROM DRAWING WHERE TYPE=circle"));
        holds(&reply, "ran=no");
        holds(&reply, "matched=0");
        assert_eq!(fixture.document.model().entities.size(), 1);
    }

    #[test]
    fn preview_changes_nothing() {
        let mut fixture = fixture("PreviewChangesNothing");
        fixture.type_line("LINE 0,0 100,0");
        let steps = fixture.document.history().undo_count();
        let reply = must(
            fixture.run("GDAL vector buffer --distance=1 FROM DRAWING TO LAYER gis/buffer PREVIEW"),
        );
        holds(&reply, "preview valid=yes changed=no");
        holds(&reply, "scope arg=input scope=drawing matched=1");
        assert_eq!(fixture.document.history().undo_count(), steps);
        assert_eq!(fixture.document.model().entities.size(), 1);
        assert!(!fixture.document.model().layers.contains("gis/buffer"));
    }

    #[test]
    fn preview_refuses_what_a_run_would_refuse() {
        let mut fixture = fixture("PreviewRefusesWhatARunWouldRefuse");
        fixture.type_line("LINE 0,0 100,0");
        let error = refused(
            fixture.run("GDAL vector buffer FROM DRAWING PREVIEW"),
            "a preview with no distance",
        );
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        holds(&error.message, "distance");
    }

    #[test]
    fn confirm_is_required_for_a_destructive_algorithm() {
        let mut fixture = fixture("ConfirmIsRequiredForADestructiveAlgorithm");
        let file = fixture.scratch.file("doomed.txt");
        std::fs::write(&file, "x").expect("the scratch folder takes a file");
        let error = refused(
            fixture.run(&format!("GDAL vsi delete \"{file}\"")),
            "vsi delete without CONFIRM",
        );
        assert_eq!(error.code, ErrorCode::Unsupported);
        holds(&error.message, "CONFIRM");
        assert!(std::path::Path::new(&file).exists());
        must(fixture.run(&format!("GDAL vsi delete \"{file}\" CONFIRM")));
        assert!(!std::path::Path::new(&file).exists());
    }

    #[test]
    fn overwrite_is_required_to_replace_a_file() {
        let _shared = shared_data();
        let mut fixture = fixture("OverwriteIsRequiredToReplaceAFile");
        let out = fixture.scratch.file("shade.tif");
        std::fs::write(&out, "not a raster").expect("the scratch folder takes a file");
        let line = format!(
            "GDAL raster hillshade FROM FILE \"{}\" TO FILE \"{out}\"",
            data("plane.asc")
        );
        let error = refused(fixture.run(&line), "a file in the way without OVERWRITE");
        assert_eq!(error.code, ErrorCode::AlreadyExists);
        must(fixture.run(&format!("{line} OVERWRITE")));
        let written = must(GdalDataset::open(std::path::Path::new(&out)));
        assert_eq!(must(written.raster_info()).width, 40);
    }

    #[test]
    fn gdals_own_words_that_change_a_file_need_overwrite() {
        let _shared = shared_data();
        let mut fixture = fixture("GdalsOwnWordsThatChangeAFileNeedOverwrite");
        let error = refused(
            fixture.run(&format!(
                "GDAL raster hillshade \"{}\" \"{}\" --overwrite",
                data("plane.asc"),
                fixture.scratch.file("out.tif")
            )),
            "--overwrite without OVERWRITE",
        );
        holds(&error.message, "OVERWRITE");
    }

    #[test]
    fn a_quoted_pipeline_that_overwrites_a_file_needs_overwrite() {
        let _shared = shared_data();
        let mut fixture = fixture("AQuotedPipelineThatOverwritesAFileNeedsOverwrite");
        let victim = fixture.scratch.file("victim.tif");
        let raster = data("plane.asc");
        must(fixture.run(&format!("GDAL raster hillshade \"{raster}\" \"{victim}\"")));
        let before = bytes_of(&victim);
        assert!(!before.is_empty());
        // One quoted text is one word to the line, and still steps to GDAL.
        let line = format!("GDAL pipeline \"read {raster} ! write --overwrite {victim}\"");
        let error = refused(fixture.run(&line), "a pipeline that overwrites");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        holds(&error.message, "OVERWRITE");
        assert_eq!(bytes_of(&victim), before);
        must(fixture.run(&format!("{line} OVERWRITE")));
        assert_ne!(bytes_of(&victim), before);
    }

    #[test]
    fn a_pipeline_update_step_needs_confirm_quoted_or_not() {
        let mut fixture = fixture("APipelineUpdateStepNeedsConfirmQuotedOrNot");
        let points = fixture.scratch.file("points.geojson");
        std::fs::write(
            &points,
            concat!(
                r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"id":"1","#,
                r#""name":"new"},"geometry":{"type":"Point","coordinates":[5,5]}}]}"#
            ),
        )
        .expect("the scratch folder takes a file");
        let target = fixture.scratch.file("target.gpkg");
        let source = fixture.scratch.file("source.gpkg");
        must(fixture.run(&format!(
            "GDAL vector convert \"{points}\" \"{target}\" --output-layer=pts"
        )));
        must(fixture.run(&format!(
            "GDAL vector convert \"{points}\" \"{source}\" --output-layer=pts"
        )));
        let before = bytes_of(&target);
        for line in [
            format!("GDAL vector pipeline read \"{source}\" ! update \"{target}\""),
            format!("GDAL vector pipeline \"read {source} ! update {target}\""),
            format!(
                "GDAL pipeline \"read {source} ! tee [ update {target} ] ! write {}\"",
                fixture.scratch.file("copy.gpkg")
            ),
        ] {
            let error = refused(fixture.run(&line), &line);
            assert_eq!(error.code, ErrorCode::Unsupported, "{line}");
            holds(&error.message, "CONFIRM");
            assert_eq!(error.context, "update");
            assert_eq!(bytes_of(&target), before, "{line}");
        }
        // Said, it runs.
        must(fixture.run(&format!(
            "GDAL vector pipeline read \"{source}\" ! update \"{target}\" CONFIRM"
        )));
    }

    #[test]
    fn rasterize_add_burns_into_an_existing_raster_only_with_overwrite() {
        let _shared = shared_data();
        let mut fixture = fixture("RasterizeAddBurnsIntoAnExistingRasterOnlyWithOverwrite");
        let raster = fixture.scratch.file("burn.tif");
        must(fixture.run(&format!(
            "GDAL raster hillshade \"{}\" \"{raster}\"",
            data("plane.asc")
        )));
        let before = bytes_of(&raster);
        let line = format!(
            "GDAL vector rasterize --burn 5 --add \"{}\" \"{raster}\"",
            data("lots.geojson")
        );
        let error = refused(fixture.run(&line), "--add without OVERWRITE");
        assert_eq!(error.context, "--add");
        assert_eq!(bytes_of(&raster), before);
    }

    #[test]
    fn a_result_in_another_crs_than_the_projects_is_said_to_be() {
        // GDAL vector reproject into EPSG:4326 makes degrees, which TO LAYER
        // draws into an MGA drawing as they are: 151 E, 34 S among 330000 m
        // eastings, and nothing said so.
        let mut fixture = fixture("AResultInAnotherCrsThanTheProjectsIsSaidToBe");
        must(
            fixture
                .document
                .set_coordinate_system("EPSG:28356", Document::SET_CRS),
        );
        fixture.type_line("LINE 330000,6250000 330100,6250000");
        let reply = must(
            fixture
                .run("GDAL vector reproject --output-crs=EPSG:4326 FROM DRAWING TO LAYER lonlat"),
        );
        holds(
            &reply,
            "warning text=\"the result is in WGS 84 (EPSG:4326), not the project's GDA94 / MGA \
             zone 56 (EPSG:28356)",
        );
        // A result in the project's own system says nothing of it.
        let same =
            must(fixture.run("GDAL vector buffer --distance 1 FROM DRAWING TO LAYER buffered"));
        lacks(&same, "not the project's");
    }

    #[test]
    fn gdal_tokens_after_a_clause_are_refused() {
        let mut fixture = fixture("GdalTokensAfterAClauseAreRefused");
        fixture.type_line("LINE 0,0 100,0");
        let error = refused(
            fixture.run("GDAL vector buffer FROM DRAWING --distance=1"),
            "GDAL's words after FROM",
        );
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(error.context, "--distance=1");
    }

    #[test]
    fn an_output_gdals_words_name_is_replaced_only_with_overwrite() {
        let _shared = shared_data();
        let mut fixture = fixture("AnOutputGdalsWordsNameIsReplacedOnlyWithOverwrite");
        let line = format!(
            "GDAL raster hillshade \"{}\" \"{}\"",
            data("plane.asc"),
            fixture.scratch.file("shade.tif")
        );
        must(fixture.run(&line));
        // GDAL refuses to write over it by itself, and so the line does too.
        let again = refused(fixture.run(&line), "a second run onto the same file");
        holds(&again.message, "already exists");
        must(fixture.run(&format!("{line} OVERWRITE")));
    }

    #[test]
    fn a_raster_cannot_go_to_a_layer_nor_features_to_a_reference() {
        let _shared = shared_data();
        let mut fixture = fixture("ARasterCannotGoToALayerNorFeaturesToAReference");
        fixture.type_line("LINE 0,0 100,0");
        let raster = refused(
            fixture.run(&format!(
                "GDAL raster hillshade FROM FILE \"{}\" TO LAYER gis/x",
                data("plane.asc")
            )),
            "a raster to a layer",
        );
        assert_eq!(raster.code, ErrorCode::Unsupported);
        let vector = refused(
            fixture.run("GDAL vector buffer distance=1 FROM DRAWING TO REFERENCE b"),
            "features to a reference",
        );
        assert_eq!(vector.code, ErrorCode::Unsupported);
    }

    #[test]
    fn from_binds_its_named_dataset_argument() {
        // Two 50 x 40 lots and a 4 m corridor across both, drawn on layers of
        // their own: the corridor's intersection with each lot is 200 m2.
        let mut fixture = fixture("FromBindsItsNamedDatasetArgument");
        fixture.type_line("LAYER NEW lots");
        fixture.type_line("LAYER NEW corridor");
        fixture.type_line("LAYER SET lots");
        fixture.type_line("PLINE 0,0 50,0 50,40 0,40 CLOSE");
        fixture.type_line("PLINE 50,0 100,0 100,40 50,40 CLOSE");
        fixture.type_line("LAYER SET corridor");
        fixture.type_line("PLINE -10,18 110,18 110,22 -10,22 CLOSE");
        let reply = must(fixture.run(
            "GDAL vector layer-algebra intersection FROM input LAYERS lots FROM method LAYERS \
             corridor TO LAYER gis/overlay",
        ));
        holds(&reply, "scope arg=input scope=layers layers=lots");
        holds(&reply, "scope arg=method scope=layers layers=corridor");
        holds(&reply, "created=2");
        near(area_on(&fixture, "gis/overlay"), 400.0, 1e-9);
    }

    #[test]
    fn an_input_given_twice_is_refused() {
        let _shared = shared_data();
        let mut fixture = fixture("AnInputGivenTwiceIsRefused");
        let plane = data("plane.asc");
        let error = refused(
            fixture.run(&format!(
                "GDAL raster hillshade --input=\"{plane}\" FROM FILE \"{plane}\""
            )),
            "an input given twice",
        );
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        holds(&error.message, "twice");
    }

    #[test]
    fn from_names_only_an_input_dataset() {
        let mut fixture = fixture("FromNamesOnlyAnInputDataset");
        let error = refused(
            fixture.run("GDAL raster hillshade FROM zfactor FILE x.tif"),
            "FROM naming an argument that is no dataset",
        );
        assert_eq!(error.context, "zfactor");
    }

    #[test]
    fn a_cancelled_run_changes_nothing() {
        let mut fixture = fixture("ACancelledRunChangesNothing");
        fixture.type_line("LINE 0,0 100,0");
        let prepared = must(fixture.prepare("GDAL vector buffer distance=1 FROM DRAWING"));
        assert!(prepared.reply.is_none());
        let stop = AtomicBool::new(true);
        let work = prepared.work.expect("a run has work");
        let error = refused(work(Some(&stop), None), "a stopped run");
        assert_eq!(error.message, "cancelled");
        assert_eq!(fixture.document.model().entities.size(), 1);
        assert!(!fixture.document.model().layers.contains("gis/buffer"));
    }

    #[test]
    fn a_cancelled_run_leaves_the_file_it_would_have_replaced_as_it_was() {
        // OVERWRITE let GDAL write over the file from the start, so a run
        // cancelled part way left a partial file where the original had been.
        let _shared = shared_data();
        let mut fixture = fixture("ACancelledRunLeavesTheFileItWouldHaveReplacedAsItWas");
        let out = fixture.scratch.file("shade.tif");
        must(fixture.run(&format!(
            "GDAL raster hillshade \"{}\" \"{out}\"",
            data("plane.asc")
        )));
        let before = bytes_of(&out);
        assert!(!before.is_empty());
        let line = format!(
            "GDAL raster hillshade --zfactor=3 FROM FILE \"{}\" TO FILE \"{out}\" OVERWRITE",
            data("plane.asc")
        );
        let prepared = must(fixture.prepare(&line));
        let stop = AtomicBool::new(false);
        let reported = AtomicBool::new(false);
        let progress: &(dyn Fn(f64) + Sync) = &|_| {
            reported.store(true, Ordering::SeqCst);
            stop.store(true, Ordering::SeqCst);
        };
        let work = prepared.work.expect("a run has work");
        let apply = work(Some(&stop), Some(progress));
        assert!(
            reported.load(Ordering::SeqCst),
            "the run never reported progress, so it was never stopped part way"
        );
        let error = refused(apply, "a run stopped part way");
        assert_eq!(error.message, "cancelled");
        assert_eq!(bytes_of(&out), before);
        // Nothing is left beside it either.
        let files = std::fs::read_dir(fixture.scratch.path())
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".katana-staging")
                    })
                    .count()
            })
            .unwrap_or(0);
        assert_eq!(files, 0);
        // Run to its end, it replaces the file, and the reply names it.
        let replaced = must(fixture.run(&line));
        assert_ne!(bytes_of(&out), before);
        holds(&replaced, &format!("file={out}"));
    }

    #[test]
    fn the_work_reads_only_what_prepare_copied() {
        // The work runs on another thread in the window; what it reads was
        // copied at prepare, so a drawing changed meanwhile does not reach
        // it, and a result that only creates is applied all the same.
        let mut fixture = fixture("TheWorkReadsOnlyWhatPrepareCopied");
        fixture.type_line("LINE 0,0 100,0");
        let prepared =
            must(fixture.prepare("GDAL vector buffer distance=1 --endcap-style=flat FROM DRAWING"));
        fixture.type_line("SELECT ALL");
        fixture.type_line("ERASE");
        let work = prepared.work.expect("a run has work");
        let apply = must(work(None, None));
        must(apply(&mut fixture.context()));
        near(area_on(&fixture, "gis/buffer"), 200.0, 1e-9);
    }

    #[test]
    fn an_in_place_apply_refuses_when_the_drawing_changed_while_the_job_ran() {
        let mut fixture = fixture("AnInPlaceApplyRefusesWhenTheDrawingChangedWhileTheJobRan");
        fixture.type_line("LINE 0,0 100,0");
        let copy = fixture
            .document
            .model()
            .entities
            .find(1)
            .cloned()
            .expect("the line is entity 1");
        assert!(unchanged_since(&fixture.context(), std::slice::from_ref(&copy)).is_ok());
        fixture.type_line("SELECT 1");
        fixture.type_line("MOVE 5,5");
        let changed = refused(
            unchanged_since(&fixture.context(), std::slice::from_ref(&copy)),
            "a moved entity",
        );
        assert_eq!(changed.code, ErrorCode::InvalidState);
        assert_eq!(changed.message, "the drawing changed while the job ran");
    }

    #[test]
    fn the_source_and_target_clauses_read_what_the_grammar_says() {
        let tokens = must(tokenize(
            "FROM SURFACE g CELL 0.5 FROM FILE \"a b.tif\" LAYER roads FROM LAYERS a,b ONLY \
             WHERE TYPE=polyline TO REFERENCE shade",
        ));
        let mut at = 1;
        let surface = must(parse_source(&tokens, &mut at));
        assert_eq!(surface.kind, SourceKind::Surface);
        assert_eq!(surface.surface, "g");
        assert_eq!(surface.cell, Some(0.5));
        at += 1;
        let file = must(parse_source(&tokens, &mut at));
        assert_eq!(file.path, "a b.tif");
        assert_eq!(file.layer, "roads");
        at += 1;
        let layers = must(parse_source(&tokens, &mut at));
        assert_eq!(layers.kind, SourceKind::Drawing);
        assert_eq!(layers.scope.layers, strings(&["a", "b"]));
        assert!(!layers.scope.sublayers);
        assert_eq!(layers.scope.filter.types.len(), 1);
        assert!(tokens.is(at, "TO"));
        at += 1;
        let target = must(parse_target(&tokens, &mut at));
        assert_eq!(target.kind, TargetKind::Reference);
        assert_eq!(target.name, "shade");
        assert_eq!(at, tokens.size());
    }

    #[test]
    fn a_quoted_keyword_is_a_value() {
        let tokens = must(tokenize("GDAL \"FROM\" FROM"));
        assert!(!tokens.is(1, "FROM"));
        assert!(tokens.is(2, "FROM"));
        assert!(tokenize("GDAL \"open").is_err());
    }
}
