//! The twins of tests/qt_widgets/geo/test_pipeline_builder.cpp: the GDAL Toolbox's Pipeline tab
//! (katana_window::geo::pipeline_builder) - steps added, edited, moved and typed, and the one line
//! they make, which Run hands the host exactly. The tab is drawn on its own in the harness's
//! window, as the C++ tests made the widget with no dialog round it.

use katana_window::drive::WINDOW;
use katana_window::geo::pipeline_builder::{PipelineBuilder, pipeline_step_names};
use katana_window::host::RecordingHost;

use super::support::{Bench, stub_host};
use super::window_support::Scratch;

fn bench() -> Bench<PipelineBuilder> {
    let mut host: RecordingHost = stub_host("ok");
    let builder = PipelineBuilder::new(&mut host);
    Bench::new(WINDOW, builder, host)
}

/// The steps gdalPipelineAdd offers, after "Add a step...".
fn offered(bench: &Bench<PipelineBuilder>) -> Vec<String> {
    bench.items("gdalPipelineAdd")[1..].to_vec()
}

fn read_from(bench: &mut Bench<PipelineBuilder>, file: &str) {
    bench.fill("gdalPipelineKind", "File");
    bench.fill("gdalPipelineFile", file);
}

fn has(names: &[String], name: &str) -> bool {
    names.iter().any(|listed| listed == name)
}

mod pipeline_builder {
    use super::*;

    #[test]
    fn hillshade_then_color_map_builds_the_exact_pipeline_string() {
        let mut bench = bench();
        read_from(&mut bench, "C:/data/dem.tif");
        bench.fill("gdalPipelineAdd", "hillshade");
        bench.fill("gdalStep.zfactor", "2");
        bench.fill("gdalPipelineAdd", "color-map");
        bench.fill("gdalStep.color-map", "ramp.txt");
        assert_eq!(
            bench.dialog.rows(),
            [
                "read",
                "hillshade  --zfactor=2",
                "color-map  --color-map=ramp.txt",
                "write"
            ]
        );
        assert_eq!(
            bench.text("gdalPipelineText"),
            "read ! hillshade --zfactor=2 ! color-map --color-map=ramp.txt ! write"
        );
        // A raster made: kept as a reference raster unless said otherwise.
        let command = bench.text("gdalPipelineCommand");
        assert_eq!(
            command,
            "GDAL pipeline \"read ! hillshade --zfactor=2 ! color-map --color-map=ramp.txt ! \
             write\" FROM input FILE C:/data/dem.tif TO REFERENCE pipeline"
        );
        bench.press("gdalPipelineRun");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(bench.host.lines()[0], command);
    }

    #[test]
    fn a_step_offered_is_available_in_a_pipeline() {
        let mut bench = bench();
        read_from(&mut bench, "C:/data/dem.tif");
        let steps = pipeline_step_names();
        assert!(!steps.is_empty());
        let names = offered(&bench);
        assert!(!names.is_empty());
        for name in &names {
            assert!(has(&steps, name), "{name}");
        }
        // A pipeline's own ends are not steps to add; a program is never run.
        for never in ["read", "write", "external", "tee"] {
            assert!(!has(&names, never), "{never}");
        }
        // What the drawing gives is features: a raster step does not read them.
        bench.fill("gdalPipelineKind", "Drawing");
        let vector = offered(&bench);
        assert!(has(&vector, "buffer"));
        assert!(!has(&vector, "hillshade"));
    }

    #[test]
    fn a_raster_turned_into_features_is_offered_feature_steps_after() {
        let mut bench = bench();
        bench.fill("gdalPipelineKind", "Surface");
        assert!(!has(&offered(&bench), "buffer"));
        bench.fill("gdalPipelineAdd", "contour");
        // After contour the pipeline carries features.
        assert!(has(&offered(&bench), "buffer"));
        assert!(!has(&offered(&bench), "hillshade"));
        assert_eq!(bench.items("gdalPipelineOutputKind"), ["Layer", "File"]);
        assert_eq!(bench.text("gdalPipelineOutputName"), "gis/pipeline");
    }

