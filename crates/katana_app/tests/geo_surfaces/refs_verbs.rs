//! tests/geo/test_refs_verbs.cpp: the reference layers managed and kept
//! (refs_verb.rs; interop's reference_data; docs/interop.md, "Reference
//! layers"): REFS shows, hides, restyles, renames and removes a layer by its
//! id or name, builds overviews only when told to, and a saved project reads
//! its layers again when it is opened - through the executor, and through a
//! Session as katana_cli and katana_mcp run it. Its RefsSession case that
//! changes the working folder is in tests/geo_surfaces_process.
//!
//! The samples' facts, by hand: samples/gis/terrain.asc is 120 x 90 cells of
//! 1.5 from (-5, -5); survey_scan.las holds 40 000 points (`pdal info`);
//! tests/archive12d/data/las_point_cloud.12da two clouds, SITE SCAN of 154
//! points and ROAD SCAN of 24 (its las_cloud_data strings).

use std::cell::Cell;
use std::rc::Rc;

use crate::geo_fixture::{GeoFixture, must};
use crate::surface_support::{quoted, samples};

/// The C++ fixture's: its scratch folder named after the test, and the
/// front end's `changed` counting what it is told.
fn fixture(name: &str) -> (GeoFixture, Rc<Cell<i32>>) {
    let mut fixture = GeoFixture::new("katana refs verbs ", name);
    let changes = Rc::new(Cell::new(0));
    let counter = Rc::clone(&changes);
    fixture.front_end.changed = Some(Box::new(move || counter.set(counter.get() + 1)));
    (fixture, changes)
}

#[track_caller]
fn import_terrain_and_scan(fixture: &mut GeoFixture) {
    must(fixture.run(&format!(
        "IMPORT {}",
        quoted(&format!("{}/terrain.asc", samples()))
    )));
    must(fixture.run(&format!(
        "IMPORT {}",
        quoted(&format!("{}/survey_scan.las", samples()))
    )));
}

mod refs_verbs {
    use std::path::Path;

    use katana_app::geo::references::record_references;
    use katana_core::ErrorCode;
    use katana_core::json::Json;
    use katana_interop::reference_data::{parse_reference_record, reference_records};
    use katana_interop::{PointColorMode, RasterRole, ReferenceSourceKind};

    use super::{fixture, import_terrain_and_scan};
    use crate::geo_fixture::{holds, must, refused, shared_data};
    use crate::surface_support::{code, data, field, normal, quoted, records_of, samples};

    #[test]
    fn refs_hide_then_show_round_trips_visibility() {
        let _shared = shared_data();
        let (mut fixture, changes) = fixture("RefsHideThenShowRoundTripsVisibility");
        import_terrain_and_scan(&mut fixture);
        let before = changes.get();
        let hidden = must(fixture.run("REFS HIDE 1"));
        assert!(!fixture.reference.find_raster(1).expect("raster 1").visible);
        assert_eq!(field(&records_of(&hidden, "reference")[0], "visible"), "no");
        // By name, in any case, the same layer.
        let shown = must(fixture.run("REFS SHOW Terrain"));
        assert!(fixture.reference.find_raster(1).expect("raster 1").visible);
        assert_eq!(field(&records_of(&shown, "reference")[0], "visible"), "yes");
        // Each change redraws: the front end is told.
        assert_eq!(changes.get(), before + 2);

        must(fixture.run("REFS HIDE 2"));
        assert!(
            !fixture
                .reference
                .find_point_cloud(2)
                .expect("cloud 2")
                .visible
        );
    }

