//! The twins of tests/qt_widgets/geo/test_slope_analysis_dialog.cpp: Terrain > Analysis > Slope and
//! Aspect (katana_window::geo::slope_analysis_dialog) - the line the fields describe, and the
//! dialog driven by its object names, running exactly the line it shows through its host.

use katana_window::customisation::ScopeChoice;
use katana_window::geo::slope_analysis_dialog::{SlopeAnalysisDialog, SlopeForm, slope_line};
use katana_window::host::RecordingHost;

use super::support::{Bench, add_raster, ran_host};

fn bench(mut host: RecordingHost) -> Bench<SlopeAnalysisDialog> {
    let mut dialog = SlopeAnalysisDialog::new(&mut host);
    dialog.show(&host);
    Bench::new("slopeAnalysisDialog", dialog, host)
}

fn message(form: &SlopeForm) -> String {
    slope_line(form).expect_err("the line is refused").message
}

mod slope_line {
    use super::*;

    #[test]
    fn the_classes_their_layer_and_the_least_area_are_written_together() {
        let mut form = SlopeForm {
            source: String::from("RASTER 3"),
            ..SlopeForm::default()
        };
        assert_eq!(
            slope_line(&form).unwrap(),
            "RASTER SLOPE RASTER 3 unit=percent"
        );
        form.classes = String::from("5, 10, 25");
        form.areas = String::from("site/batters");
        form.min_area = String::from("25");
        form.name = String::from("site slope");
        assert_eq!(
            slope_line(&form).unwrap(),
            "RASTER SLOPE RASTER 3 unit=percent classes=5,10,25 areas=site/batters min_area=25 \
             NAME \"site slope\""
        );
        // Without classes there are no areas to put anywhere.
        form.classes.clear();
        assert_eq!(
            slope_line(&form).unwrap(),
            "RASTER SLOPE RASTER 3 unit=percent NAME \"site slope\""
        );
        // Aspect has no unit and no classes.
        form.aspect = true;
        form.classes = String::from("5");
        form.clip = true;
        form.scope = String::from("LAYERS lots");
        assert_eq!(
            slope_line(&form).unwrap(),
            "RASTER ASPECT RASTER 3 NAME \"site slope\" LAYERS lots"
        );
    }

    #[test]
    fn what_cannot_be_written_is_refused_naming_the_field() {
        let mut form = SlopeForm::default();
        assert!(message(&form).contains("Source"));
        form.source = String::from("SURFACE ground");
        form.classes = String::from("five");
        assert!(message(&form).contains("Classes"));
        form.classes = String::from("5");
        form.min_area = String::from("0");
        assert!(message(&form).contains("Least area"));
        form.min_area.clear();
        form.clip = true;
        form.scope_error = String::from("no layer is ticked");
        assert!(message(&form).contains("Keep inside"));
    }
}

mod slope_analysis_dialog {
    use super::*;

    #[test]
    fn every_field_has_its_name_and_run_runs_the_line_shown() {
        let mut host = ran_host();
        let id = add_raster(&mut host, "terrain", "terrain.asc");
        let mut bench = bench(host);
        assert!(bench.harness.target_record("slopeAnalysisDialog").is_some());
        for name in [
            "slopeSource",
            "slopeKind",
            "slopeUnit",
            "slopeClasses",
            "slopeAreasLayer",
            "slopeMinArea",
            "slopeName",
            "slopeClip",
            "slopeScope",
            "slopeCommand",
            "slopePreview",
            "slopeRun",
            "slopeReply",
        ] {
            assert!(bench.has(name), "{name}");
        }
        assert_eq!(
            bench.text("slopeCommand"),
            format!("RASTER SLOPE RASTER {id} unit=percent")
        );
        // The classes' layer and least area wait for classes.
        assert!(!bench.enabled("slopeAreasLayer"));
        bench.fill("slopeClasses", "5,25");
        assert!(bench.enabled("slopeAreasLayer"));
        bench.fill("slopeMinArea", "4");
        bench.tick("slopeClip", true);
        bench
            .dialog
            .scope_controls()
            .set_choice(ScopeChoice::Drawing);
        bench.draw();
        bench.press("slopeRun");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(
            bench.host.lines()[0],
            format!(
                "RASTER SLOPE RASTER {id} unit=percent classes=5,25 areas=terrain/slope \
                 min_area=4 DRAWING"
            )
        );
        assert_eq!(
            bench.text("slopeReply"),
            format!("ran {}", bench.host.lines()[0])
        );

        // Aspect: no unit, no classes.
        bench.fill("slopeKind", "Aspect");
        assert!(!bench.enabled("slopeUnit"));
        assert!(!bench.enabled("slopeClasses"));
        bench.press("slopePreview");
        assert_eq!(
            bench.host.lines().last().unwrap(),
            &format!("RASTER ASPECT RASTER {id} DRAWING PREVIEW")
        );
    }

    #[test]
    fn with_nothing_to_analyse_run_is_off_and_the_command_says_why() {
        let bench = bench(ran_host());
        assert!(!bench.enabled("slopeRun"));
        assert!(bench.placeholder("slopeCommand").contains("Source"));
    }
}
