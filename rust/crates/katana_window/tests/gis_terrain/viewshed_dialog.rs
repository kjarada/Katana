//! The twins of tests/qt_widgets/geo/test_viewshed_dialog.cpp: Terrain > Analysis > Viewshed and
//! Line of Sight (katana_window::geo::viewshed_dialog) - the lines the fields describe, and the
//! dialog driven by its object names, each tab running exactly the line it shows.

use katana_window::customisation::ScopeChoice;
use katana_window::geo::viewshed_dialog::{
    LosForm, ViewshedDialog, ViewshedForm, los_line, viewshed_line,
};
use katana_window::host::RecordingHost;

use super::support::{Bench, PickingViews, add_raster, ran_host};

fn bench(mut host: RecordingHost) -> Bench<ViewshedDialog> {
    let mut dialog = ViewshedDialog::new(&mut host);
    dialog.show(&host);
    Bench::new("viewshedDialog", dialog, host)
}

fn message(form: &ViewshedForm) -> String {
    viewshed_line(form)
        .expect_err("the line is refused")
        .message
}

mod viewshed_line {
    use super::*;

    #[test]
    fn observers_options_and_name_are_written_in_order() {
        let mut form = ViewshedForm {
            source: String::from("RASTER 4"),
            observers: vec![String::from("10,20"), String::from("30.5, 40")],
            ..ViewshedForm::default()
        };
        assert_eq!(
            viewshed_line(&form).unwrap(),
            "RASTER VIEWSHED RASTER 4 OBSERVER 10,20 OBSERVER 30.5,40 height=1.7 target=0"
        );
        form.max = String::from("500");
        form.curvature = String::from("None");
        form.areas = String::from("site/seen");
        form.name = String::from("from the hill");
        assert_eq!(
            viewshed_line(&form).unwrap(),
            "RASTER VIEWSHED RASTER 4 OBSERVER 10,20 OBSERVER 30.5,40 height=1.7 target=0 \
             max=500 curvature=none areas=site/seen NAME \"from the hill\""
        );
        // The points of a scope instead.
        form.use_scope = true;
        form.scope = String::from("LAYERS survey/control");
        form.max.clear();
        form.curvature.clear();
        form.areas.clear();
        form.name.clear();
        assert_eq!(
            viewshed_line(&form).unwrap(),
            "RASTER VIEWSHED RASTER 4 OBSERVERS LAYERS survey/control height=1.7 target=0"
        );
    }

    #[test]
    fn what_cannot_be_written_is_refused_naming_the_field() {
        let mut form = ViewshedForm::default();
        assert!(message(&form).contains("Source"));
        form.source = String::from("SURFACE ground");
        assert!(message(&form).contains("Observers"));
        form.observers = vec![String::from("1,2")];
        form.height = String::from("tall");
        assert!(message(&form).contains("Eye height"));
        form.height = String::from("1.7");
        form.max = String::from("0");
        assert!(message(&form).contains("Look as far"));
        form.max.clear();
        form.curvature = String::from("flat");
        assert!(message(&form).contains("Curvature"));
    }
}

mod los_line {
    use super::*;

    #[test]
    fn both_ends_and_the_heights_are_written() {
        let mut form = LosForm {
            source: String::from("SURFACE ground"),
            ..LosForm::default()
        };
        assert!(los_line(&form).unwrap_err().message.contains("Observer"));
        form.observer = String::from("0,0");
        assert!(los_line(&form).unwrap_err().message.contains("Target"));
        form.target = String::from("100, 50");
        form.curvature = String::from("none");
        assert_eq!(
            los_line(&form).unwrap(),
            "LOS SURFACE ground OBSERVER 0,0 TARGET 100,50 height=1.7 target=0 curvature=none"
        );
    }
}

mod viewshed_dialog {
    use super::*;

    #[test]
    fn every_field_has_its_name_and_each_tab_runs_its_line() {
        // A picker that answers at once, as a click in a plan view would.
        let mut host = ran_host().with_views(Box::new(PickingViews::new(12.0, 34.5)));
        let id = add_raster(&mut host, "terrain", "terrain.asc");
        let mut bench = bench(host);
        assert!(bench.harness.target_record("viewshedDialog").is_some());
        for name in [
            "viewshedTabs",
            "viewshedSource",
            "viewshedObserver",
            "viewshedAdd",
            "viewshedPick",
            "viewshedObservers",
            "viewshedRemove",
            "viewshedUseScope",
            "viewshedScope",
            "viewshedHeight",
            "viewshedTarget",
            "viewshedMax",
            "viewshedCurvature",
            "viewshedAreas",
            "viewshedName",
            "viewshedCommand",
            "viewshedPreview",
            "viewshedRun",
            "viewshedReply",
            "losSource",
            "losObserver",
            "losPickObserver",
            "losTarget",
            "losPickTarget",
            "losHeight",
            "losTargetHeight",
            "losCurvature",
            "losCommand",
            "losRun",
            "losReply",
        ] {
            assert!(bench.has(name), "{name}");
        }
        // Nothing to run until there is an observer: one typed, one picked.
        assert!(!bench.enabled("viewshedRun"));
        bench.fill("viewshedObserver", "1,2");
        bench.press("viewshedAdd");
        bench.press("viewshedPick");
        assert_eq!(bench.items("viewshedObservers").len(), 2);
        bench.fill("viewshedMax", "250");
        bench.press("viewshedRun");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(
            bench.host.lines().last().unwrap(),
            &format!(
                "RASTER VIEWSHED RASTER {id} OBSERVER 1,2 OBSERVER 12.000,34.500 height=1.7 \
                 target=0 max=250"
            )
        );
        assert_eq!(
            bench.text("viewshedReply"),
            format!("ran {}", bench.host.lines().last().unwrap())
        );
        // The points of the whole drawing instead: the list is off.
        bench.tick("viewshedUseScope", true);
        assert!(!bench.enabled("viewshedObservers"));
        bench
            .dialog
            .scope_controls()
            .set_choice(ScopeChoice::Drawing);
        bench.draw();
        bench.press("viewshedPreview");
        assert_eq!(
            bench.host.lines().last().unwrap(),
            &format!(
                "RASTER VIEWSHED RASTER {id} OBSERVERS DRAWING height=1.7 target=0 max=250 PREVIEW"
            )
        );

        // Line of sight: the observer picked, the target typed.
        assert!(!bench.enabled("losRun"));
        bench.press("losPickObserver");
        assert_eq!(bench.text("losObserver"), "12.000,34.500");
        bench.fill("losTarget", "80,20");
        bench.fill("losCurvature", "none");
        bench.press("losRun");
        assert_eq!(
            bench.host.lines().last().unwrap(),
            &format!(
                "LOS RASTER {id} OBSERVER 12.000,34.500 TARGET 80,20 height=1.7 target=0 \
                 curvature=none"
            )
        );
        assert_eq!(
            bench.text("losReply"),
            format!("ran {}", bench.host.lines().last().unwrap())
        );
    }

    #[test]
    fn with_no_picker_the_pick_buttons_are_off() {
        let bench = bench(ran_host());
        for name in ["viewshedPick", "losPickObserver", "losPickTarget"] {
            assert!(!bench.enabled(name), "{name}");
        }
        assert!(bench.placeholder("viewshedCommand").contains("Source"));
    }
}
