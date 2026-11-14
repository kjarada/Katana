//! The twins of tests/qt_widgets/geo/test_gdal_toolbox.cpp: GIS > GDAL Toolbox
//! (katana_window::geo::gdal_toolbox_dialog) - the catalogue searched, each algorithm's form built
//! from what GDAL declares, and the line the fields describe, which Run hands the host exactly.

use katana_gis::processing as gp;
use katana_window::geo::gdal_toolbox_dialog::{
    GdalToolboxDialog, GdalToolboxForm, GdalToolboxInput, gdal_toolbox_line,
};
use katana_window::host::{Answer, RecordingHost, VerbOutcome};

use super::support::{Bench, add_raster, stub_host};

fn bench(mut host: RecordingHost) -> Bench<GdalToolboxDialog> {
    let mut dialog = GdalToolboxDialog::new(&mut host);
    dialog.show(&host);
    Bench::new("gdalToolboxDialog", dialog, host)
}

/// `choose(path)`, as a click on the algorithm's row.
fn choose(bench: &mut Bench<GdalToolboxDialog>, path: &str) {
    assert!(bench.dialog.choose(&mut bench.host, path), "{path}");
    bench.draw();
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| String::from(*word)).collect()
}

mod gdal_toolbox {
    use super::*;

    #[test]
    fn searching_contour_finds_raster_contour() {
        let mut bench = bench(stub_host("ok"));
        bench.fill("gdalToolboxSearch", "contour");
        // The algorithm named exactly so is chosen, and shown under its group.
        assert_eq!(bench.dialog.chosen(), "raster contour");
        let (current, parent) = bench.dialog.tree_current().expect("a row is current");
        assert_eq!(current, "contour");
        assert_eq!(bench.dialog.tree_hidden("raster contour"), Some(false));
        assert_eq!(parent.as_deref(), Some("raster"));
        // Something the search does not match is hidden.
        assert_eq!(bench.dialog.tree_hidden("raster hillshade"), Some(true));
    }

    #[test]
    fn hillshade_builds_its_form_with_bounds_and_choices() {
        let mut bench = bench(stub_host("ok"));
        choose(&mut bench, "raster hillshade");
        // GDAL's own bounds and choices, read at run time.
        let spec = gp::describe(&strings(&["raster", "hillshade"])).expect("GDAL has hillshade");
        assert!(bench.has("gdalArg.altitude"));
        let (_, maximum) = bench
            .dialog
            .arguments()
            .number_range("altitude")
            .expect("altitude is a number box");
        assert_eq!(maximum, 90.0);
        let mut choices = vec![String::new()];
        for arg in &spec.args {
            if arg.name == "variant" {
                choices.extend(arg.choices.iter().cloned());
            }
        }
        assert!(choices.len() > 1);
        assert_eq!(bench.items("gdalArg.variant"), choices);
        // Its default is chosen, and a default is no word on the line.
        assert_eq!(bench.value("gdalArg.variant"), "regular");
        // --quiet would print instead of running: the executor refuses it, so the form does not
        // offer it; the output's format is the target's to say.
        assert!(!bench.has("gdalArg.quiet"));
        assert!(!bench.has("gdalArg.output-format"));
        assert!(bench.has("gdalOutput.format"));
    }

    #[test]
    fn setting_zfactor_builds_the_exact_line() {
        let mut host = stub_host("ok");
        assert_eq!(add_raster(&mut host, "ground", "C:/data/ground.tif"), 1);
        let mut bench = bench(host);
        choose(&mut bench, "raster hillshade");
        bench.fill("gdalArg.zfactor", "2");
        assert_eq!(
            bench.text("gdalToolboxCommand"),
            "GDAL raster hillshade --zfactor=2 FROM RASTER 1 TO REFERENCE hillshade"
        );
    }