    #[test]
    fn opacity_outside_zero_to_one_is_refused() {
        let _shared = shared_data();
        let (mut fixture, _changes) = fixture("OpacityOutsideZeroToOneIsRefused");
        import_terrain_and_scan(&mut fixture);
        must(fixture.run("REFS OPACITY 1 0.5"));
        assert_eq!(
            fixture.reference.find_raster(1).expect("raster 1").opacity,
            0.5
        );
        for bad in ["1.5", "-0.1", "half", "nan"] {
            let refusal = refused(fixture.run(&format!("REFS OPACITY 1 {bad}")), bad);
            assert_eq!(refusal.code, ErrorCode::InvalidArgument, "{bad}");
        }
        assert_eq!(
            fixture.reference.find_raster(1).expect("raster 1").opacity,
            0.5,
            "a refusal changes nothing"
        );
        // A point cloud has no opacity; a raster no colouring.
        assert_eq!(
            code(fixture.run("REFS OPACITY 2 0.5")),
            ErrorCode::Unsupported
        );
        assert_eq!(
            code(fixture.run("REFS COLOR 1 intensity")),
            ErrorCode::Unsupported
        );
    }

    #[test]
    fn a_colour_rename_and_remove_are_by_id_or_name() {
        let _shared = shared_data();
        let (mut fixture, _changes) = fixture("AColourRenameAndRemoveAreByIdOrName");
        import_terrain_and_scan(&mut fixture);
        let coloured = must(fixture.run("REFS COLOUR survey_scan classification"));
        assert_eq!(
            fixture
                .reference
                .find_point_cloud(2)
                .expect("cloud 2")
                .color_mode,
            PointColorMode::Classification
        );
        assert_eq!(
            field(&records_of(&coloured, "reference")[0], "color"),
            "classification"
        );
        assert_eq!(
            code(fixture.run("REFS COLOR 2 purple")),
            ErrorCode::InvalidArgument
        );

        must(fixture.run("REFS RENAME 1 \"site dem\""));
        assert_eq!(
            fixture.reference.find_raster(1).expect("raster 1").name,
            "site dem"
        );

        let removed = must(fixture.run("REFS REMOVE \"site dem\""));
        assert_eq!(removed, "removed id=1 kind=raster name=\"site dem\"");
        assert!(fixture.reference.find_raster(1).is_none());
        assert_eq!(code(fixture.run("REFS REMOVE 1")), ErrorCode::NotFound);

        // Two layers of one name are named by their ids.
        let terrain = quoted(&format!("{}/terrain.asc", samples()));
        must(fixture.run(&format!("IMPORT {terrain}")));
        must(fixture.run(&format!("IMPORT {terrain}")));
        let ambiguous = refused(fixture.run("REFS HIDE terrain"), "a name two layers have");
        assert_eq!(ambiguous.code, ErrorCode::InvalidArgument);
        holds(&ambiguous.message, "3, 4");
    }

    #[test]
    fn refs_json_and_info_give_each_layers_facts() {
        let _shared = shared_data();
        let (mut fixture, _changes) = fixture("RefsJsonAndInfoGiveEachLayersFacts");
        import_terrain_and_scan(&mut fixture);
        let json = must(fixture.run("REFS JSON"));
        let parsed = Json::parse(&json).unwrap_or_else(|error| panic!("{error}: {json}"));
        assert_eq!(parsed["references"].size(), 2);
        assert_eq!(parsed["references"][0]["id"], Json::from(1));
        assert_eq!(parsed["references"][0]["width"], Json::from(120));
        assert_eq!(parsed["references"][1]["points"], Json::from(40000));
        assert!(parsed["missing"].empty());

        let info = must(fixture.run("REFS INFO 1"));
        let source = records_of(&info, "source");
        assert_eq!(source.len(), 1, "{info}");
        assert_eq!(
            field(&source[0], "file"),
            format!("{}/terrain.asc", samples())
        );
        holds(&field(&source[0], "crs"), "EPSG:32630");
    }

