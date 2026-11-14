//! The twins of tests/qt_widgets/geo/test_export_dialog.cpp: Export Drawing's dialog
//! (gis_export_dialog): its fields make the EXPORT line a person would type - the shared scope
//! first, then EXPORT's options - and Run and Preview hand that line to the window's executor.
//! The verb itself is tested in katana_app's geo twins. Expected values are the C++'s.

use katana_window::geo::gis_tool_dialog::{GisDialogContext, GisTool};
use katana_window::gis_export_dialog::{
    VectorExportDialog, VectorExportForm, export_is_native, vector_export_line,
};
use katana_window::host::{Answer, HostParts, RecordingHost, VerbOutcome};

use super::support::{Bench, runner_context};

fn drawing_form(file: &str) -> VectorExportForm {
    let mut form = VectorExportForm::default();
    form.file = String::from(file);
    form.scope.words = String::from("DRAWING");
    form
}

fn line_of(form: &VectorExportForm) -> String {
    vector_export_line(form).unwrap_or_else(|error| panic!("{}", error.describe()))
}

mod vector_export_line {
    use super::*;

    #[test]
    fn the_options_follow_the_scope_and_defaults_are_left_out() {
        let mut form = drawing_form("C:/out/site lots.gpkg");
        assert_eq!(line_of(&form), "EXPORT \"C:/out/site lots.gpkg\" DRAWING");
        form.layer_name = String::from("parcels");
        form.append = true;
        form.crs = String::from("EPSG:4326");
        form.creation_options = String::from("VERSION=1.4");
        form.layer_options = String::from("SPATIAL_INDEX=NO  FID=fid");
        form.text_as_points = true;
        form.curve = String::from("0.01");
        form.properties = false;
        assert_eq!(
            line_of(&form),
            "EXPORT \"C:/out/site lots.gpkg\" DRAWING layername=parcels append crs=EPSG:4326 \
             co=VERSION=1.4 lco=SPATIAL_INDEX=NO lco=FID=fid text=points curve=0.01 \
             properties=no"
        );
        // project is the verb's default, and not said.
        let mut form = drawing_form("C:/out/a.gpkg");
        form.crs = String::from("project");
        assert_eq!(line_of(&form), "EXPORT \"C:/out/a.gpkg\" DRAWING");
    }

    #[test]
    fn what_a_line_cannot_say_has_no_line() {
        let mut form = drawing_form("C:/out/a.gpkg");
        form.split = true;
        form.layer_name = String::from("one");
        // Split names each layer itself.
        assert!(vector_export_line(&form).is_err());
        let mut form = drawing_form("C:/out/a.gpkg");
        form.creation_options = String::from("VERSION");
        // Not KEY=VALUE.
        assert!(vector_export_line(&form).is_err());
        let mut form = drawing_form("C:/out/a.gpkg");
        form.curve = String::from("0");
        // A chord's stray is above 0.
        assert!(vector_export_line(&form).is_err());
        // No file.
        assert!(vector_export_line(&drawing_form("")).is_err());
        let mut form = drawing_form("C:/out/a.gpkg");
        form.scope.words.clear();
        // No scope.
        assert!(vector_export_line(&form).is_err());
    }

    #[test]
    fn a_dxf_takes_the_scope_alone() {
        let mut form = drawing_form("C:/out/site.dxf");
        form.layer_name = String::from("parcels");
        form.text_as_points = true;
        assert_eq!(line_of(&form), "EXPORT \"C:/out/site.dxf\" DRAWING");
        assert!(export_is_native("C:/out/site.12da"));
        assert!(!export_is_native("C:/out/site.gpkg"));
    }
}

mod vector_export_dialog {
    use super::*;

    #[test]
    fn the_fields_build_exactly_the_typed_line_and_run_hands_it_over() {
        let host = RecordingHost::new().answering(Answer::Custom(Box::new(
            |_parts: &mut HostParts, line: &str| {
                let reply = if line.ends_with(" PREVIEW") {
                    "export file=\"C:/out/site.gpkg\" kind=vector driver=GPKG preview=yes \
                     entities=4"
                } else {
                    "exported file=\"C:/out/site.gpkg\" kind=vector driver=GPKG features=4 \
                     skipped=0 layers=parcels crs="
                };
                VerbOutcome::new(true, reply, "")
            },
        )));
        let mut dialog = VectorExportDialog::new(runner_context(), &host);
        assert_eq!(dialog.object_name(), "vectorExportDialog");
        // File > Export calls its item "Export Drawing...", and the window it opens says the
        // same.
        assert_eq!(dialog.window_title(), "Export Drawing");
        dialog.set_file("C:/out/site.gpkg");
        dialog.show(&host);
        let mut bench = Bench::new("vectorExportDialog", dialog, host);
        // The whole drawing is where an export starts.
        assert_eq!(
            bench.text("vectorExportCommand"),
            "EXPORT \"C:/out/site.gpkg\" DRAWING"
        );
        bench.fill("vectorExportLayerName", "parcels");
        bench.fill("vectorExportCrs", "native");
        bench.fill("vectorExportLayerOptions", "SPATIAL_INDEX=NO");
        // The choice whose word is "points".
        bench.fill(
            "vectorExportText",
            "Text as points with its text, height and angle",
        );
        bench.tick("vectorExportProperties", false);
        let expected = "EXPORT \"C:/out/site.gpkg\" DRAWING layername=parcels crs=native \
                        lco=SPATIAL_INDEX=NO text=points properties=no";
        assert_eq!(bench.text("vectorExportCommand"), expected);

        bench.press("vectorExportPreview");
        bench.press("vectorExportRun");
        let ran = bench.host.lines();
        assert_eq!(ran.len(), 2);
        assert_eq!(ran[0], format!("{expected} PREVIEW"));
        assert_eq!(ran[1], expected);
        assert_eq!(
            bench.dialog.status_text(),
            "Wrote 4 features to C:/out/site.gpkg."
        );
    }

    #[test]
    fn a_dxf_turns_the_gdal_fields_off() {
        let host = RecordingHost::new();
        let mut dialog = VectorExportDialog::new(GisDialogContext::default(), &host);
        dialog.set_file("C:/out/site.dxf");
        dialog.show(&host);
        let mut bench = Bench::new("vectorExportDialog", dialog, host);
        assert!(!bench.enabled("vectorExportCrs"));
        assert!(!bench.enabled("vectorExportSplit"));
        assert_eq!(
            bench.text("vectorExportCommand"),
            "EXPORT \"C:/out/site.dxf\" DRAWING"
        );
        bench.dialog.set_file("C:/out/site.gpkg");
        bench.draw();
        assert!(bench.enabled("vectorExportCrs"));
        // One file layer per drawing layer names each: the layer name goes off.
        bench.tick("vectorExportSplit", true);
        assert!(!bench.enabled("vectorExportLayerName"));
        assert_eq!(
            bench.text("vectorExportCommand"),
            "EXPORT \"C:/out/site.gpkg\" DRAWING split=layer"
        );
    }
}