    #[test]
    fn a_raster_only_input_offers_no_drawing_source() {
        let mut bench = bench(stub_host("ok"));
        choose(&mut bench, "raster hillshade");
        let raster = bench.items("gdalInput.input.Kind");
        assert!(!raster.iter().any(|kind| kind == "Drawing"));
        for kind in ["Reference raster", "Surface", "File"] {
            assert!(raster.iter().any(|listed| listed == kind), "{kind}");
        }
        // And the other way round: features come from the drawing or a file.
        choose(&mut bench, "vector buffer");
        assert_eq!(bench.items("gdalInput.input.Kind"), ["Drawing", "File"]);
        // A raster result is not offered a layer, nor features a reference raster.
        assert!(
            !bench
                .items("gdalOutput.kind")
                .iter()
                .any(|kind| kind == "Reference raster")
        );
        choose(&mut bench, "raster hillshade");
        assert!(
            !bench
                .items("gdalOutput.kind")
                .iter()
                .any(|kind| kind == "Layer")
        );
    }

    #[test]
    fn an_exclusive_minimum_is_not_emitted() {
        // contour --interval must be above 0 (GDAL's minimum is exclusive): 0 is refused when the
        // line is written, and nothing can run.
        let mut bench = bench(stub_host("ok"));
        choose(&mut bench, "raster contour");
        bench.fill("gdalInput.input.Kind", "File");
        bench.fill("gdalInput.input.File", "C:/data/dem.tif");
        bench.fill("gdalArg.interval", "0");
        assert!(bench.text("gdalToolboxCommand").is_empty());
        let why = bench.placeholder("gdalToolboxCommand");
        assert!(why.contains("interval must be above 0"), "{why}");
        // Run is off, so nothing can be run: the harness refuses a press of it, where Qt's click
        // did nothing.
        assert!(!bench.enabled("gdalToolboxRun"));
        assert!(bench.host.lines().is_empty());
        bench.fill("gdalArg.interval", "0.5");
        assert_eq!(
            bench.text("gdalToolboxCommand"),
            "GDAL raster contour --interval=0.5 FROM FILE C:/data/dem.tif TO LAYER gis/contour"
        );
        // interval, levels and exp-base are one exclusion group: one given, the others are not
        // offered.
        assert!(!bench.enabled("gdalArg.levels"));
    }

    #[test]
    fn a_confirm_algorithm_shows_confirm_and_emits_it() {
        let mut bench = bench(stub_host("ok"));
        choose(&mut bench, "vector buffer");
        assert!(!bench.visible("gdalToolboxConfirm"));
        choose(&mut bench, "vsi delete");
        assert!(bench.visible("gdalToolboxConfirm"));
        bench.fill("gdalArg.filename", "C:/tmp/old.tif");
        // Until it is ticked there is no line to run.
        assert!(bench.text("gdalToolboxCommand").is_empty());
        assert!(bench.placeholder("gdalToolboxCommand").contains("Confirm"));
        bench.tick("gdalToolboxConfirm", true);
        assert_eq!(
            bench.text("gdalToolboxCommand"),
            "GDAL vsi delete --filename=C:/tmp/old.tif CONFIRM"
        );
    }

    #[test]
    fn run_hands_the_runner_exactly_the_shown_line() {
        let mut bench = bench(stub_host("ok"));
        choose(&mut bench, "vector buffer");
        bench.fill("gdalArg.distance", "1");
        bench.fill("gdalArg.endcap-style", "flat");
        let command = bench.text("gdalToolboxCommand");
        assert_eq!(
            command,
            "GDAL vector buffer --distance=1 --endcap-style=flat FROM DRAWING TO LAYER gis/buffer"
        );
        bench.press("gdalToolboxRun");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(bench.host.lines()[0], command);
        // Without distance, which buffer requires, there is nothing to run: the box back at its
        // least value, which says "not given".
        let (minimum, _) = bench
            .dialog
            .arguments()
            .number_range("distance")
            .expect("distance is a number box");
        assert!(bench.dialog.arguments().set_value(
            "distance",
            &katana_core::text::format_exact_real(minimum)
        ));
        bench.draw();
        assert!(
            bench
                .placeholder("gdalToolboxCommand")
                .contains("distance is required")
        );
    }

