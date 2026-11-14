//! The twins of tests/qt_widgets/geo/test_buffer_dialog.cpp: GIS > Analysis - GDAL > Buffer...
//! and Dissolve... (geo::buffer_dialog, geo::dissolve_dialog) and the frame they are built on
//! (geo::gis_tool_dialog): the line each builds from its fields, run through the window's
//! executor, and its reply shown - headless at once, interactively when the job ends. The verbs
//! themselves are tested in katana_app's geo twins. Expected values are the C++'s.

use katana_entity::Geometry;
use katana_window::geo::buffer_dialog::{GisBufferDialog, GisBufferForm, gis_buffer_line};
use katana_window::geo::dissolve_dialog::{GisDissolveDialog, GisDissolveForm, gis_dissolve_line};
use katana_window::geo::gis_tool_dialog::GisTool;
use katana_window::host::{Host, RecordingHost, VerbOutcome};

use super::support::{Bench, WindowHost, fixed, ok, runner_context, window_context};

fn buffer_form(distance: &str) -> GisBufferForm {
    let mut form = GisBufferForm::default();
    form.scope.words = String::from("DRAWING");
    form.distance = String::from(distance);
    form
}

/// The Buffer dialog over a runner that answers `answer`, shown.
fn buffer_bench(answer: VerbOutcome) -> Bench<GisBufferDialog, RecordingHost> {
    let host = fixed(answer);
    let mut dialog = GisBufferDialog::new(runner_context(), &host);
    dialog.show(&host);
    Bench::new("gisBufferDialog", dialog, host)
}

mod gis_buffer_line {
    use super::*;

    #[test]
    fn a_choice_left_at_its_default_is_left_out() {
        let line = gis_buffer_line(&buffer_form("5")).expect("a line");
        assert_eq!(line, "GIS BUFFER DRAWING distance=5");
        let mut form = buffer_form("-3");
        form.side = String::from("left");
        form.caps = String::from("flat");
        form.joins = String::from("mitre");
        form.layer = String::from("design/set backs");
        let line = gis_buffer_line(&form).expect("a line");
        assert_eq!(
            line,
            "GIS BUFFER DRAWING distance=-3 side=left caps=flat joins=mitre TO LAYER \
             \"design/set backs\""
        );
    }

    // A layer called "preview" was written bare, and the line then read PREVIEW as the flag:
    // refused, where the person asked for that layer.
    #[test]
    fn a_layer_named_like_a_keyword_is_quoted() {
        let mut form = buffer_form("5");
        form.layer = String::from("preview");
        let line = gis_buffer_line(&form).expect("a line");
        assert_eq!(line, "GIS BUFFER DRAWING distance=5 TO LAYER \"preview\"");
    }

    #[test]
    fn a_property_distance_wins_and_dissolve_names_its_keys() {
        let mut form = buffer_form("5");
        form.distance_property = String::from("clearance");
        form.dissolve = true;
        let line = gis_buffer_line(&form).expect("a line");
        assert_eq!(line, "GIS BUFFER DRAWING distance=prop:clearance DISSOLVE");
        form.dissolve_by = String::from("owner,stage");
        let line = gis_buffer_line(&form).expect("a line");
        assert_eq!(
            line,
            "GIS BUFFER DRAWING distance=prop:clearance dissolve=owner,stage"
        );
    }

    #[test]
    fn what_cannot_be_said_is_refused_naming_the_field() {
        assert!(gis_buffer_line(&buffer_form("")).is_err());
        assert!(gis_buffer_line(&buffer_form("0")).is_err());
        assert!(gis_buffer_line(&buffer_form("five")).is_err());
        let mut form = buffer_form("5");
        form.scope.words.clear();
        form.scope.error = String::from("tick at least one layer");
        let refused = gis_buffer_line(&form).expect_err("no line");
        assert!(refused.message.contains("tick at least one layer"));
        let mut form = buffer_form("5");
        form.layer = String::from("a\"b");
        assert!(gis_buffer_line(&form).is_err());
    }
}

mod gis_dissolve_line {
    use super::*;

    #[test]
    fn keep_and_replace_are_said_as_the_verb_takes_them() {
        let mut form = GisDissolveForm::default();
        form.scope.words = String::from("LAYERS lots");
        let line = gis_dissolve_line(&form).expect("a line");
        assert_eq!(line, "GIS DISSOLVE LAYERS lots");
        form.by = String::from("owner");
        form.keep_identical = true;
        form.replace = true;
        form.layer = String::from("superlots");
        let line = gis_dissolve_line(&form).expect("a line");
        assert_eq!(
            line,
            "GIS DISSOLVE LAYERS lots by=owner keep=identical TO LAYER superlots REPLACE"
        );
    }
}

mod gis_buffer_dialog {
    use super::*;

    #[test]
    fn every_field_has_its_object_name() {
        let bench = buffer_bench(VerbOutcome::default());
        assert_eq!(bench.dialog.frame().object_name(), "gisBufferDialog");
        for name in [
            "gisBufferScope",
            "gisBufferScopeDrawing",
            "gisBufferFilterLayer",
            "gisBufferDistance",
            "gisBufferDistanceProperty",
            "gisBufferSide",
            "gisBufferCaps",
            "gisBufferJoins",
            "gisBufferDissolve",
            "gisBufferDissolveBy",
            "gisBufferLayer",
            "gisBufferCommand",
            "gisBufferPreview",
            "gisBufferRun",
            "gisBufferStatus",
            "gisBufferReply",
            "gisBufferClose",
        ] {
            assert!(bench.has(name), "{name}");
        }
    }

