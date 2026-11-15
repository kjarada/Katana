//! The twins of tests/qt_widgets/geo/test_terrain_shading_dialog.cpp: Terrain > Analysis > Terrain
//! Shading (katana_window::geo::terrain_shading_dialog) - the line the fields describe, and the
//! dialog driven by its object names, running exactly the line it shows through its host.

use katana_terrain::colour_ramp_names;
use katana_window::geo::terrain_shading_dialog::{ShadingForm, TerrainShadingDialog, shading_line};
use katana_window::host::RecordingHost;

use super::support::{Bench, add_raster, ran_host};

fn bench(host: RecordingHost) -> Bench<TerrainShadingDialog> {
    let mut dialog = TerrainShadingDialog::new(&host);
    dialog.show(&host);
    Bench::new("terrainShadingDialog", dialog, host)
}

fn message(form: &ShadingForm) -> String {
    shading_line(form).expect_err("the line is refused").message
}

/// QDir::toNativeSeparators, as the C++ test wrote its path.
fn native(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        String::from(path)
    }
}

mod shading_line {
    use super::*;

    #[test]
    fn the_light_is_written_for_a_hillshade_only_where_it_is_not_gdals_default() {
        let mut form = ShadingForm {
            source: String::from("RASTER 2"),
            ..ShadingForm::default()
        };
        assert_eq!(
            shading_line(&form).unwrap(),
            "RASTER SHADE RASTER 2 style=hillshade"
        );
        form.azimuth = 270.0;
        form.z_factor = 2.5;
        form.variant = String::from("combined");
        assert_eq!(
            shading_line(&form).unwrap(),
            "RASTER SHADE RASTER 2 style=hillshade azimuth=270 z=2.5 variant=combined"
        );
        // Multidirectional light comes from every side: no azimuth for it.
        form.variant = String::from("multidirectional");
        assert_eq!(
            shading_line(&form).unwrap(),
            "RASTER SHADE RASTER 2 style=hillshade z=2.5 variant=multidirectional"
        );
        // A coloured style has no light, and its own ramp is not written.
        form.style = String::from("relief");
        form.ramp = String::from("terrain");
        assert_eq!(
            shading_line(&form).unwrap(),
            "RASTER SHADE RASTER 2 style=relief"
        );
    }

    #[test]
    fn the_ramp_the_range_the_name_and_the_save_are_written() {
        let form = ShadingForm {
            source: String::from("SURFACE ground"),
            style: String::from("relief+hillshade"),
            ramp: String::from("diverging"),
            range_min: String::from("-2"),
            range_max: String::from("2"),
            name: String::from("cut fill"),
            save: native("C:/out/cut fill.tif"),
            overwrite: true,
            ..ShadingForm::default()
        };
        assert_eq!(
            shading_line(&form).unwrap(),
            "RASTER SHADE SURFACE ground style=relief+hillshade ramp=diverging range=-2,2 NAME \
             \"cut fill\" save=\"C:/out/cut fill.tif\" OVERWRITE"
        );
    }

    #[test]
    fn what_cannot_be_written_is_refused_naming_the_field() {
        let mut form = ShadingForm::default();
        assert!(message(&form).contains("Source"));
        form.source = String::from("RASTER 1");
        form.style = String::from("slope");
        form.range_min = String::from("0");
        assert!(message(&form).contains("Range"));
        form.range_max = String::from("-1");
        assert!(message(&form).contains("Range"));
        form.range_min.clear();
        form.range_max.clear();
        form.name = String::from("a\"b");
        assert!(message(&form).contains("Name"));
    }
}

mod terrain_shading_dialog {
    use super::*;

    #[test]
    fn every_field_has_its_name_and_run_runs_the_line_shown() {
        let mut host = ran_host();
        let id = add_raster(&mut host, "terrain", "terrain.asc");
        let mut bench = bench(host);
        assert!(
            bench
                .harness
                .target_record("terrainShadingDialog")
                .is_some()
        );
        for name in [
            "shadingSource",
            "shadingStyle",
            "shadingAzimuth",
            "shadingAltitude",
            "shadingZFactor",
            "shadingVariant",
            "shadingRamp",
            "shadingRangeMin",
            "shadingRangeMax",
            "shadingName",
            "shadingSave",
            "shadingSaveBrowse",
            "shadingOverwrite",
            "shadingCommand",
            "shadingPreview",
            "shadingRun",
            "shadingReply",
        ] {
            assert!(bench.has(name), "{name}");
        }
        assert_eq!(
            bench.text("shadingCommand"),
            format!("RASTER SHADE RASTER {id} style=hillshade")
        );
        // A hillshade is grey: the ramp waits for a coloured style.
        assert!(!bench.enabled("shadingRamp"));

        // The item whose data is "slope".
        bench.fill("shadingStyle", "Slope");
        assert!(bench.enabled("shadingRamp"));
        assert!(!bench.enabled("shadingAzimuth"));
        assert_eq!(bench.value("shadingRamp"), "slope");
        bench.fill("shadingRangeMin", "0");
        bench.fill("shadingRangeMax", "30");
        bench.press("shadingRun");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(
            bench.host.lines()[0],
            format!("RASTER SHADE RASTER {id} style=slope range=0,30")
        );
        assert_eq!(
            bench.text("shadingReply"),
            format!("ran {}", bench.host.lines()[0])
        );
        bench.press("shadingPreview");
        assert!(bench.host.lines().last().unwrap().ends_with(" PREVIEW"));
    }

    #[test]
    fn the_ramp_box_offers_every_ramp_of_the_one_table_in_its_order() {
        // The box is filled from the table the verb and the surface styles read
        // (katana_terrain's colour ramps), so earth and spectral - the styles' own until the
        // tables were made one - are offered with the rest.
        let bench = bench(ran_host());
        let ramp = bench.items("shadingRamp");
        let names = colour_ramp_names();
        assert_eq!(ramp.len(), names.len());
        for (at, name) in names.iter().enumerate() {
            assert_eq!(ramp[at], *name, "{at}");
        }
        assert!(ramp.iter().any(|name| name == "spectral"));
        assert!(ramp.iter().any(|name| name == "earth"));
    }

    #[test]
    fn with_nothing_to_shade_run_is_off_and_the_command_says_why() {
        let bench = bench(ran_host());
        assert!(!bench.enabled("shadingRun"));
        assert!(bench.placeholder("shadingCommand").contains("Source"));
    }
}
