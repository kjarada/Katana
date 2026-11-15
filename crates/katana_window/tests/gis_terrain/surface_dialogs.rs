//! The twins of tests/qt_widgets/geo/test_surface_dialogs.cpp: Terrain > Surface From and GIS >
//! Export Surface as DEM (katana_window::surface_raster_dialog) - the lines the fields describe,
//! and each dialog driven by its object names, running exactly the line it shows.

use katana_window::host::RecordingHost;
use katana_window::surface_raster_dialog::{
    SurfaceExportForm, SurfaceFromDialog, SurfaceFromForm, SurfaceFromKind, SurfaceRasterDialog,
    surface_export_line, surface_from_line,
};

use super::support::{Bench, add_raster, add_surface, ran_host, tin_of};

/// The dialog on `kind`, as showKind brings it forward.
fn from_bench(mut host: RecordingHost, kind: SurfaceFromKind) -> Bench<SurfaceFromDialog> {
    let mut dialog = SurfaceFromDialog::new(&mut host);
    dialog.show(&host);
    dialog.show_kind(&host, kind, "");
    Bench::new("surfaceFromDialog", dialog, host)
}

fn message(form: &SurfaceFromForm) -> String {
    surface_from_line(form)
        .expect_err("the line is refused")
        .message
}

/// QDir::toNativeSeparators, as the C++ test wrote its path.
fn native(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        String::from(path)
    }
}

mod surface_from_line {
    use super::*;

    #[test]
    fn each_source_writes_its_own_form_and_leaves_blanks_out() {
        let cloud = SurfaceFromForm {
            kind: SurfaceFromKind::Cloud,
            source: String::from("CLOUD 2"),
            classes: String::from("2, 8"),
            ..SurfaceFromForm::default()
        };
        assert_eq!(
            surface_from_line(&cloud).unwrap(),
            "SURFACE FROM CLOUD 2 classes=2,8"
        );

        let raster = SurfaceFromForm {
            kind: SurfaceFromKind::Raster,
            source: String::from("RASTER 3"),
            max_points: String::from("100000"),
            area: String::from("0,0, 40,30"),
            name: String::from("site ground"),
            ..SurfaceFromForm::default()
        };
        assert_eq!(
            surface_from_line(&raster).unwrap(),
            "SURFACE FROM RASTER 3 max=100000 AREA 0,0,40,30 NAME \"site ground\""
        );

        let drawing = SurfaceFromForm {
            scope: String::from("LAYERS survey WHERE DRAWN"),
            ..SurfaceFromForm::default()
        };
        assert_eq!(
            surface_from_line(&drawing).unwrap(),
            "SURFACE FROM LAYERS survey WHERE DRAWN"
        );
    }

    #[test]
    fn what_cannot_be_written_is_refused_naming_the_field() {
        let mut raster = SurfaceFromForm {
            kind: SurfaceFromKind::Raster,
            ..SurfaceFromForm::default()
        };
        assert!(message(&raster).contains("Source"));
        raster.source = String::from("RASTER 1");
        raster.area = String::from("0,0,40");
        assert!(message(&raster).contains("Window"));
        raster.area.clear();
        raster.max_points = String::from("lots");
        assert!(message(&raster).contains("Most points"));

        let mut drawing = SurfaceFromForm {
            scope_error: String::from("no layer is ticked"),
            ..SurfaceFromForm::default()
        };
        assert!(message(&drawing).contains("Apply to"));
        drawing.scope_error.clear();
        drawing.name = String::from("a \"b\"");
        assert!(message(&drawing).contains("Name"));
    }
}

mod surface_from_dialog {
    use super::*;