    #[test]
    fn overviews_need_confirm() {
        // A copy: the overviews are written beside the file.
        let (mut fixture, _changes) = fixture("OverviewsNeedConfirm");
        let scratch = fixture.scratch.path().to_path_buf();
        std::fs::copy(
            format!("{}/terrain.asc", samples()),
            scratch.join("terrain.asc"),
        )
        .expect("the sample terrain copied");
        std::fs::copy(
            format!("{}/terrain.prj", samples()),
            scratch.join("terrain.prj"),
        )
        .expect("the sample terrain's system copied");
        let copy = fixture.scratch.file("terrain.asc");
        must(fixture.run(&format!("IMPORT {}", quoted(&copy))));
        let ovr = scratch.join("terrain.asc.ovr");

        let refusal = refused(
            fixture.run("REFS OVERVIEWS 1 levels=2,4"),
            "overviews without CONFIRM",
        );
        assert_eq!(refusal.code, ErrorCode::Unsupported);
        holds(&refusal.message, "add CONFIRM");
        assert!(!ovr.exists(), "refused, nothing written");

        // 120 x 90 halved is 60 x 45; quartered 30 x 22.5, which GDAL rounds
        // up to 23 (an overview covers every pixel of the full image).
        let built = must(fixture.run("REFS OVERVIEWS 1 levels=2,4 CONFIRM"));
        assert!(ovr.exists());
        let head = records_of(&built, "overviews");
        assert_eq!(head.len(), 1, "{built}");
        assert_eq!(field(&head[0], "count"), "2");
        let levels = records_of(&built, "overview");
        assert_eq!(levels.len(), 2);
        assert_eq!(field(&levels[0], "width"), "60");
        assert_eq!(field(&levels[0], "height"), "45");
        assert_eq!(field(&levels[1], "width"), "30");
        assert_eq!(field(&levels[1], "height"), "23");
        // The source itself is untouched: external overviews, and no sidecar.
        assert!(!scratch.join("terrain.asc.aux.xml").exists());
    }