    #[test]
    fn run_hands_the_line_it_shows_to_the_executor() {
        let mut bench = buffer_bench(ok(
            "gis op=buffer seconds=0.001 cancelled=no\noutput arg=output kind=vector \
             target=layer layer=gis/buffer created=2 updated=0 deleted=0 skipped=0",
        ));
        bench.press("gisBufferScopeDrawing");
        // No distance yet: nothing to run.
        assert!(!bench.enabled("gisBufferRun"));
        bench.fill("gisBufferDistance", "1");
        bench.fill("gisBufferCaps", "flat");
        assert_eq!(
            bench.text("gisBufferCommand"),
            "GIS BUFFER DRAWING distance=1 caps=flat"
        );
        assert!(bench.enabled("gisBufferRun"));
        bench.press("gisBufferRun");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(
            bench.host.lines()[0],
            "GIS BUFFER DRAWING distance=1 caps=flat"
        );
        assert!(bench.dialog.reply_text().contains("created=2"));
        assert!(
            bench.dialog.status_text().contains("one undo step"),
            "{}",
            bench.dialog.status_text()
        );
        bench.press("gisBufferPreview");
        assert_eq!(
            bench.host.lines().last().map(String::as_str),
            Some("GIS BUFFER DRAWING distance=1 caps=flat PREVIEW")
        );
    }

    // A runner that answers as the window does interactively: the line becomes a job, and its
    // reply comes later.
    #[test]
    fn an_interactive_run_shows_its_jobs_reply_when_it_ends() {
        let mut bench = buffer_bench(ok("job id=7 title=\"GIS BUFFER\" state=started"));
        bench.press("gisBufferScopeDrawing");
        bench.fill("gisBufferDistance", "1");
        bench.dialog.run(&mut bench.host);
        assert!(bench.dialog.status_text().contains("background job"));
        let other = VerbOutcome::new(true, "not this one", "");
        bench.dialog.job_finished(8, &other);
        assert!(!bench.dialog.reply_text().contains("not this one"));
        let ended = VerbOutcome::new(
            true,
            "gis op=buffer seconds=0.002 cancelled=no\nbuffer area=200.000",
            "",
        );
        bench.dialog.job_finished(7, &ended);
        assert!(bench.dialog.reply_text().contains("area=200.000"));
        let failed = VerbOutcome::new(false, "", "error: InvalidState: cancelled");
        bench.dialog.run(&mut bench.host);
        bench.dialog.job_finished(7, &failed);
        assert!(bench.dialog.status_text().contains("cancelled"));
    }

    // A 100 m line, 1 m either side, flat ends: 200 m2, one undo step.
    #[test]
    fn its_line_through_the_executor_draws_the_buffer_as_one_step() {
        let mut host = WindowHost::new("katana-gis-dialog-test");
        host.type_line("LINE 0,0 100,0");
        let mut dialog = GisBufferDialog::new(window_context(), &host);
        dialog.show(&host);
        let mut bench = Bench::new("gisBufferDialog", dialog, host);
        bench.press("gisBufferScopeDrawing");
        bench.fill("gisBufferDistance", "1");
        bench.fill("gisBufferCaps", "flat");
        bench.fill("gisBufferLayer", "easement");
        bench.dialog.run(&mut bench.host);
        assert!(
            bench.dialog.reply_text().contains("area=200.000"),
            "{}",
            bench.dialog.reply_text()
        );
        let mut area = 0.0;
        bench.host.document().model().entities.for_each(|entity| {
            if entity.layer == "easement" {
                match &entity.geometry {
                    Geometry::Polyline(polyline) => area += polyline.area(),
                    _ => panic!("the buffer is not a polyline"),
                }
            }
        });
        assert!((area - 200.0_f64).abs() <= 1e-9, "{area}");
        bench.host.type_line("UNDO");
        assert_eq!(bench.host.document().model().entities.size(), 1);
    }
}

mod gis_dissolve_dialog {
    use super::*;

    #[test]
    fn every_field_has_its_object_name_and_the_line_follows_them() {
        let host = fixed(VerbOutcome::default());
        let mut dialog = GisDissolveDialog::new(runner_context(), &host);
        dialog.show(&host);
        let mut bench = Bench::new("gisDissolveDialog", dialog, host);
        assert_eq!(bench.dialog.frame().object_name(), "gisDissolveDialog");
        for name in [
            "gisDissolveScope",
            "gisDissolveBy",
            "gisDissolveKeepIdentical",
            "gisDissolveReplace",
            "gisDissolveLayer",
            "gisDissolveCommand",
            "gisDissolvePreview",
            "gisDissolveRun",
            "gisDissolveReply",
        ] {
            assert!(bench.has(name), "{name}");
        }
        bench.press("gisDissolveScopeDrawing");
        bench.fill("gisDissolveBy", "owner");
        bench.tick("gisDissolveReplace", true);
        assert_eq!(
            bench.text("gisDissolveCommand"),
            "GIS DISSOLVE DRAWING by=owner REPLACE"
        );
    }
}
