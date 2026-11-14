//! The twins of tests/qt_widgets/geo/test_hull_and_clip_dialogs.cpp: GIS > Analysis - GDAL >
//! Boundary Around Features... and Clip to Boundary... (geo::hull_dialog, geo::clip_dialog): the
//! line each builds, and a clip through the window's executor. The verbs themselves are tested in
//! katana_app's geo twins. Expected values are the C++'s.

use katana_entity::Geometry;
use katana_window::customisation::ScopeChoice;
use katana_window::geo::clip_dialog::{GisClipDialog, GisClipForm, gis_clip_line};
use katana_window::geo::gis_tool_dialog::GisTool;
use katana_window::geo::hull_dialog::{GisHullDialog, GisHullForm, gis_hull_line};
use katana_window::host::{Host, VerbOutcome};

use super::support::{Bench, WindowHost, fixed, runner_context, window_context};

fn line_of(made: katana_core::Result<String>) -> String {
    made.unwrap_or_else(|error| panic!("{}", error.describe()))
}

mod gis_hull_line {
    use super::*;

    #[test]
    fn convex_is_the_default_and_concave_takes_its_ratio() {
        let mut form = GisHullForm::default();
        form.scope.words = String::from("LAYERS survey");
        assert_eq!(line_of(gis_hull_line(&form)), "GIS HULL LAYERS survey");
        form.concave = true;
        form.ratio = String::from("0.1");
        form.holes = true;
        form.layer = String::from("extent");
        assert_eq!(
            line_of(gis_hull_line(&form)),
            "GIS HULL LAYERS survey concave=0.1 holes TO LAYER extent"
        );
        form.ratio = String::from("1.5");
        assert!(gis_hull_line(&form).is_err());
    }
}

mod gis_clip_line {
    use super::*;

    #[test]
    fn a_boundary_is_a_scope_or_a_file_and_replace_drops_the_layer() {
        let mut form = GisClipForm::default();
        form.scope.words = String::from("LAYERS pipes");
        form.by.words = String::from("LAYERS site");
        assert_eq!(
            line_of(gis_clip_line(&form)),
            "GIS CLIP LAYERS pipes BY LAYERS site"
        );
        form.layer = String::from("cut");
        form.replace = true;
        assert_eq!(
            line_of(gis_clip_line(&form)),
            "GIS CLIP LAYERS pipes BY LAYERS site REPLACE"
        );
        form.by_file = true;
        form.replace = false;
        // No file yet.
        assert!(gis_clip_line(&form).is_err());
        form.file = String::from("C:/data/site.gpkg");
        form.file_where = String::from("kind = 'site'");
        assert_eq!(
            line_of(gis_clip_line(&form)),
            "GIS CLIP LAYERS pipes BY FILE C:/data/site.gpkg \"where=kind = 'site'\" TO LAYER cut"
        );
    }
}

mod gis_hull_dialog {
    use super::*;

    #[test]
    fn every_field_has_its_object_name_and_the_kind_chooses_the_ratio() {
        let host = fixed(VerbOutcome::default());
        let mut dialog = GisHullDialog::new(runner_context(), &host);
        dialog.show(&host);
        let mut bench = Bench::new("gisHullDialog", dialog, host);
        for name in [
            "gisHullScope",
            "gisHullKind",
            "gisHullRatio",
            "gisHullHoles",
            "gisHullLayer",
            "gisHullCommand",
            "gisHullRun",
            "gisHullReply",
        ] {
            assert!(bench.has(name), "{name}");
        }
        assert!(!bench.enabled("gisHullRatio"));
        bench.press("gisHullScopeDrawing");
        bench.fill("gisHullKind", "concave");
        assert!(bench.enabled("gisHullRatio"));
        assert_eq!(bench.text("gisHullCommand"), "GIS HULL DRAWING concave=0.1");
    }
}

mod gis_clip_dialog {
    use super::*;

    // A 100 m pipe through a site from x = 10 to 40: 30 m of it stays, on the pipe's own id.
    #[test]
    fn its_line_cuts_a_pipe_in_place_through_the_executor() {
        let mut host = WindowHost::new("katana-gis-clip-dialog-test");
        for line in [
            "LAYER NEW pipes",
            "LAYER SET pipes",
            "LINE 0,0 100,0",
            "LAYER NEW site",
            "LAYER SET site",
            "RECT 10,-5 40,5",
        ] {
            host.type_line(line);
        }
        let mut dialog = GisClipDialog::new(window_context(), &host);
        dialog.show(&host);
        let mut bench = Bench::new("gisClipDialog", dialog, host);
        for name in [
            "gisClipScope",
            "gisClipByScope",
            "gisClipBySourceDrawing",
            "gisClipBySourceFile",
            "gisClipByFile",
            "gisClipByFileBrowse",
            "gisClipByFileLayer",
            "gisClipByFileWhere",
            "gisClipReplace",
            "gisClipLayer",
            "gisClipCommand",
            "gisClipRun",
            "gisClipReply",
        ] {
            assert!(bench.has(name), "{name}");
        }
        bench.press("gisClipScopeLayers");
        // The lists hold each layer once, and its box is ticked (the C++ set the item's check
        // state): the scope's own setter is that tick.
        for (list, layer) in [("gisClipLayers", "pipes"), ("gisClipByLayers", "site")] {
            let listed = bench
                .control(list)
                .items
                .iter()
                .filter(|item| item.as_str() == layer)
                .count();
            assert_eq!(listed, 1, "{list}");
        }
        bench
            .dialog
            .frame_mut()
            .scope_controls_mut()
            .expect("a scope")
            .tick_layer("pipes", true);
        assert_eq!(bench.dialog.by_controls().choice(), ScopeChoice::Layers);
        bench.dialog.by_controls().tick_layer("site", true);
        bench.draw();
        bench.tick("gisClipReplace", true);
        assert!(!bench.enabled("gisClipLayer"));
        assert_eq!(
            bench.text("gisClipCommand"),
            "GIS CLIP LAYERS pipes BY LAYERS site REPLACE"
        );
        bench.dialog.run(&mut bench.host);
        assert!(
            bench
                .dialog
                .reply_text()
                .contains("target=in-place created=0 updated=1"),
            "{}",
            bench.dialog.reply_text()
        );
        bench.draw();
        // An in-place reply names no layer: the status said "... deleted on .".
        assert_eq!(
            bench.text("gisClipStatus"),
            "Done as one undo step: 0 created, 1 changed, 0 deleted, in place."
        );
        let entities = &bench.host.document().model().entities;
        let pipe = entities.find(1).expect("the pipe keeps its id");
        match &pipe.geometry {
            Geometry::Line(segment) => {
                assert!((segment.length() - 30.0_f64).abs() <= 1e-9, "{}", segment.length());
            }
            _ => panic!("the pipe is not a line"),
        }
    }
}