    #[test]
    fn a_record_round_trips_every_display_setting() {
        let _shared = shared_data();
        let (mut fixture, _changes) = fixture("ARecordRoundTripsEveryDisplaySetting");
        import_terrain_and_scan(&mut fixture);
        must(fixture.run("REFS OPACITY 1 0.25"));
        must(fixture.run("REFS HIDE 1"));
        must(fixture.run("REFS RENAME 1 dem"));
        must(fixture.run("REFS COLOR 2 rgb"));
        let records = reference_records(&fixture.reference, None);
        assert_eq!(records.len(), 2);
        for record in &records {
            assert!(!record.contains('\n'), "one line: {record}");
        }
        let raster = must(parse_reference_record(&records[0]));
        assert_eq!(raster.kind, ReferenceSourceKind::Raster);
        assert_eq!(raster.name, "dem");
        assert_eq!(
            raster.source,
            normal(Path::new(&format!("{}/terrain.asc", samples())))
        );
        assert!(!raster.visible);
        assert_eq!(raster.opacity, 0.25);
        assert_eq!(raster.max_pixels, 120);
        let cloud = must(parse_reference_record(&records[1]));
        assert_eq!(cloud.kind, ReferenceSourceKind::PointCloud);
        assert_eq!(cloud.color_mode, PointColorMode::SourceColor);
        assert_eq!(cloud.budget, 40000);

        // A newer record is refused by name, not half read.
        let newer = refused(
            parse_reference_record(r#"{"version":2,"kind":"raster","name":"x","source":"x.tif"}"#),
            "a record of a newer version",
        );
        holds(&newer.message, "newer");
    }

    #[test]
    fn a_derived_raster_persists_its_derivation() {
        // A raster a GDAL line made keeps the line: recorded with the
        // project, and read again with it.
        let _shared = shared_data();
        let (mut fixture, _changes) = fixture("ADerivedRasterPersistsItsDerivation");
        let line = format!(
            "GDAL raster hillshade FROM FILE {} TO REFERENCE shade",
            quoted(&format!("{}/plane.asc", data()))
        );
        must(fixture.run(&line));
        record_references(&mut fixture.context());
        assert_eq!(fixture.document.metadata().reference_layers.len(), 1);
        let recorded = must(parse_reference_record(
            &fixture.document.metadata().reference_layers[0],
        ));
        assert_eq!(recorded.role, RasterRole::Derived);
        assert_eq!(recorded.derivation, line);

        fixture.reference.clear();
        must(fixture.run("REFS RESTORE"));
        assert_eq!(fixture.reference.rasters().len(), 1);
        assert_eq!(fixture.reference.rasters()[0].name, "shade");
        assert_eq!(fixture.reference.rasters()[0].role, RasterRole::Derived);
        assert_eq!(fixture.reference.rasters()[0].derivation, line);
        let info = must(fixture.run("REFS INFO shade"));
        assert_eq!(field(&records_of(&info, "derivation")[0], "line"), line);
    }

    #[test]
    fn a_missing_source_is_warned_not_fatal_and_kept_for_the_next_save() {
        let _shared = shared_data();
        let (mut fixture, _changes) =
            fixture("AMissingSourceIsWarnedNotFatalAndKeptForTheNextSave");
        let gone = fixture.scratch.path().join("gone.asc");
        std::fs::copy(format!("{}/terrain.asc", samples()), &gone)
            .expect("the sample terrain copied");
        let gone_text = fixture.scratch.file("gone.asc");
        must(fixture.run(&format!("IMPORT {}", quoted(&gone_text))));
        must(fixture.run(&format!(
            "IMPORT {}",
            quoted(&format!("{}/survey_scan.las", samples()))
        )));
        record_references(&mut fixture.context());
        let recorded = fixture.document.metadata().reference_layers.clone();
        assert_eq!(recorded.len(), 2);
        std::fs::remove_file(&gone).expect("the copy removed");

        let restored = must(fixture.run("REFS RESTORE"));
        let warnings = records_of(&restored, "warning");
        assert_eq!(warnings.len(), 1, "{restored}");
        holds(&field(&warnings[0], "text"), "is gone");
        assert_eq!(field(&records_of(&restored, "restored")[0], "layers"), "1");
        assert_eq!(field(&records_of(&restored, "restored")[0], "missing"), "1");
        assert!(fixture.reference.rasters().is_empty());
        assert_eq!(fixture.reference.point_clouds().len(), 1);

        // Listed as missing, and written back as it was by the next save: the
        // drive may simply not be there today.
        let listed = must(fixture.run("REFS"));
        assert_eq!(records_of(&listed, "missing").len(), 1);
        assert_eq!(field(&records_of(&listed, "missing")[0], "name"), "gone");
        record_references(&mut fixture.context());
        assert_eq!(fixture.document.metadata().reference_layers.len(), 2);
        assert_eq!(
            fixture.document.metadata().reference_layers.last(),
            recorded.first()
        );
        // Until it is removed by name.
        must(fixture.run("REFS REMOVE gone"));
        record_references(&mut fixture.context());
        assert_eq!(fixture.document.metadata().reference_layers.len(), 1);
    }

    #[test]
    fn a_cloud_from_an_archive_is_read_again_from_the_archive() {
        let (mut fixture, _changes) = fixture("ACloudFromAnArchiveIsReadAgainFromTheArchive");
        let archives = format!("{}/../../tests/archive12d/data", samples());
        must(fixture.run(&format!(
            "IMPORT {}",
            quoted(&format!("{archives}/las_point_cloud.12da"))
        )));
        must(fixture.run("REFS COLOR \"ROAD SCAN\" flat"));
        record_references(&mut fixture.context());
        fixture.reference.clear();
        must(fixture.run("REFS RESTORE"));
        let clouds = fixture.reference.point_clouds();
        assert_eq!(clouds.len(), 2);
        assert_eq!(clouds[0].name, "SITE SCAN");
        assert_eq!(clouds[0].points.len(), 154);
        assert_eq!(clouds[1].name, "ROAD SCAN");
        assert_eq!(clouds[1].points.len(), 24);
        assert_eq!(clouds[1].color_mode, PointColorMode::Flat);
        assert!(
            fixture.document.model().entities.empty(),
            "only the clouds are taken from it"
        );
    }
}

// ---- through a Session, saved and opened, as katana_cli and katana_mcp run it -------------------

mod refs_session {
    use katana_app::{Captured, Session};

    use crate::geo_fixture::{TempDir, generic, holds, shared_data};
    use crate::surface_support::{field, quoted, records_of, samples};