    #[test]
    fn the_drawing_is_what_is_drawn_at_first_and_run_runs_the_line_shown() {
        let mut bench = from_bench(ran_host(), SurfaceFromKind::Drawing);
        assert!(bench.harness.target_record("surfaceFromDialog").is_some());
        assert_eq!(
            bench.text("surfaceFromCommand"),
            "SURFACE FROM DRAWING WHERE DRAWN"
        );
        assert!(bench.has("surfaceFromScope"));
        assert!(bench.has("surfaceFromDrawnOnly"));
        bench.fill("surfaceFromName", "ground");
        bench.press("surfaceFromRun");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(
            bench.host.lines()[0],
            "SURFACE FROM DRAWING WHERE DRAWN NAME ground"
        );
        assert_eq!(
            bench.text("surfaceFromReply"),
            "ran SURFACE FROM DRAWING WHERE DRAWN NAME ground"
        );
        bench.press("surfaceFromPreview");
        assert_eq!(
            bench.host.lines().last().unwrap(),
            "SURFACE FROM DRAWING WHERE DRAWN NAME ground PREVIEW"
        );
    }

    #[test]
    fn a_raster_source_is_chosen_by_its_id_and_the_window_is_written() {
        let mut host = ran_host();
        let id = add_raster(&mut host, "terrain", "terrain.asc");
        let mut bench = from_bench(host, SurfaceFromKind::Raster);
        assert_eq!(bench.items("surfaceFromSource").len(), 1);
        assert_eq!(bench.dialog.form().source, format!("RASTER {id}"));
        bench.fill("surfaceFromArea", "0,0,20,20");
        assert_eq!(
            bench.text("surfaceFromCommand"),
            format!("SURFACE FROM RASTER {id} AREA 0,0,20,20")
        );
        // The drawing's controls are a drawing's: off for a raster.
        assert!(!bench.enabled("surfaceFromScope"));
    }

    #[test]
    fn with_no_raster_run_is_off_and_the_command_says_why() {
        let bench = from_bench(ran_host(), SurfaceFromKind::Raster);
        assert!(!bench.enabled("surfaceFromRun"));
        assert!(
            bench
                .placeholder("surfaceFromCommand")
                .contains("no raster")
        );
        assert!(bench.host.lines().is_empty());
    }
}

// ---- Export Surface as DEM ----------------------------------------------------------------------

mod surface_export_line {
    use super::*;

    #[test]
    fn the_line_names_the_surface_the_file_and_the_options() {
        let mut form = SurfaceExportForm {
            surface: String::from("site ground"),
            file: native("C:/out/dem 1.tif"),
            cell: 0.5,
            cog: true,
            overwrite: true,
            ..SurfaceExportForm::default()
        };
        assert_eq!(
            surface_export_line(&form).unwrap(),
            "SURFACE EXPORT \"site ground\" \"C:/out/dem 1.tif\" cell=0.5 type=Float32 COG \
             OVERWRITE"
        );
        form.file.clear();
        assert!(
            surface_export_line(&form)
                .unwrap_err()
                .message
                .contains("File")
        );
        form.file = String::from("dem.tif");
        form.cell = 0.0;
        assert!(
            surface_export_line(&form)
                .unwrap_err()
                .message
                .contains("Cell")
        );
    }
}

mod surface_raster_dialog {
    use super::*;

    #[test]
    fn it_suggests_a_cell_for_the_surface_and_runs_the_export_line() {
        let mut host = ran_host();
        add_surface(
            &mut host,
            "ground",
            tin_of(&[
                (0.0, 0.0, 1.0),
                (100.0, 0.0, 2.0),
                (100.0, 50.0, 3.0),
                (0.0, 50.0, 2.0),
            ]),
        );
        let mut dialog = SurfaceRasterDialog::new(&host);
        dialog.show(&host);
        let mut bench = Bench::new("surfaceRasterDialog", dialog, host);
        assert!(bench.harness.target_record("surfaceRasterDialog").is_some());
        // A 100 m extent at about 1000 cells along its longer side: 0.1.
        assert_eq!(bench.dialog.cell_size(), 0.1);
        assert_eq!(bench.value("surfaceRasterSource"), "ground");
        assert!(!bench.enabled("surfaceRasterRun"), "no file yet");
        bench.fill("surfaceRasterFile", "dem.tif");
        bench.fill("surfaceRasterCell", "1");
        bench.fill("surfaceRasterType", "Float64");
        assert_eq!(bench.text("surfaceRasterGrid"), "100 x 50 cells");
        bench.press("surfaceRasterRun");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(
            bench.host.lines()[0],
            "SURFACE EXPORT ground dem.tif cell=1 type=Float64"
        );
    }
}
