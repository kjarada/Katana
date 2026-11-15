//! The twins of tests/qt_widgets/geo/test_grid_dem_dialog.cpp: Terrain > Grid Points to DEM
//! (katana_window::geo::grid_dem_dialog) - the line the fields describe, the dialog driven by its
//! object names, and its line run for real through the window's geoprocessing executor.

use katana_commands::create_entities;
use katana_entity::entity::set_heights;
use katana_entity::{Entity, Geometry, PointGeometry, PropertyValue};
use katana_geometry::Point2;
use katana_window::geo::grid_dem_dialog::{GridDemDialog, GridDemForm, grid_dem_command_line};
use katana_window::host::{Host, RecordingHost, VerbOutcome};

use super::support::{Bench, GeoHost, stub_host};

fn bench(mut host: RecordingHost) -> Bench<GridDemDialog> {
    let mut dialog = GridDemDialog::new(&mut host);
    dialog.show(&host);
    Bench::new("gridDemDialog", dialog, host)
}

/// A point at (x, y) whose height is `z`.
fn point(x: f64, y: f64, z: f64) -> Entity {
    let mut point = Entity {
        geometry: Geometry::Point(PointGeometry {
            position: Point2::new(x, y),
        }),
        ..Entity::default()
    };
    set_heights(&mut point.properties, &[Some(z)]);
    point
}

mod grid_dem_command_line {
    use super::*;

    #[test]
    fn a_blank_field_is_left_out_and_the_verbs_default_applies() {
        let mut form = GridDemForm {
            scope: String::from("DRAWING"),
            ..GridDemForm::default()
        };
        assert_eq!(grid_dem_command_line(&form).unwrap(), "RASTER GRID DRAWING");
        form.z = String::from("geometry");
        assert_eq!(grid_dem_command_line(&form).unwrap(), "RASTER GRID DRAWING");
    }

    #[test]
    fn every_field_given_is_its_word_on_the_line() {
        let form = GridDemForm {
            scope: String::from("LAYERS spots WHERE TYPE=point"),
            method: String::from("invdist"),
            cell: String::from("2.5"),
            z: String::from("rl"),
            extent: String::from("0,0,100,80"),
            power: String::from("2"),
            radius: String::from("8"),
            name: String::from("ground model"),
            ..GridDemForm::default()
        };
        let line = grid_dem_command_line(&form).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(
            line,
            "RASTER GRID LAYERS spots WHERE TYPE=point method=invdist cell=2.5 z=rl \
             extent=0,0,100,80 power=2 radius=8 NAME \"ground model\""
        );
    }

    #[test]
    fn what_a_line_cannot_say_is_refused_naming_the_field() {
        let refused = |form: &GridDemForm, says: &str| {
            let error = grid_dem_command_line(form).expect_err(says);
            assert!(error.message.contains(says), "{}", error.message);
        };
        let form = GridDemForm {
            scope: String::from("DRAWING"),
            ..GridDemForm::default()
        };
        // A comma is no decimal point on the command line.
        let comma = GridDemForm {
            cell: String::from("2,5"),
            ..form.clone()
        };
        refused(&comma, "cell size");
        let size = GridDemForm {
            size: String::from("400 by 300"),
            ..form.clone()
        };
        refused(&size, "size");
        let extent = GridDemForm {
            extent: String::from("0,0,100"),
            ..form.clone()
        };
        refused(&extent, "extent");
        let z = GridDemForm {
            z: String::from("reduced level"),
            ..form.clone()
        };
        refused(&z, "height property");
        let quote = GridDemForm {
            name: String::from("the \"best\" dem"),
            ..form.clone()
        };
        refused(&quote, "double quote");
        let no_scope = GridDemForm {
            scope_error: String::from("tick a layer"),
            ..GridDemForm::default()
        };
        refused(&no_scope, "tick a layer");
    }
}

mod grid_dem_dialog {
    use super::*;

    #[test]
    fn the_fields_build_the_line_run_hands_the_runner() {
        let mut bench = bench(stub_host("grid method=linear"));
        bench.press("gridScopeDrawing");
        bench.fill("gridCell", "5");
        bench.fill("gridName", "ground");
        let command = bench.text("gridCommand");
        assert_eq!(
            command,
            "RASTER GRID DRAWING method=linear cell=5 NAME ground"
        );
        // A cell or a size: the size is not offered while a cell is given.
        assert!(!bench.enabled("gridSize"));
        bench.press("gridRun");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(bench.host.lines()[0], command);
        assert_eq!(bench.text("gridReply"), "grid method=linear");
        bench.press("gridPreview");
        assert_eq!(bench.host.lines().len(), 2);
        assert_eq!(
            bench.host.lines().last().unwrap(),
            &format!("{command} PREVIEW")
        );
    }