    #[test]
    fn editing_the_text_rebuilds_the_steps() {
        let mut bench = bench();
        read_from(&mut bench, "C:/data/dem.tif");
        // As GDAL's own command line writes it: a value after its option, and one by position
        // (buffer's distance).
        bench.fill(
            "gdalPipelineText",
            "read ! slope --unit percent ! contour --interval=5 ! buffer 0.1 ! write",
        );
        assert_eq!(
            bench.dialog.rows(),
            [
                "read",
                "slope  --unit=percent",
                "contour  --interval=5",
                "buffer  --distance=0.1",
                "write"
            ]
        );
        bench.dialog.select(1);
        bench.draw();
        assert_eq!(bench.value("gdalStep.unit"), "percent");
        // A text the steps cannot follow still runs as typed, and says why.
        bench.fill("gdalPipelineText", "read ! slope --nosuch=1 ! write");
        assert!(bench.text("gdalPipelineStatus").contains("nosuch"));
        assert!(bench.text("gdalPipelineCommand").contains("--nosuch=1"));
    }

    #[test]
    fn a_pipeline_with_an_external_step_cannot_be_built() {
        let mut bench = bench();
        read_from(&mut bench, "C:/data/dem.tif");
        assert!(!has(&offered(&bench), "external"));
        bench.fill(
            "gdalPipelineText",
            "read ! external --command=calc.exe ! write",
        );
        assert!(bench.text("gdalPipelineCommand").is_empty());
        let why = bench.placeholder("gdalPipelineCommand");
        assert!(why.contains("external"), "{why}");
        assert!(!bench.enabled("gdalPipelineRun"));
        // The status says it does not run: it said "It runs as typed" beside a disabled Run.
        let status = bench.text("gdalPipelineStatus");
        assert!(!status.contains("runs as typed"), "{status}");
        assert!(status.contains("cannot run"), "{status}");
        // Run is off: the harness refuses a press of it, where Qt's click did nothing.
        assert!(bench.host.lines().is_empty());
    }

    #[test]
    fn steps_move_and_go_and_the_text_follows() {
        let mut bench = bench();
        read_from(&mut bench, "C:/data/dem.tif");
        bench.fill("gdalPipelineAdd", "fill-nodata");
        bench.fill("gdalPipelineAdd", "hillshade");
        assert_eq!(
            bench.text("gdalPipelineText"),
            "read ! fill-nodata ! hillshade ! write"
        );
        // hillshade is selected; up puts it first.
        bench.press("gdalPipelineUp");
        assert_eq!(
            bench.text("gdalPipelineText"),
            "read ! hillshade ! fill-nodata ! write"
        );
        bench.press("gdalPipelineRemove");
        assert_eq!(bench.text("gdalPipelineText"), "read ! fill-nodata ! write");
        // read and write are the pipeline's own: they do not move or go.
        bench.dialog.select(0);
        bench.draw();
        assert!(!bench.enabled("gdalPipelineRemove"));
    }

    #[test]
    fn a_recipe_is_the_line_saved_into_a_script() {
        let mut bench = bench();
        read_from(&mut bench, "C:/data/dem.tif");
        bench.fill("gdalPipelineAdd", "hillshade");
        let scratch = Scratch::new("pipeline-recipe");
        let script = scratch.path("katana-pipeline-recipe.kcs");
        bench.fill("gdalPipelineScript", &script.to_string_lossy());
        bench.press("gdalPipelineSave");
        // Read as a text file, as the C++ read it: a line ends in "\n" whatever the system
        // writes.
        let saved = std::fs::read_to_string(&script)
            .expect("the script was written")
            .replace("\r\n", "\n");
        assert_eq!(saved, format!("{}\n", bench.text("gdalPipelineCommand")));
    }
}
