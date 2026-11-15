//! The twins of tests/qt_widgets/terrain/test_surface_edit_dialogs.cpp: the surface editing
//! dialogs (katana_window::terrain::surface_edit_dialogs) and Terrain > Edit Surface's menu and
//! toolbar (katana_window::terrain::surface_edit_ui) - the lines the fields describe, the dialogs
//! driven by their object names, and the toolbar's list pointing the tools.

use katana_cad::interactive_tool::tool_catalog;
use katana_cad::terrain::surface_edit_verbs::{
    active_edit_surface, set_surface_edit_target, surface_edit_target,
};
use katana_core::Result;
use katana_window::drive::WINDOW;
use katana_window::drive::action::{Menu, MenuItem};
use katana_window::host::RecordingHost;
use katana_window::terrain::surface_edit_dialogs::{
    SurfaceCombineDialog, SurfaceCombineForm, SurfaceCombineMode, SurfaceLevelsDialog,
    SurfaceLevelsForm, SurfaceLevelsKind, surface_combine_line, surface_levels_line,
};
use katana_window::terrain::surface_edit_ui::SurfaceEditUi;
use katana_window::tools::{ToolMenuTargets, fill_tool_menus};

use super::support::{Bench, Drawn, add_surfaces, ran_host};

/// The levels dialog of `kind` on a drawing with the surfaces `names`, shown.
fn levels_bench(kind: SurfaceLevelsKind, names: &[&str]) -> Bench<SurfaceLevelsDialog> {
    let mut host = ran_host();
    add_surfaces(&mut host, names);
    let mut dialog = SurfaceLevelsDialog::new(kind, &mut host);
    dialog.show(&host);
    let target = String::from(dialog.object_name());
    Bench::new(&target, dialog, host)
}

fn combine_bench(names: &[&str]) -> Bench<SurfaceCombineDialog> {
    let mut host = ran_host();
    add_surfaces(&mut host, names);
    let mut dialog = SurfaceCombineDialog::new(&mut host);
    dialog.show(&host);
    Bench::new("surfaceCombineDialog", dialog, host)
}

/// The refusal's message, or the line that was written instead.
fn error(line: Result<String>) -> String {
    match line {
        Ok(line) => format!("no error: {line}"),
        Err(error) => error.message,
    }
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| String::from(*word)).collect()
}

/// The toolbar's list of the active surface, drawn on its own in the window.
struct ActiveBar(SurfaceEditUi);

impl Drawn for ActiveBar {
    fn draw(&mut self, ui: &mut egui::Ui, host: &mut RecordingHost) {
        self.0.active_ui(ui, host);
    }
}

mod surface_edit_lines {
    use super::*;

    #[test]
    fn the_levels_fields_write_the_line() {
        let mut form = SurfaceLevelsForm {
            surface: String::from("ground"),
            operation: 0,
            value: String::from("1.5"),
            ..SurfaceLevelsForm::default()
        };
        assert_eq!(
            surface_levels_line(SurfaceLevelsKind::Heights, &form).unwrap(),
            "SURFACE EDIT ground RAISE 1.5"
        );
        form.operation = 3;
        form.value = String::from("2");
        form.base = String::from("100");
        form.inside = true;
        form.scope = String::from("LAYERS pads ONLY");
        assert_eq!(
            surface_levels_line(SurfaceLevelsKind::Heights, &form).unwrap(),
            "SURFACE EDIT ground SCALE 2 base=100 INSIDE FROM LAYERS pads ONLY"
        );
        form.surface = String::from("my ground");
        form.passes = 4;
        form.weight = String::from("0.25");
        form.inside = false;
        assert_eq!(
            surface_levels_line(SurfaceLevelsKind::Smooth, &form).unwrap(),
            "SURFACE EDIT \"my ground\" SMOOTH passes=4 weight=0.25"
        );
        form.surface = String::from("ground");
        form.value = String::from("0.02");
        assert_eq!(
            surface_levels_line(SurfaceLevelsKind::Thin, &form).unwrap(),
            "SURFACE EDIT ground THIN 0.02"
        );
        form.value = String::from("40");
        assert_eq!(
            surface_levels_line(SurfaceLevelsKind::DeleteLong, &form).unwrap(),
            "SURFACE EDIT ground DELETE LONG 40"
        );
        form.long_sides = false;
        form.value = String::from("5");
        assert_eq!(
            surface_levels_line(SurfaceLevelsKind::DeleteLong, &form).unwrap(),
            "SURFACE EDIT ground DELETE THIN 5"
        );
    }