    #[test]
    fn keep_as_a_surface_writes_to_surface_with_the_name() {
        // TO SURFACE works on every front end (RASTER GRID ... TO SURFACE g), so the window
        // offers it: the box was disabled, and a line with it could not be built there.
        let mut bench = bench(stub_host("grid method=linear"));
        bench.press("gridScopeDrawing");
        bench.fill("gridCell", "5");
        assert!(bench.enabled("gridToSurface"));
        bench.press("gridToSurface");
        assert_eq!(
            bench.text("gridCommand"),
            "RASTER GRID DRAWING method=linear cell=5 TO SURFACE dem"
        );
        bench.fill("gridName", "ground");
        assert_eq!(
            bench.text("gridCommand"),
            "RASTER GRID DRAWING method=linear cell=5 NAME ground TO SURFACE ground"
        );
    }

    #[test]
    fn power_is_offered_only_to_a_method_that_takes_it() {
        let mut bench = bench(stub_host("grid method=linear"));
        // The methods are GDAL's own, from its catalogue.
        let methods = bench.items("gridMethod");
        for method in ["linear", "invdist", "nearest"] {
            assert!(methods.iter().any(|listed| listed == method), "{method}");
        }
        bench.fill("gridMethod", "linear");
        assert!(!bench.enabled("gridPower"));
        bench.fill("gridMethod", "invdist");
        assert!(bench.enabled("gridPower"));
        bench.fill("gridPower", "2");
        assert!(
            bench
                .text("gridCommand")
                .contains("method=invdist power=2")
        );
        // Back to linear: the power it cannot take leaves the line.
        bench.fill("gridMethod", "linear");
        assert!(!bench.text("gridCommand").contains("power="));
    }

    // The C++ case also told its listener of another job's end (id 6) and saw the panel still
    // running. Which job an end is of is the window's to match (`Host::await_job`, which the geo
    // workbench answers by the job's id - its own twins), so the recording host tells the panel
    // only of its own job's end; that step has no twin here.
    #[test]
    fn an_interactive_run_shows_its_reply_when_the_job_ends() {
        let mut host = stub_host("job id=7 title=\"RASTER GRID\" state=started");
        host.awaits_jobs = true;
        let mut bench = bench(host);
        bench.press("gridScopeDrawing");
        bench.press("gridRun");
        assert!(bench.dialog.run_panel().running());
        assert!(bench.text("gridStatus").contains("job 7"));
        assert_eq!(bench.host.awaited().len(), 1);
        assert!(bench.host.finish_awaited(&VerbOutcome::new(
            true,
            "output arg=output kind=raster target=reference id=1 name=dem",
            ""
        )));
        bench.draw();
        assert!(!bench.dialog.run_panel().running());
        assert!(bench.text("gridReply").contains("name=dem"));
    }

    #[test]
    fn the_height_properties_offered_are_the_numbers_the_drawing_holds() {
        let mut host = stub_host("grid method=linear");
        let mut entity = point(0.0, 0.0, 100.0);
        entity
            .properties
            .insert(String::from("rl"), PropertyValue::Real(101.5));
        entity.properties.insert(
            String::from("code"),
            PropertyValue::Text(String::from("TREE")),
        );
        host.document_mut()
            .execute(create_entities(vec![entity]))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let mut bench = bench(host);
        let z = bench.items("gridZ");
        assert_eq!(z[0], "geometry");
        assert!(z.iter().any(|item| item == "rl"));
        // A word is no height, and the geometry's own height is "geometry".
        assert!(!z.iter().any(|item| item == "code"));
        assert!(!z.iter().any(|item| item == "elevation"));
        bench.fill("gridZ", "rl");
        assert!(bench.text("gridCommand").contains(" z=rl"));
    }

    #[test]
    fn running_its_line_grids_the_points_into_a_reference_raster() {
        // Points every 10 m over 0..40 x 0..30 on z = 100 + x/10 + y/20.
        let mut host = GeoHost::new("grid-dialog-test");
        let mut points = Vec::new();
        for x in (0..=40).step_by(10) {
            for y in (0..=30).step_by(10) {
                let (x, y) = (f64::from(x), f64::from(y));
                points.push(point(x, y, 100.0 + x / 10.0 + y / 20.0));
            }
        }
        host.document_mut()
            .execute(create_entities(points))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let mut dialog = GridDemDialog::new(&mut host);
        dialog.show(&host);
        let mut bench = Bench::new("gridDemDialog", dialog, host);
        bench.press("gridScopeDrawing");
        bench.fill("gridCell", "5");
        bench.fill("gridName", "ground");
        bench.press("gridRun");
        let reply = bench.text("gridReply");
        // 0..40 x 0..30 at 5 m: 8 x 6 cells, by hand.
        assert!(reply.contains("size=8x6"), "{reply}");
        assert!(
            reply.contains("scope arg=input scope=drawing matched=20 used=20 points=20"),
            "{reply}"
        );
        assert!(
            reply.contains("target=reference id=1 name=ground raster=8x6"),
            "{reply}"
        );
        let rasters = bench.host.reference().rasters();
        assert_eq!(rasters.len(), 1);
        assert_eq!(
            rasters[0].derivation,
            "RASTER GRID DRAWING method=linear cell=5 NAME ground"
        );
    }
}