    #[test]
    fn a_preview_reply_is_shown() {
        let host = RecordingHost::new().answering(Answer::Fixed(VerbOutcome::new(
            true,
            "gdal algorithm=\"vector buffer\" policy=safe preview=yes\n\
             preview valid=yes changed=no",
            "",
        )));
        let mut bench = bench(host);
        choose(&mut bench, "vector buffer");
        bench.fill("gdalArg.distance", "5");
        bench.press("gdalToolboxPreview");
        assert_eq!(bench.host.lines().len(), 1);
        assert!(bench.host.lines()[0].ends_with(" PREVIEW"));
        assert!(
            bench
                .text("gdalToolboxReply")
                .contains("preview valid=yes changed=no")
        );
        assert!(bench.text("gdalToolboxStatus").starts_with("Previewed"));
    }

    #[test]
    fn an_optional_input_is_left_out_until_it_is_given_and_then_named() {
        let mut bench = bench(stub_host("ok"));
        choose(&mut bench, "raster clip");
        bench.fill("gdalInput.input.Kind", "File");
        bench.fill("gdalInput.input.File", "C:/data/dem.tif");
        assert_eq!(bench.value("gdalInput.like.Kind"), "Not given");
        assert_eq!(
            bench.text("gdalToolboxCommand"),
            "GDAL raster clip FROM FILE C:/data/dem.tif TO REFERENCE clip"
        );
        bench.fill("gdalInput.like.Kind", "Drawing");
        bench.press("gdalInput.like.ScopeDrawing");
        assert_eq!(
            bench.text("gdalToolboxCommand"),
            "GDAL raster clip FROM FILE C:/data/dem.tif FROM like DRAWING TO REFERENCE clip"
        );
    }

    #[test]
    fn several_datasets_for_one_argument_each_name_it() {
        let mut bench = bench(stub_host("ok"));
        choose(&mut bench, "raster mosaic");
        bench.fill("gdalInput.input.Kind", "File");
        bench.fill("gdalInput.input.File", "C:/tiles/a.tif");
        bench.press("gdalInput.input.Add");
        bench.fill("gdalInput.input.File", "C:/tiles/b.tif");
        bench.press("gdalInput.input.Add");
        assert_eq!(
            bench.text("gdalToolboxCommand"),
            "GDAL raster mosaic FROM input FILE C:/tiles/a.tif FROM input FILE C:/tiles/b.tif TO \
             REFERENCE mosaic"
        );
    }

    #[test]
    fn a_file_output_takes_its_format_and_overwrite() {
        let mut bench = bench(stub_host("ok"));
        choose(&mut bench, "vector buffer");
        bench.fill("gdalArg.distance", "1");
        bench.fill("gdalOutput.kind", "File");
        // A file must be named: the layer's default does not follow it there.
        assert!(bench.text("gdalToolboxCommand").is_empty());
        bench.fill("gdalOutput.name", "C:/out/buffer.gpkg");
        assert!(
            bench
                .items("gdalOutput.format")
                .iter()
                .any(|format| format == "GPKG")
        );
        bench.fill("gdalOutput.format", "GPKG");
        bench.tick("gdalOutput.overwrite", true);
        assert_eq!(
            bench.text("gdalToolboxCommand"),
            "GDAL vector buffer --distance=1 FROM DRAWING TO FILE C:/out/buffer.gpkg FORMAT GPKG \
             OVERWRITE"
        );
    }
}

mod gdal_toolbox_line {
    use super::*;

    #[test]
    fn what_the_forms_hold_is_the_line_in_the_verbs_order() {
        let mut form = GdalToolboxForm {
            path: strings(&["vector", "layer-algebra", "intersection"]),
            inputs: vec![
                GdalToolboxInput {
                    arg: String::from("input"),
                    first_required: true,
                    required: true,
                    sources: strings(&["LAYERS lots"]),
                    error: String::new(),
                },
                GdalToolboxInput {
                    arg: String::from("method"),
                    first_required: false,
                    required: true,
                    sources: strings(&["LAYERS corridor"]),
                    error: String::new(),
                },
            ],
            output_kind: String::from("LAYER"),
            output_name: String::from("gis/overlay"),
            ..GdalToolboxForm::default()
        };
        assert_eq!(
            gdal_toolbox_line(&form).unwrap(),
            "GDAL vector layer-algebra intersection FROM LAYERS lots FROM method LAYERS corridor \
             TO LAYER gis/overlay"
        );
        form.inputs[1].sources.clear();
        let missing = gdal_toolbox_line(&form).expect_err("a required input is missing");
        assert!(missing.message.contains("method"));
    }
}