    #[test]
    fn what_cannot_be_written_is_refused_naming_the_field() {
        let mut form = SurfaceLevelsForm {
            value: String::from("1"),
            ..SurfaceLevelsForm::default()
        };
        assert!(error(surface_levels_line(SurfaceLevelsKind::Heights, &form)).contains("Surface"));
        form.surface = String::from("ground");
        form.value = String::from("up");
        assert!(error(surface_levels_line(SurfaceLevelsKind::Heights, &form)).contains("By"));
        form.value = String::from("-1");
        assert!(error(surface_levels_line(SurfaceLevelsKind::Thin, &form)).contains("Tolerance"));
        form.weight = String::from("1.5");
        assert!(error(surface_levels_line(SurfaceLevelsKind::Smooth, &form)).contains("Weight"));
        form.weight = String::from("0.5");
        form.inside = true;
        form.scope_error = String::from("no layer is ticked");
        assert!(
            error(surface_levels_line(SurfaceLevelsKind::Smooth, &form)).contains("Only inside")
        );
    }

    #[test]
    fn the_data_dialogs_write_their_lines_from_the_scope_and_refuse_without_one() {
        // Delete points inside, delete triangles inside or outside, and add points are what the
        // one-pick tools cannot say: any number of polygons, or the levelled points, from a scope.
        // The scope is always asked.
        let mut form = SurfaceLevelsForm {
            surface: String::from("ground"),
            inside: true,
            scope: String::from("LAYERS pads ONLY"),
            ..SurfaceLevelsForm::default()
        };
        assert_eq!(
            surface_levels_line(SurfaceLevelsKind::DeletePoints, &form).unwrap(),
            "SURFACE EDIT ground DELETE POINTS INSIDE FROM LAYERS pads ONLY"
        );
        assert_eq!(
            surface_levels_line(SurfaceLevelsKind::DeleteTriangles, &form).unwrap(),
            "SURFACE EDIT ground DELETE TRIANGLES INSIDE FROM LAYERS pads ONLY"
        );
        form.outside = true;
        assert_eq!(
            surface_levels_line(SurfaceLevelsKind::DeleteTriangles, &form).unwrap(),
            "SURFACE EDIT ground DELETE TRIANGLES OUTSIDE FROM LAYERS pads ONLY"
        );
        form.surface = String::from("my ground");
        form.scope = String::from("DRAWING WHERE TYPE=polyline");
        assert_eq!(
            surface_levels_line(SurfaceLevelsKind::AddPoints, &form).unwrap(),
            "SURFACE EDIT \"my ground\" ADD POINTS FROM DRAWING WHERE TYPE=polyline"
        );
        assert_eq!(
            surface_levels_line(SurfaceLevelsKind::DeleteBreakline, &form).unwrap(),
            "SURFACE EDIT \"my ground\" DELETE BREAKLINE FROM DRAWING WHERE TYPE=polyline"
        );
        form.scope.clear();
        assert!(
            error(surface_levels_line(SurfaceLevelsKind::DeletePoints, &form)).contains("Polygons")
        );
        assert!(error(surface_levels_line(SurfaceLevelsKind::AddPoints, &form)).contains("Points"));
        assert!(
            error(surface_levels_line(
                SurfaceLevelsKind::DeleteBreakline,
                &form
            ))
            .contains("Lines: say which lines")
        );
        form.scope_error = String::from("no layer is ticked");
        assert!(
            error(surface_levels_line(
                SurfaceLevelsKind::DeleteTriangles,
                &form
            ))
            .contains("no layer is ticked")
        );
        form.surface.clear();
        assert!(
            error(surface_levels_line(SurfaceLevelsKind::AddPoints, &form)).contains("Surface")
        );
    }

