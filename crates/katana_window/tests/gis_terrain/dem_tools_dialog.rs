//! The twins of tests/qt_widgets/geo/test_dem_tools_dialog.cpp: Terrain > DEM Tools
//! (katana_window::geo::dem_tools_dialog) - each tool's line from its fields, the dialog driven by
//! its object names, and a clip run for real through the window's geoprocessing executor.

use katana_window::geo::dem_tools_dialog::{
    DemTool, DemToolForm, DemToolsDialog, dem_tool_command_line,
};
use katana_window::host::{Host, RecordingHost};

use super::support::{Bench, GeoHost, add_raster, stub_host};
use super::window_support::geo_data;

fn bench(mut host: RecordingHost) -> Bench<DemToolsDialog> {
    let mut dialog = DemToolsDialog::new(&mut host);
    dialog.show(&host);
    Bench::new("demToolsDialog", dialog, host)
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| String::from(*word)).collect()
}

fn options(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(key, value)| (String::from(*key), String::from(*value)))
        .collect()
}

mod dem_tool_command_line {
    use super::*;

    #[test]
    fn each_tools_line_is_its_verb_sources_and_options_in_order() {
        let mosaic = DemToolForm {
            tool: DemTool::Mosaic,
            sources: strings(&["FILE tiles/a.tif", "FILE tiles/b.tif"]),
            options: options(&[("resolution", "highest")]),
            save: String::from("C:/work/site dem.tif"),
            ..DemToolForm::default()
        };
        assert_eq!(
            dem_tool_command_line(&mosaic).unwrap(),
            "RASTER MOSAIC FILE tiles/a.tif FILE tiles/b.tif resolution=highest SAVE \
             \"C:/work/site dem.tif\""
        );

        let clip = DemToolForm {
            tool: DemTool::Clip,
            sources: strings(&["RASTER 2"]),
            region: String::from("LAYERS boundary"),
            name: String::from("site"),
            ..DemToolForm::default()
        };
        assert_eq!(
            dem_tool_command_line(&clip).unwrap(),
            "RASTER CLIP RASTER 2 LAYERS boundary NAME site"
        );

        let fill = DemToolForm {
            tool: DemTool::Fill,
            sources: strings(&["SURFACE ground CELL 1"]),
            options: options(&[("distance", "20"), ("smoothing", ""), ("strategy", "nearest")]),
            ..DemToolForm::default()
        };
        assert_eq!(
            dem_tool_command_line(&fill).unwrap(),
            "RASTER FILL SURFACE ground CELL 1 distance=20 strategy=nearest"
        );

        let footprint = DemToolForm {
            tool: DemTool::Footprint,
            sources: strings(&["RASTER 1"]),
            layer: String::from("survey/extent"),
            ..DemToolForm::default()
        };
        assert_eq!(
            dem_tool_command_line(&footprint).unwrap(),
            "RASTER FOOTPRINT RASTER 1 TO LAYER survey/extent"
        );

        let difference = DemToolForm {
            tool: DemTool::Difference,
            sources: strings(&["RASTER 3", "RASTER 1"]),
            region: String::from("SELECTION"),
            ..DemToolForm::default()
        };
        assert_eq!(
            dem_tool_command_line(&difference).unwrap(),
            "RASTER DIFFERENCE RASTER 3 RASTER 1 SELECTION"
        );
    }

    #[test]
    fn what_a_line_cannot_say_is_refused_naming_the_field() {
        let refused = |form: &DemToolForm, says: &str| {
            let error = dem_tool_command_line(form).expect_err(says);
            assert!(error.message.contains(says), "{}", error.message);
        };
        let none = DemToolForm {
            tool: DemTool::Fill,
            ..DemToolForm::default()
        };
        refused(&none, "raster");
        let clip = DemToolForm {
            tool: DemTool::Clip,
            sources: strings(&["RASTER 1"]),
            ..DemToolForm::default()
        };
        refused(&clip, "clip to");
        let distance = DemToolForm {
            tool: DemTool::Fill,
            sources: strings(&["RASTER 1"]),
            options: options(&[("distance", "ten")]),
            ..DemToolForm::default()
        };
        refused(&distance, "distance must be a number");
        let crs = DemToolForm {
            tool: DemTool::Reproject,
            sources: strings(&["RASTER 1"]),
            options: options(&[("crs", "GDA94 MGA 56")]),
            ..DemToolForm::default()
        };
        refused(&crs, "one word");
    }
}

mod dem_tools_dialog {
    use super::*;

    #[test]
    fn a_clip_tab_builds_its_line_and_run_hands_the_runner_exactly_that() {
        let mut bench = bench(stub_host("ok"));
        bench.dialog.show_tool(DemTool::Clip);
        bench.draw();
        bench.fill("demClipKind", "File");
        bench.fill("demClipFile", "C:/data/dem.tif");
        bench.fill("demClipArea", "0,0,20,15");
        assert_eq!(
            bench.text("demClipCommand"),
            "RASTER CLIP FILE C:/data/dem.tif AREA 0,0,20,15"
        );
        // To the boundaries of a scope instead: the scope's own words.
        bench.press("demClipByBoundaries");
        bench.press("demClipBoundaryScopeDrawing");
        let command = bench.text("demClipCommand");
        assert_eq!(command, "RASTER CLIP FILE C:/data/dem.tif DRAWING");
        bench.press("demClipRun");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(bench.host.lines()[0], command);
        assert_eq!(bench.text("demClipReply"), "ok");
    }