    #[test]
    fn a_saved_project_reopens_with_its_reference_rasters() {
        let _shared = shared_data();
        let scratch = TempDir::new("katana refs verbs ", "session");
        let project = scratch.file("site.katana");
        let listed_first;
        {
            let mut session = Session::new(None, &mut Captured::default());
            let mut captured = Captured::default();
            assert!(session.run("RECT 0,0 10,5", &mut captured));
            assert!(session.run(
                format!("IMPORT {}", quoted(&format!("{}/terrain.asc", samples()))),
                &mut captured
            ));
            assert!(session.run(
                format!(
                    "IMPORT {}",
                    quoted(&format!("{}/survey_scan.las", samples()))
                ),
                &mut captured
            ));
            assert!(session.run("REFS OPACITY 1 0.75", &mut captured));
            assert!(
                session.run(format!("SAVE {}", quoted(&project)), &mut captured),
                "{}",
                captured.err
            );
            let mark = captured.out.len();
            assert!(session.run("REFS", &mut captured));
            listed_first = String::from(&captured.out[mark..]);
        }
        let mut reopened = Session::new(None, &mut Captured::default());
        let mut captured = Captured::default();
        assert!(
            reopened.run(format!("OPEN {}", quoted(&project)), &mut captured),
            "{}",
            captured.err
        );
        assert!(reopened.run("REFS", &mut captured));
        let before = records_of(&listed_first, "reference");
        let after = records_of(&captured.out, "reference");
        assert_eq!(before.len(), 2, "{listed_first}");
        // The OPEN's restore and the REFS after it each list both layers.
        assert_eq!(after.len(), 4, "{}", captured.out);
        for (k, first) in before.iter().enumerate() {
            for key in ["name", "kind", "file", "visible"] {
                assert_eq!(field(&after[2 + k], key), field(first, key), "{key}");
            }
        }
        assert_eq!(field(&after[2], "opacity"), "0.75");
        assert_eq!(field(&after[3], "points"), "40000");
        assert_eq!(captured.err, "");

        // NEW lets them go with the drawing.
        assert!(reopened.run("NEW", &mut captured));
        assert!(reopened.run("REFS", &mut captured));
        holds(&captured.out, "references rasters=0 clouds=0 missing=0");
    }

    #[test]
    fn a_layer_inside_the_project_follows_the_project_when_it_moves() {
        // The data kept in the project's own folder, and the folder moved: the
        // absolute source is gone, and the layer is found where the record
        // places it inside the project.
        let scratch = TempDir::new("katana refs verbs ", "moved");
        let first = scratch.path().join("first.katana");
        let moved = scratch.path().join("moved.katana");
        {
            let mut session = Session::new(None, &mut Captured::default());
            let mut captured = Captured::default();
            assert!(
                session.run(format!("SAVE {}", quoted(&generic(&first))), &mut captured),
                "{}",
                captured.err
            );
            std::fs::create_dir_all(first.join("data")).expect("the project's data folder");
            std::fs::copy(
                format!("{}/terrain.asc", samples()),
                first.join("data").join("terrain.asc"),
            )
            .expect("the sample terrain copied");
            assert!(
                session.run(
                    format!(
                        "IMPORT {}",
                        quoted(&generic(&first.join("data").join("terrain.asc")))
                    ),
                    &mut captured
                ),
                "{}",
                captured.err
            );
            assert!(session.run("SAVE", &mut captured), "{}", captured.err);
        }
        std::fs::rename(&first, &moved).expect("the project moved");
        let mut reopened = Session::new(None, &mut Captured::default());
        let mut captured = Captured::default();
        assert!(
            reopened.run(format!("OPEN {}", quoted(&generic(&moved))), &mut captured),
            "{}",
            captured.err
        );
        let restored = records_of(&captured.out, "restored");
        assert_eq!(restored.len(), 1, "{}", captured.out);
        assert_eq!(field(&restored[0], "layers"), "1", "{}", captured.out);
        let listed = records_of(&captured.out, "reference");
        assert!(!listed.is_empty(), "{}", captured.out);
        holds(&field(&listed[0], "file"), "moved.katana/data/terrain.asc");
    }
}