    #[test]
    fn the_combine_fields_write_the_line() {
        let mut form = SurfaceCombineForm {
            merge: strings(&["ground", "pad"]),
            name: String::from("merged"),
            ..SurfaceCombineForm::default()
        };
        assert_eq!(
            surface_combine_line(&form).unwrap(),
            "SURFACE MERGE ground pad NAME merged"
        );
        form.mode = SurfaceCombineMode::Paste;
        form.top = String::from("pad");
        form.base = String::from("ground");
        form.name.clear();
        assert_eq!(
            surface_combine_line(&form).unwrap(),
            "SURFACE PASTE pad INTO ground"
        );
        form.name = String::from("design");
        assert_eq!(
            surface_combine_line(&form).unwrap(),
            "SURFACE PASTE pad INTO ground NAME design"
        );
        form.mode = SurfaceCombineMode::Clip;
        form.surface = String::from("ground");
        form.scope = String::from("SELECTION");
        form.name = String::from("part");
        assert_eq!(
            surface_combine_line(&form).unwrap(),
            "SURFACE CLIP ground FROM SELECTION NAME part"
        );
        form.mode = SurfaceCombineMode::Offset;
        form.dz = String::from("-0.3");
        form.name = String::from("subgrade");
        assert_eq!(
            surface_combine_line(&form).unwrap(),
            "SURFACE OFFSET ground -0.3 NAME subgrade"
        );
    }

    #[test]
    fn a_combine_that_cannot_be_written_is_refused_naming_the_field() {
        let mut form = SurfaceCombineForm {
            merge: strings(&["ground"]),
            name: String::from("merged"),
            ..SurfaceCombineForm::default()
        };
        assert!(error(surface_combine_line(&form)).contains("Surfaces"));
        form.merge = strings(&["ground", "pad"]);
        form.name.clear();
        assert!(error(surface_combine_line(&form)).contains("New surface"));
        form.mode = SurfaceCombineMode::Paste;
        form.top = String::from("ground");
        form.base = String::from("GROUND");
        assert!(error(surface_combine_line(&form)).contains("Top"));
        form.mode = SurfaceCombineMode::Offset;
        form.surface = String::from("ground");
        form.dz = String::from("up");
        form.name = String::from("x");
        assert!(error(surface_combine_line(&form)).contains("Raise by"));
    }
}

mod surface_levels_dialog {
    use super::*;

    #[test]
    fn every_kind_has_its_named_controls_and_run_runs_the_line_shown() {
        let mut heights = levels_bench(SurfaceLevelsKind::Heights, &["ground"]);
        assert_eq!(heights.dialog.object_name(), "surfaceHeightsDialog");
        for name in [
            "surfaceHeightsSurface",
            "surfaceHeightsOperation",
            "surfaceHeightsValue",
            "surfaceHeightsBase",
            "surfaceHeightsInside",
            "surfaceHeightsScope",
            "surfaceHeightsCommand",
            "surfaceHeightsPreview",
            "surfaceHeightsRun",
            "surfaceHeightsReply",
        ] {
            assert!(heights.has(name), "{name}");
        }
        assert_eq!(
            heights.text("surfaceHeightsCommand"),
            "SURFACE EDIT ground RAISE 1"
        );
        // The item at index 2.
        heights.fill("surfaceHeightsOperation", "Set Level");
        heights.fill("surfaceHeightsValue", "12.5");
        heights.press("surfaceHeightsRun");
        assert_eq!(heights.host.lines().len(), 1);
        assert_eq!(
            heights.host.lines()[0],
            "SURFACE EDIT ground SETLEVEL 12.5"
        );
        assert_eq!(
            heights.text("surfaceHeightsReply"),
            "ran SURFACE EDIT ground SETLEVEL 12.5"
        );
        heights.press("surfaceHeightsPreview");
        assert_eq!(
            heights.last_line(),
            "SURFACE EDIT ground SETLEVEL 12.5 PREVIEW"
        );
        // Only inside: the shared scope widget writes the polygon's scope.
        heights.tick("surfaceHeightsInside", true);
        assert!(heights.enabled("surfaceHeightsScope"));
        heights.press("surfaceHeightsScopeDrawing");
        assert_eq!(
            heights.text("surfaceHeightsCommand"),
            "SURFACE EDIT ground SETLEVEL 12.5 INSIDE FROM DRAWING"
        );

        let mut smooth = levels_bench(SurfaceLevelsKind::Smooth, &["ground"]);
        assert_eq!(smooth.dialog.object_name(), "surfaceSmoothDialog");
        smooth.fill("surfaceSmoothPasses", "5");
        assert_eq!(
            smooth.text("surfaceSmoothCommand"),
            "SURFACE EDIT ground SMOOTH passes=5 weight=0.5"
        );

        let thin = levels_bench(SurfaceLevelsKind::Thin, &["ground"]);
        assert_eq!(thin.dialog.object_name(), "surfaceThinDialog");
        assert_eq!(
            thin.text("surfaceThinCommand"),
            "SURFACE EDIT ground THIN 0.01"
        );

        let mut delete_long = levels_bench(SurfaceLevelsKind::DeleteLong, &["ground"]);
        assert_eq!(delete_long.dialog.object_name(), "surfaceDeleteLongDialog");
        // The item at index 1.
        delete_long.fill(
            "surfaceDeleteLongMeasure",
            "An angle smaller than (degrees)",
        );
        delete_long.fill("surfaceDeleteLongValue", "10");
        assert_eq!(
            delete_long.text("surfaceDeleteLongCommand"),
            "SURFACE EDIT ground DELETE THIN 10"
        );
    }

