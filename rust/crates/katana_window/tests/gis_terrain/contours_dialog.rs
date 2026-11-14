//! The twins of tests/qt_widgets/geo/test_contours_dialog.cpp: Terrain > Analysis > Contours
//! (katana_window::geo::contours_dialog) - the line the fields describe, and the dialog driven by
//! its object names as a person clicks it, running exactly the line it shows through the host it
//! was given.

use katana_window::customisation::ScopeChoice;
use katana_window::geo::contours_dialog::{ContourForm, ContoursDialog, contour_line};
use katana_window::host::RecordingHost;

use super::support::{Bench, add_raster, add_surface, ran_host, tin_of};

fn bench(mut host: RecordingHost) -> Bench<ContoursDialog> {
    let mut dialog = ContoursDialog::new(&mut host);
    dialog.show(&host);
    Bench::new("contoursDialog", dialog, host)
}

fn message(form: &ContourForm) -> String {
    contour_line(form).expect_err("the line is refused").message
}

mod contour_line {
    use super::*;

    #[test]
    fn the_fields_write_the_line_and_a_blank_is_left_out() {
        let mut form = ContourForm {
            source: String::from("RASTER 3"),
            interval: String::from("0.5"),
            major_every: 2,
            base: String::from("100"),
            layer: String::from("site contours"),
            smooth: 5,
            ..ContourForm::default()
        };
        assert_eq!(
            contour_line(&form).unwrap(),
            "CONTOUR RASTER 3 interval=0.5 major=2 base=100 layer=\"site contours\" smooth=5"
        );
        form.base.clear();
        form.layer.clear();
        form.clip = true;
        form.scope = String::from("LAYERS lots WHERE TYPE=polyline");
        assert_eq!(
            contour_line(&form).unwrap(),
            "CONTOUR RASTER 3 interval=0.5 major=2 smooth=5 LAYERS lots WHERE TYPE=polyline"
        );
        // A surface is traced exactly: no smoothing is written for one.
        form.source = String::from("SURFACE ground");
        form.clip = false;
        assert_eq!(
            contour_line(&form).unwrap(),
            "CONTOUR SURFACE ground interval=0.5 major=2"
        );
    }

    #[test]
    fn what_cannot_be_written_is_refused_naming_the_field() {
        let mut form = ContourForm {
            interval: String::from("1"),
            ..ContourForm::default()
        };
        assert!(message(&form).contains("Source"));
        form.source = String::from("SURFACE ground");
        for interval in ["", "0", "-1", "one"] {
            form.interval = String::from(interval);
            assert!(message(&form).contains("Interval"), "{interval}");
        }
        form.interval = String::from("1");
        form.base = String::from("sea");
        assert!(message(&form).contains("Base"));
        form.base.clear();
        form.clip = true;
        form.scope_error = String::from("no layer is ticked");
        assert!(message(&form).contains("Keep inside"));
    }
}

mod contours_dialog {
    use super::*;

    #[test]
    fn a_surface_is_chosen_and_run_runs_the_line_shown() {
        let mut host = ran_host();
        add_surface(
            &mut host,
            "ground",
            tin_of(&[
                (0.0, 0.0, 1.0),
                (10.0, 0.0, 2.0),
                (10.0, 10.0, 3.0),
                (0.0, 10.0, 2.0),
            ]),
        );
        let mut bench = bench(host);
        assert!(bench.harness.target_record("contoursDialog").is_some());
        for name in [
            "contourSource",
            "contourInterval",
            "contourMajorEvery",
            "contourBase",
            "contourLayer",
            "contourSmooth",
            "contourClip",
            "contourScope",
            "contourCommand",
            "contourPreview",
            "contourRun",
            "contourReply",
        ] {
            assert!(bench.has(name), "{name}");
        }
        assert_eq!(bench.items("contourSource").len(), 1);
        assert_eq!(bench.dialog.form().source, "SURFACE ground");
        // A surface has no smoothing, and the drawing's controls wait for the box.
        assert!(!bench.enabled("contourSmooth"));
        assert!(!bench.enabled("contourScope"));
        assert_eq!(
            bench.text("contourCommand"),
            "CONTOUR SURFACE ground interval=1 major=5 layer=terrain/contours"
        );

        bench.fill("contourInterval", "0.25");
        bench.fill("contourMajorEvery", "4");
        bench.press("contourRun");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(
            bench.host.lines()[0],
            "CONTOUR SURFACE ground interval=0.25 major=4 layer=terrain/contours"
        );
        assert_eq!(
            bench.text("contourReply"),
            format!("ran {}", bench.host.lines()[0])
        );
        bench.press("contourPreview");
        assert_eq!(
            bench.host.lines().last().unwrap(),
            "CONTOUR SURFACE ground interval=0.25 major=4 layer=terrain/contours PREVIEW"
        );
    }

    #[test]
    fn a_raster_takes_smoothing_and_the_clip_writes_the_scope() {
        let mut host = ran_host();
        let id = add_raster(&mut host, "terrain", "terrain.asc");
        let mut bench = bench(host);
        assert_eq!(bench.items("contourSource").len(), 1);
        assert_eq!(bench.dialog.form().source, format!("RASTER {id}"));
        assert!(bench.enabled("contourSmooth"));
        // The item whose data is 3.
        bench.fill("contourSmooth", "3 cells");
        bench.tick("contourClip", true);
        assert!(bench.enabled("contourScope"));
        bench
            .dialog
            .scope_controls()
            .set_choice(ScopeChoice::Drawing);
        bench.draw();
        assert_eq!(
            bench.text("contourCommand"),
            format!(
                "CONTOUR RASTER {id} interval=1 major=5 layer=terrain/contours smooth=3 DRAWING"
            )
        );
    }

    #[test]
    fn with_nothing_to_contour_run_is_off_and_the_command_says_why() {
        let bench = bench(ran_host());
        assert!(!bench.enabled("contourRun"));
        assert!(bench.placeholder("contourCommand").contains("Source"));
        assert!(bench.host.lines().is_empty());
    }
}
