//! The twins of tests/qt_widgets/geo/test_overlay_dialog.cpp: GIS > Analysis - GDAL > Overlay...
//! (geo::overlay_dialog): the line its two scopes, or its scope and a file, make, and a run
//! through the window's executor. The verb itself is tested in katana_app's geo twins. Expected
//! values are the C++'s.

use katana_window::geo::gis_tool_dialog::GisTool;
use katana_window::geo::overlay_dialog::{GisOverlayDialog, GisOverlayForm, gis_overlay_line};

use super::support::{Bench, WindowHost, window_context};

fn line_of(made: katana_core::Result<String>) -> String {
    made.unwrap_or_else(|error| panic!("{}", error.describe()))
}

mod gis_overlay_line {
    use super::*;

    #[test]
    fn two_scopes_are_joined_by_with() {
        let mut form = GisOverlayForm::default();
        form.subject.words = String::from("LAYERS lots");
        form.with.words = String::from("LAYERS corridor");
        assert_eq!(
            line_of(gis_overlay_line(&form)),
            "GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor"
        );
        form.operation = String::from("difference");
        form.keep = String::from("owner");
        form.keep_with = String::from("none");
        form.layer = String::from("net area");
        form.csv = String::from("C:/out/rows.csv");
        form.overwrite = true;
        assert_eq!(
            line_of(gis_overlay_line(&form)),
            "GIS OVERLAY difference LAYERS lots WITH LAYERS corridor keep=owner keepwith=none \
             csv=C:/out/rows.csv TO LAYER \"net area\" OVERWRITE"
        );
    }

    #[test]
    fn a_file_takes_its_layer_and_where() {
        let mut form = GisOverlayForm::default();
        form.subject.words = String::from("DRAWING");
        form.with_file = true;
        // No file yet.
        assert!(gis_overlay_line(&form).is_err());
        form.file = String::from("C:/data/zones.gpkg");
        form.file_layer = String::from("flood");
        form.file_where = String::from("depth > 0.5");
        assert_eq!(
            line_of(gis_overlay_line(&form)),
            "GIS OVERLAY intersection DRAWING WITH FILE C:/data/zones.gpkg LAYER flood \
             \"where=depth > 0.5\""
        );
    }
}

mod gis_overlay_dialog {
    use super::*;

    // Two 50 x 40 lots and a 4 m corridor across both: 200 m2 on each.
    #[test]
    fn every_field_has_its_object_name_and_the_two_scopes_make_the_line() {
        let mut host = WindowHost::new("katana-gis-overlay-dialog-test");
        for line in [
            "LAYER NEW lots",
            "LAYER SET lots",
            "RECT 0,0 50,40",
            "RECT 50,0 100,40",
            "LAYER NEW corridor",
            "LAYER SET corridor",
            "RECT -10,18 110,22",
        ] {
            host.type_line(line);
        }
        let mut dialog = GisOverlayDialog::new(window_context(), &host);
        dialog.show(&host);
        let mut bench = Bench::new("gisOverlayDialog", dialog, host);
        for name in [
            "gisOverlayScope",
            "gisOverlayWithScope",
            "gisOverlayOperation",
            "gisOverlayWithSourceDrawing",
            "gisOverlayWithSourceFile",
            "gisOverlayWithFile",
            "gisOverlayWithFileBrowse",
            "gisOverlayWithFileLayer",
            "gisOverlayWithFileWhere",
            "gisOverlayKeep",
            "gisOverlayKeepWith",
            "gisOverlayLayer",
            "gisOverlayCsv",
            "gisOverlayOverwrite",
            "gisOverlayCommand",
            "gisOverlayPreview",
            "gisOverlayRun",
            "gisOverlayReply",
        ] {
            assert!(bench.has(name), "{name}");
        }
        bench.press("gisOverlayScopeLayers");
        // Each list holds the layer once, and its box is ticked: the scope's own setter is the
        // tick the C++ made on the list's item.
        for (list, layer) in [
            ("gisOverlayLayers", "lots"),
            ("gisOverlayWithLayers", "corridor"),
        ] {
            let listed = bench
                .control(list)
                .items
                .iter()
                .filter(|item| item.as_str() == layer)
                .count();
            assert_eq!(listed, 1, "{layer}");
        }
        bench
            .dialog
            .frame_mut()
            .scope_controls_mut()
            .expect("a scope")
            .tick_layer("lots", true);
        bench.dialog.with_controls().tick_layer("corridor", true);
        bench.draw();
        assert_eq!(
            bench.text("gisOverlayCommand"),
            "GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor"
        );
        bench.dialog.run(&mut bench.host);
        assert!(
            bench
                .dialog
                .reply_text()
                .contains("overlay operation=intersection features=2 area=400.000"),
            "{}",
            bench.dialog.reply_text()
        );
        // A file instead: the second scope is set aside.
        bench.press("gisOverlayWithSourceFile");
        assert!(!bench.enabled("gisOverlayWithScope"));
        bench.fill("gisOverlayWithFile", "zones.gpkg");
        assert_eq!(
            bench.text("gisOverlayCommand"),
            "GIS OVERLAY intersection LAYERS lots WITH FILE zones.gpkg"
        );
    }
}
