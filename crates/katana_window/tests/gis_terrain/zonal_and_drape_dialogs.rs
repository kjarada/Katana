//! The twins of tests/qt_widgets/geo/test_zonal_and_drape_dialogs.cpp: Terrain > Analysis >
//! Statistics by Area (katana_window::geo::zonal_stats_dialog) and Drape and Sample Heights
//! (katana_window::geo::drape_dialog) - the lines the fields describe, and each dialog driven by
//! its object names, running exactly the line it shows.

use katana_geometry::Point3;
use katana_terrain::TinSurface;
use katana_window::customisation::ScopeChoice;
use katana_window::geo::drape_dialog::{
    DrapeDialog, DrapeForm, SampleForm, drape_line, point_words, sample_line,
};
use katana_window::geo::zonal_stats_dialog::{ZonalForm, ZonalStatsDialog, zonal_line};
use katana_window::host::RecordingHost;

use super::support::{Bench, PickingViews, add_raster, add_surface, ran_host};

fn zonal_bench(mut host: RecordingHost) -> Bench<ZonalStatsDialog> {
    let mut dialog = ZonalStatsDialog::new(&mut host);
    dialog.show(&host);
    Bench::new("zonalStatsDialog", dialog, host)
}

fn drape_bench(mut host: RecordingHost) -> Bench<DrapeDialog> {
    let mut dialog = DrapeDialog::new(&mut host);
    dialog.show(&host);
    Bench::new("drapeDialog", dialog, host)
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| String::from(*word)).collect()
}

// ---- Statistics by Area -------------------------------------------------------------------------

mod zonal_line {
    use super::*;

    #[test]
    fn the_scope_statistics_prefix_and_file_are_written_in_order() {
        let mut form = ZonalForm {
            source: String::from("RASTER 2"),
            scope: String::from("LAYERS lots"),
            stats: strings(&["mean", "count"]),
            ..ZonalForm::default()
        };
        assert_eq!(
            zonal_line(&form).unwrap(),
            "RASTER ZONAL RASTER 2 LAYERS lots stats=mean,count prefix=zone pixels=fractional"
        );
        form.prefix = String::from("ground");
        form.pixels = String::from("centre");
        form.csv = String::from("C:/out/lot stats.csv");
        form.overwrite = true;
        assert_eq!(
            zonal_line(&form).unwrap(),
            "RASTER ZONAL RASTER 2 LAYERS lots stats=mean,count prefix=ground pixels=centre \
             \"csv=C:/out/lot stats.csv\" OVERWRITE"
        );
    }

    #[test]
    fn what_cannot_be_written_is_refused_naming_the_field() {
        let message = |form: &ZonalForm| zonal_line(form).expect_err("refused").message;
        let mut form = ZonalForm {
            stats: strings(&["mean"]),
            ..ZonalForm::default()
        };
        assert!(message(&form).contains("Source"));
        form.source = String::from("SURFACE ground");
        form.stats.clear();
        assert!(message(&form).contains("Statistics"));
        form.stats = strings(&["mean"]);
        form.prefix = String::from("a b");
        assert!(message(&form).contains("prefix"));
        form.prefix = String::from("zone");
        form.scope_error = String::from("no layer is ticked");
        assert!(message(&form).contains("Zones"));
    }
}

mod zonal_stats_dialog {
    use super::*;

    #[test]
    fn every_field_has_its_name_and_run_runs_the_line_shown() {
        let mut host = ran_host();
        let id = add_raster(&mut host, "terrain", "terrain.asc");
        let mut bench = zonal_bench(host);
        assert!(bench.harness.target_record("zonalStatsDialog").is_some());
        for name in [
            "zonalSource",
            "zonalScope",
            "zonalStats",
            "zonalPrefix",
            "zonalPixels",
            "zonalCsv",
            "zonalOverwrite",
            "zonalCommand",
            "zonalPreview",
            "zonalRun",
            "zonalReply",
        ] {
            assert!(bench.has(name), "{name}");
        }
        // The verb's own default statistics are ticked, the zones the selection.
        assert_eq!(
            bench.text("zonalCommand"),
            format!(
                "RASTER ZONAL RASTER {id} SELECTION stats=mean,min,max,count,sum prefix=zone \
                 pixels=fractional"
            )
        );
        // Untick all but the count (each item's setCheckState, by the word it carries); zones from
        // the whole drawing.
        for word in ZonalStatsDialog::stat_words() {
            bench.dialog.set_stat(word, word == "count");
        }
        bench
            .dialog
            .scope_controls()
            .set_choice(ScopeChoice::Drawing);
        bench.draw();
        bench.fill("zonalPrefix", "lot");
        assert!(!bench.enabled("zonalOverwrite"));
        bench.press("zonalRun");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(
            bench.host.lines()[0],
            format!("RASTER ZONAL RASTER {id} DRAWING stats=count prefix=lot pixels=fractional")
        );
        assert_eq!(
            bench.text("zonalReply"),
            format!("ran {}", bench.host.lines()[0])
        );
        bench.press("zonalPreview");
        assert!(bench.host.lines().last().unwrap().ends_with(" PREVIEW"));
    }