    // The Clip tab says the rule GDAL's clip keeps: every cell a boundary touches (DemVerbs'
    // triangle keeps 231 cells, the centre rule 210). It said a cell was kept when its centre was
    // inside.
    #[test]
    fn the_clip_tab_says_every_cell_a_boundary_touches_is_kept() {
        let mut bench = bench(stub_host("ok"));
        bench.dialog.show_tool(DemTool::Clip);
        bench.draw();
        let rule = bench.text("demClipNote");
        assert!(rule.contains("every cell a boundary touches"), "{rule}");
        assert!(!rule.contains("centre"), "{rule}");
    }

    #[test]
    fn the_mosaics_tiles_are_added_from_its_picker_or_the_picker_is_the_one() {
        let mut bench = bench(stub_host("ok"));
        bench.fill("demMosaicKind", "File");
        // None listed: the folder picked is the mosaic's source.
        bench.fill("demMosaicFile", "C:/survey/tiles");
        assert_eq!(
            bench.text("demMosaicCommand"),
            "RASTER MOSAIC FILE C:/survey/tiles"
        );
        bench.fill("demMosaicFile", "C:/survey/a.tif");
        bench.press("demMosaicAdd");
        bench.fill("demMosaicFile", "C:/survey/b tile.tif");
        bench.press("demMosaicAdd");
        assert_eq!(bench.items("demMosaicTiles").len(), 2);
        assert_eq!(
            bench.text("demMosaicCommand"),
            "RASTER MOSAIC FILE C:/survey/a.tif FILE \"C:/survey/b tile.tif\""
        );
        bench.fill("demMosaicSave", "C:/survey/site.tif");
        assert!(
            bench
                .text("demMosaicCommand")
                .ends_with(" SAVE C:/survey/site.tif")
        );
    }

    #[test]
    fn a_reference_raster_is_picked_by_its_id() {
        let mut host = stub_host("ok");
        let id = add_raster(&mut host, "ground", "C:/data/ground.tif");
        let mut bench = bench(host);
        bench.fill("demFillKind", "Reference raster");
        assert_eq!(bench.value("demFillRaster"), format!("{id}  ground"));
        bench.fill("demFillDistance", "20");
        assert_eq!(
            bench.text("demFillCommand"),
            format!("RASTER FILL RASTER {id} distance=20")
        );
        // The rasters REPROJECT can align to are the same ones; choosing one takes the place of a
        // system.
        bench.fill("demReprojectKind", "File");
        bench.fill("demReprojectFile", "C:/data/new.tif");
        bench.fill("demReprojectCrs", "EPSG:28356");
        // The item at index 1: the first raster after "(none)".
        let like = bench.items("demReprojectLike")[1].clone();
        bench.fill("demReprojectLike", &like);
        assert!(!bench.enabled("demReprojectCrs"));
        assert_eq!(
            bench.text("demReprojectCommand"),
            format!("RASTER REPROJECT FILE C:/data/new.tif like={id}")
        );
    }

    #[test]
    fn a_difference_is_the_first_minus_the_second_within_an_optional_scope() {
        let mut bench = bench(stub_host("ok"));
        bench.fill("demDifferenceKind", "File");
        bench.fill("demDifferenceFile", "C:/data/design.tif");
        bench.fill("demDifferenceSecondKind", "File");
        bench.fill("demDifferenceSecondFile", "C:/data/ground.tif");
        assert_eq!(
            bench.text("demDifferenceCommand"),
            "RASTER DIFFERENCE FILE C:/data/design.tif FILE C:/data/ground.tif"
        );
        // The scope is offered only when asked for.
        assert!(!bench.enabled("demDifferenceWithinScopeDrawing"));
        bench.tick("demDifferenceLimit", true);
        bench.press("demDifferenceWithinScopeDrawing");
        assert_eq!(
            bench.text("demDifferenceCommand"),
            "RASTER DIFFERENCE FILE C:/data/design.tif FILE C:/data/ground.tif DRAWING"
        );
    }

    #[test]
    fn a_headless_browse_says_to_fill_the_field() {
        let mut bench = bench(stub_host("ok"));
        bench.fill("demFootprintKind", "File");
        bench.press("demFootprintBrowse");
        assert!(bench.text("demFootprintStatus").contains("demFootprintFile"));
        assert!(bench.host.lines().is_empty());
    }

    #[test]
    fn running_a_clip_through_the_windows_executor_keeps_a_reference_raster() {
        // tests/geo/data/plane.asc: 40 x 30 cells of 1 m from (0,0).
        let plane = geo_data().join("plane.asc");
        let mut host = GeoHost::new("dem-dialog-test");
        let mut dialog = DemToolsDialog::new(&mut host);
        dialog.show(&host);
        dialog.show_tool(DemTool::Clip);
        let mut bench = Bench::new("demToolsDialog", dialog, host);
        bench.fill("demClipKind", "File");
        bench.fill("demClipFile", &plane.to_string_lossy());
        bench.fill("demClipArea", "0,0,20,15");
        bench.fill("demClipName", "west");
        bench.press("demClipRun");
        let reply = bench.text("demClipReply");
        // 0..20 x 0..15 of 1 m cells: 20 x 15, by hand.
        assert!(reply.contains("name=west raster=20x15"), "{reply}");
        assert_eq!(bench.host.reference().rasters().len(), 1);
        // What the job made is offered at once to the next tool.
        assert!(
            bench
                .items("demFillRaster")
                .iter()
                .any(|item| item == "1  west")
        );
    }
}