    #[test]
    fn the_data_dialogs_ask_the_scope_always_and_run_the_line_shown() {
        let cases = [
            (
                SurfaceLevelsKind::DeletePoints,
                "surfaceDeletePoints",
                "SURFACE EDIT ground DELETE POINTS INSIDE FROM DRAWING",
            ),
            (
                SurfaceLevelsKind::DeleteTriangles,
                "surfaceDeleteTriangles",
                "SURFACE EDIT ground DELETE TRIANGLES INSIDE FROM DRAWING",
            ),
            (
                SurfaceLevelsKind::AddPoints,
                "surfaceAddPoints",
                "SURFACE EDIT ground ADD POINTS FROM DRAWING",
            ),
            (
                SurfaceLevelsKind::DeleteBreakline,
                "surfaceDeleteBreakline",
                "SURFACE EDIT ground DELETE BREAKLINE FROM DRAWING",
            ),
        ];
        for (kind, p, line) in cases {
            let mut bench = levels_bench(kind, &["ground"]);
            assert_eq!(bench.dialog.object_name(), format!("{p}Dialog"));
            for suffix in ["Surface", "Scope", "Command", "Preview", "Run", "Reply"] {
                assert!(bench.has(&format!("{p}{suffix}")), "{p}{suffix}");
            }
            // No tick box in front of the scope: it is what the dialog is about.
            assert!(!bench.has(&format!("{p}Inside")), "{p}");
            assert!(bench.enabled(&format!("{p}Scope")), "{p}");
            bench.press(&format!("{p}ScopeDrawing"));
            assert_eq!(bench.text(&format!("{p}Command")), line);
            bench.press(&format!("{p}Run"));
            assert_eq!(bench.host.lines().len(), 1, "{p}");
            assert_eq!(bench.host.lines()[0], line);
            bench.press(&format!("{p}Preview"));
            assert_eq!(bench.last_line(), format!("{line} PREVIEW"));
        }
        // Delete triangles: Outside keeps the union of the polygons.
        let mut triangles = levels_bench(SurfaceLevelsKind::DeleteTriangles, &["ground"]);
        assert!(triangles.has("surfaceDeleteTrianglesSide"));
        // The item at index 1.
        triangles.fill(
            "surfaceDeleteTrianglesSide",
            "Outside the polygons (keep what is inside)",
        );
        triangles.press("surfaceDeleteTrianglesScopeDrawing");
        assert_eq!(
            triangles.text("surfaceDeleteTrianglesCommand"),
            "SURFACE EDIT ground DELETE TRIANGLES OUTSIDE FROM DRAWING"
        );
    }

    #[test]
    fn with_no_surface_run_is_off_and_the_command_says_why() {
        let bench = levels_bench(SurfaceLevelsKind::Thin, &[]);
        assert!(!bench.enabled("surfaceThinRun"));
        assert!(
            bench
                .control("surfaceThinCommand")
                .text
                .contains("Surface")
        );
        assert!(bench.host.lines().is_empty());
    }

    #[test]
    fn the_surface_list_follows_the_drawing_while_the_dialog_stays_open() {
        // The dialog is non-modal and kept. An UNDO that removes the surface it offers used to
        // leave it offering it, with Run enabled on a line that the verb refused (NotFound); a
        // surface made while it was open could not be chosen until it was closed and shown again.
        let mut bench = levels_bench(SurfaceLevelsKind::Heights, &["ground"]);
        assert_eq!(
            bench.text("surfaceHeightsCommand"),
            "SURFACE EDIT ground RAISE 1"
        );
        bench
            .host
            .document_mut()
            .undo()
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        bench.draw();
        assert_eq!(bench.items("surfaceHeightsSurface").len(), 0);
        assert!(!bench.enabled("surfaceHeightsRun"));
        assert!(bench.text("surfaceHeightsCommand").is_empty());

        add_surfaces(&mut bench.host, &["design"]);
        bench.draw();
        assert_eq!(bench.items("surfaceHeightsSurface"), ["design"]);
        assert_eq!(
            bench.text("surfaceHeightsCommand"),
            "SURFACE EDIT design RAISE 1"
        );
        assert!(bench.enabled("surfaceHeightsRun"));

        // A surface added beside the chosen one is offered and the choice stays.
        add_surfaces(&mut bench.host, &["pad"]);
        bench.draw();
        assert_eq!(bench.items("surfaceHeightsSurface").len(), 2);
        assert_eq!(bench.value("surfaceHeightsSurface"), "design");
        // Following the drawing runs nothing.
        assert!(bench.host.lines().is_empty());
    }
}