    #[test]
    fn with_nothing_to_measure_run_is_off_and_the_command_says_why() {
        let bench = zonal_bench(ran_host());
        assert!(!bench.enabled("zonalRun"));
        assert!(bench.placeholder("zonalCommand").contains("Source"));
    }
}

// ---- Drape and Sample Heights -------------------------------------------------------------------

mod drape_line {
    use super::*;

    #[test]
    fn a_raster_takes_a_method_and_a_surface_does_not() {
        let mut form = DrapeForm {
            source: String::from("RASTER 1"),
            scope: String::from("LAYERS design/strings"),
            ..DrapeForm::default()
        };
        assert_eq!(
            drape_line(&form).unwrap(),
            "DRAPE RASTER 1 LAYERS design/strings method=bilinear"
        );
        form.source = String::from("SURFACE ground");
        assert_eq!(
            drape_line(&form).unwrap(),
            "DRAPE SURFACE ground LAYERS design/strings"
        );
        form.scope_error = String::from("no layer is ticked");
        assert!(drape_line(&form).is_err());
    }
}

mod sample_line {
    use super::*;

    #[test]
    fn each_point_is_an_at_and_a_point_must_be_two_numbers() {
        let mut form = SampleForm {
            source: String::from("RASTER 1"),
            ..SampleForm::default()
        };
        assert!(sample_line(&form).unwrap_err().message.contains("Points"));
        form.points = strings(&["12.5,40", "3, 4"]);
        form.method = String::from("nearest");
        assert_eq!(
            sample_line(&form).unwrap(),
            "RASTER SAMPLE RASTER 1 AT 12.5,40 AT 3,4 method=nearest"
        );
        form.points = strings(&["12.5"]);
        assert!(sample_line(&form).is_err());
        assert_eq!(point_words(1.23456, -7.0), "1.235,-7.000");
    }
}

mod drape_dialog {
    use super::*;

    #[test]
    fn every_field_has_its_name_and_each_tab_runs_its_line() {
        // A picker that answers at once, as a click in a plan view would.
        let mut host = ran_host().with_views(Box::new(PickingViews::new(3.25, 4.5)));
        let tin = TinSurface::create(
            vec![
                Point3::new(0.0, 0.0, 10.0),
                Point3::new(10.0, 0.0, 11.0),
                Point3::new(0.0, 10.0, 12.0),
            ],
            vec![[0, 1, 2]],
            Vec::new(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        add_surface(&mut host, "ground", tin);
        let mut bench = drape_bench(host);
        assert!(bench.harness.target_record("drapeDialog").is_some());
        for name in [
            "drapeTabs",
            "drapeSource",
            "drapeMethod",
            "drapeScope",
            "drapeCommand",
            "drapePreview",
            "drapeRun",
            "drapeReply",
            "sampleSource",
            "sampleMethod",
            "samplePoint",
            "sampleAdd",
            "samplePick",
            "samplePoints",
            "sampleRemove",
            "sampleCommand",
            "sampleRun",
            "sampleReply",
        ] {
            assert!(bench.has(name), "{name}");
        }
        // A surface is read on its triangles: no method.
        assert!(!bench.enabled("drapeMethod"));
        bench
            .dialog
            .scope_controls()
            .set_choice(ScopeChoice::Drawing);
        bench.draw();
        bench.press("drapeRun");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(
            bench.host.lines().last().unwrap(),
            "DRAPE SURFACE ground DRAWING"
        );
        bench.press("drapePreview");
        assert_eq!(
            bench.host.lines().last().unwrap(),
            "DRAPE SURFACE ground DRAWING PREVIEW"
        );
        // Sample: nothing to run until there is a point; one typed, one picked.
        assert!(!bench.enabled("sampleRun"));
        bench.fill("samplePoint", "1,2");
        bench.press("sampleAdd");
        bench.press("samplePick");
        assert_eq!(bench.items("samplePoints").len(), 2);
        bench.press("sampleRun");
        assert_eq!(
            bench.host.lines().last().unwrap(),
            "RASTER SAMPLE SURFACE ground AT 1,2 AT 3.250,4.500"
        );
        assert_eq!(
            bench.text("sampleReply"),
            format!("ran {}", bench.host.lines().last().unwrap())
        );
        // A point that is not two numbers is not added.
        bench.fill("samplePoint", "north");
        bench.press("sampleAdd");
        assert_eq!(bench.items("samplePoints").len(), 2);
    }

    #[test]
    fn with_no_picker_pick_is_off() {
        let bench = drape_bench(ran_host());
        assert!(!bench.enabled("samplePick"));
        assert!(!bench.enabled("drapeRun"));
    }
}