mod surface_combine_dialog {
    use super::*;

    #[test]
    fn each_mode_writes_its_line_and_run_runs_it() {
        let mut bench = combine_bench(&["ground", "pad"]);
        assert!(bench.harness.target_record("surfaceCombineDialog").is_some());
        for name in [
            "surfaceCombineMode",
            "surfaceCombineMergeList",
            "surfaceCombineUp",
            "surfaceCombineDown",
            "surfaceCombineTop",
            "surfaceCombineBase",
            "surfaceCombineSource",
            "surfaceCombineDz",
            "surfaceCombineScope",
            "surfaceCombineName",
            "surfaceCombineCommand",
            "surfaceCombinePreview",
            "surfaceCombineRun",
            "surfaceCombineReply",
        ] {
            assert!(bench.has(name), "{name}");
        }
        // Merge: tick both (each item's setCheckState), the pad last so it wins.
        assert_eq!(bench.items("surfaceCombineMergeList").len(), 2);
        bench.dialog.set_merge_ticked(0, true);
        bench.dialog.set_merge_ticked(1, true);
        bench.draw();
        bench.fill("surfaceCombineName", "merged");
        assert_eq!(
            bench.text("surfaceCombineCommand"),
            "SURFACE MERGE ground pad NAME merged"
        );
        // Down moves the chosen one later in the order: the ground now wins.
        bench.dialog.set_merge_current(0);
        bench.press("surfaceCombineDown");
        assert_eq!(
            bench.text("surfaceCombineCommand"),
            "SURFACE MERGE pad ground NAME merged"
        );
        // Paste in place: the top is the first surface, the base the other.
        bench.dialog.set_mode(SurfaceCombineMode::Paste);
        bench.draw();
        bench.fill("surfaceCombineName", "");
        bench.fill("surfaceCombineTop", "pad");
        bench.fill("surfaceCombineBase", "ground");
        bench.press("surfaceCombineRun");
        assert!(!bench.host.lines().is_empty());
        assert_eq!(bench.last_line(), "SURFACE PASTE pad INTO ground");

        bench.dialog.set_mode(SurfaceCombineMode::Offset);
        bench.draw();
        bench.fill("surfaceCombineSource", "ground");
        bench.fill("surfaceCombineDz", "-0.5");
        bench.fill("surfaceCombineName", "subgrade");
        bench.press("surfaceCombinePreview");
        assert_eq!(
            bench.last_line(),
            "SURFACE OFFSET ground -0.5 NAME subgrade PREVIEW"
        );
    }

    #[test]
    fn the_lists_follow_the_drawing_while_the_dialog_stays_open() {
        let mut bench = combine_bench(&["ground", "pad"]);
        assert_eq!(bench.items("surfaceCombineMergeList").len(), 2);
        assert_eq!(bench.items("surfaceCombineTop").len(), 2);
        bench
            .host
            .document_mut()
            .undo()
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        bench.draw();
        assert_eq!(bench.items("surfaceCombineMergeList").len(), 0);
        assert_eq!(bench.items("surfaceCombineTop").len(), 0);
        add_surfaces(&mut bench.host, &["design", "road"]);
        bench.draw();
        assert_eq!(bench.items("surfaceCombineMergeList").len(), 2);
        assert!(
            bench
                .items("surfaceCombineTop")
                .iter()
                .any(|name| name == "road")
        );
        assert!(bench.host.lines().is_empty());
    }
}

mod surface_edit_ui {
    use super::*;

    #[test]
    fn the_submenu_toolbar_and_dialog_items_are_named_and_the_tools_fill_them() {
        let mut host = ran_host();
        add_surfaces(&mut host, &["ground", "pad"]);
        set_surface_edit_target(String::new());
        let mut ui = SurfaceEditUi::new(&host);
        assert_eq!(ui.menu().name, "terrainEditMenu");
        assert_eq!(ui.tool_bar().name, "SurfaceEditToolBar");

        // The window hands the catalogue this menu and toolbar for "Terrain".
        let mut targets = ToolMenuTargets::default();
        targets
            .menus
            .insert(String::from("Terrain"), ui.menu().clone());
        targets
            .tool_bars
            .insert(String::from("Terrain"), ui.tool_bar().clone());
        let actions = fill_tool_menus(tool_catalog(), &mut targets);
        // The other categories have no target in this test and are left out; Terrain's tools all
        // went where the window puts them.
        for category in actions.unplaced() {
            assert_ne!(category, "Terrain");
        }
        let mut terrain = Menu::new("terrainMenu", "Terrain");
        let menu = targets.menus.remove("Terrain").expect("the menu is back");
        let tool_bar = targets
            .tool_bars
            .remove("Terrain")
            .expect("the toolbar is back");
        ui.finish(menu, tool_bar, &mut terrain);

        let names: Vec<String> = ui
            .menu()
            .actions()
            .iter()
            .map(|action| action.name.clone())
            .collect();
        for name in [
            "terrain.edit.deletetriangle",
            "terrain.edit.flip",
            "terrain.edit.addpoint",
            "terrain.edit.deletepoint",
            "terrain.edit.movepoint",
            "terrain.edit.deleteinside",
            "terrain.edit.breakline",
            "terrain.edit.boundary",
            "terrain.edit.hole",
            "terrainSurfaceHeights",
            "terrainSurfaceSmooth",
            "terrainSurfaceThin",
            "terrainSurfaceDeleteLong",
            "terrainSurfaceDeletePoints",
            "terrainSurfaceDeleteTriangles",
            "terrainSurfaceAddPoints",
            "terrainSurfaceDeleteBreakline",
        ] {
            assert!(names.iter().any(|listed| listed == name), "{name}");
        }
        for name in [
            "terrainSurfaceDeletePoints",
            "terrainSurfaceDeleteTriangles",
            "terrainSurfaceAddPoints",
            "terrainSurfaceDeleteBreakline",
        ] {
            let action = ui.action(name).unwrap_or_else(|| panic!("no {name}"));
            assert!(!action.tip.is_empty(), "{name}");
            assert!(!action.icon.is_empty(), "{name}");
            assert!(
                action
                    .dialog
                    .as_deref()
                    .is_some_and(|dialog| dialog.ends_with("Dialog")),
                "{name}"
            );
        }
        let heights = ui
            .action("terrainSurfaceHeights")
            .expect("the levels item is there");
        assert_eq!(heights.dialog.as_deref(), Some("surfaceHeightsDialog"));
        assert!(ui.action("terrainCombineSurfaces").is_some());
        assert!(terrain.items.iter().any(
            |item| matches!(item, MenuItem::Menu(menu) if menu.name == "terrainEditMenu")
        ));

        // The toolbar's list is the drawing's surfaces; choosing one points the tools at it.
        set_surface_edit_target(String::new());
        let mut bench = Bench::new(WINDOW, ActiveBar(ui), host);
        assert_eq!(bench.items("surfaceEditActive").len(), 2);
        assert_eq!(bench.value("surfaceEditActive"), "ground");
        bench.fill("surfaceEditActive", "pad");
        assert_eq!(surface_edit_target(), "pad");
        assert_eq!(
            active_edit_surface(bench.host.document()).expect("the drawing has surfaces"),
            "pad"
        );
        // A tool's own [Surface] option sets the target and the list follows it: the toolbar kept
        // naming the old surface while every tool edited the new one.
        set_surface_edit_target(String::from("ground"));
        bench.draw();
        assert_eq!(bench.value("surfaceEditActive"), "ground");
        set_surface_edit_target(String::from("pad"));
        bench.draw();
        assert_eq!(bench.value("surfaceEditActive"), "pad");
        assert_eq!(
            surface_edit_target(),
            "pad",
            "following the target does not set it again"
        );
        // And a choice made by the program, as the headless driver's fill makes it, points the
        // tools too.
        bench.fill("surfaceEditActive", "ground");
        assert_eq!(surface_edit_target(), "ground");
        set_surface_edit_target(String::new());
    }
}
