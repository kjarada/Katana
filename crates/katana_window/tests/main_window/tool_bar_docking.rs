//! Toolbars docked along any side or floating, and menus torn off into panels (main_window::
//! docking and docking_ui; the owner's ask of 2026-11-19, "make toolbars and menus resizable and
//! dockable"): each move by its action - what View > Toolbars, a bar's right-click menu and a `*`
//! step trigger - and by a drag of the grip with the pointer; the layout saved with every place and
//! restored, an older layout carried over; a torn-off menu's panel holding the menu's items.

use egui::{Event, Modifiers, PointerButton, Pos2, Rect};
use katana_window::MainWindow;
use katana_window::drive::Posted;
use katana_window::drive::headless::Headless;
use katana_window::drive::DrivenWindow;
use katana_window::main_window::actions::BarPlace;
use katana_window::main_window::docking::{
    DropSpot, DropZones, FloatAt, FlowItem, PanelPlace, Side, apply_drop, drop_spot,
    flow_columns, normalise, panel_drop_side, parse_action, DockingAction,
};
use katana_window::main_window::docking_ui::{GRIP_DOT, grip_dots};
use katana_window::main_window::layout;
use katana_window::tools::flyout_button::{
    StripItem, balanced_wrap_width, separator_cell, separator_rule, wrap_strip,
};

/// The window drawn until it is idle, and the runner that drew it.
fn settled(window: &mut MainWindow) -> Headless {
    let mut headless = Headless::new();
    headless
        .settle(window)
        .unwrap_or_else(|problem| panic!("{problem}"));
    headless
}

fn place_of(window: &MainWindow, bar: &str) -> BarPlace {
    window
        .bar_place(bar)
        .unwrap_or_else(|| panic!("the window has a toolbar {bar}"))
}

fn drawn(headless: &Headless, target: &str) -> Rect {
    headless
        .snapshot()
        .target(target)
        .unwrap_or_else(|| panic!("{target} is drawn"))
        .rect
}

/// The pointer pressed at `from`, moved to `to` over a few frames, and let go there.
fn drag(window: &mut MainWindow, headless: &mut Headless, from: Pos2, to: Pos2) {
    headless.push_event(Event::PointerMoved(from));
    let _ = headless.frame(window);
    headless.push_event(Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    let _ = headless.frame(window);
    for step in 1..=4 {
        let at = from + (to - from) * (step as f32 / 4.0);
        headless.push_event(Event::PointerMoved(at));
        let _ = headless.frame(window);
    }
    headless.push_event(Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    headless
        .settle(window)
        .unwrap_or_else(|problem| panic!("{problem}"));
}

mod tool_bar_docking {
    use super::*;

    #[test]
    fn a_bar_docked_to_each_side_lands_there_runs_along_it_and_keeps_its_actions_by_name() {
        for side in [Side::Left, Side::Right, Side::Bottom, Side::Top] {
            let mut window = MainWindow::new();
            window
                .trigger(&format!("viewToolBarDockSurvey{}", side.title()))
                .unwrap_or_else(|error| panic!("{}: {}", side.word(), error.describe()));
            assert_eq!(
                place_of(&window, "SurveyToolBar").docked().map(|(at, _)| at),
                Some(side),
                "docked {}",
                side.word()
            );
            let headless = settled(&mut window);
            let bar = drawn(&headless, "SurveyToolBar");
            let views = window.ui.docking.drawing;
            match side {
                Side::Left => assert!(bar.max.x <= views.min.x, "{bar:?} left of {views:?}"),
                Side::Right => assert!(bar.min.x >= views.max.x, "{bar:?} right of {views:?}"),
                Side::Bottom => assert!(bar.min.y >= views.max.y, "{bar:?} under {views:?}"),
                Side::Top => assert!(bar.max.y <= views.min.y, "{bar:?} over {views:?}"),
            }
            // Its orientation follows its side.
            assert_eq!(
                bar.height() > bar.width(),
                side.vertical(),
                "{} runs along its side: {bar:?}",
                side.word()
            );
            // The item that docked it is checked, and every action of the bar is still one a
            // name reaches: triggered by name, and reported.
            let composed = window.composed();
            let item = composed
                .action(&format!("viewToolBarDockSurvey{}", side.title()))
                .expect("View > Toolbars > Place > Survey has the item");
            assert_eq!(item.checked, Some(true));
            let survey = composed
                .bars
                .iter()
                .find(|(bar, _)| bar.name == "SurveyToolBar")
                .map(|(bar, _)| bar.clone())
                .expect("the Survey toolbar");
            assert!(!survey.actions().is_empty());
            for action in survey.actions() {
                assert!(
                    composed.action(&action.name).is_some(),
                    "{} is reachable by name",
                    action.name
                );
            }
        }
    }

    #[test]
    fn a_bar_floated_shows_every_button_over_the_views_and_docks_again() {
        let mut window = MainWindow::new();
        window.trigger("viewToolBarDockGISFloat").expect("floated");
        assert!(matches!(place_of(&window, "GISToolBar"), BarPlace::Floating(_)));
        let headless = settled(&mut window);
        let bar = drawn(&headless, "GISToolBar");
        let views = window.ui.docking.drawing;
        assert!(views.contains_rect(bar), "{bar:?} floats over the views {views:?}");
        let frame = headless.snapshot();
        let gis = window
            .composed()
            .bars
            .iter()
            .find(|(bar, _)| bar.name == "GISToolBar")
            .map(|(bar, _)| bar.clone())
            .expect("the GIS toolbar");
        for action in gis.actions() {
            let control = frame
                .control("GISToolBar", &action.name)
                .unwrap_or_else(|| panic!("{} is drawn on the floating bar", action.name));
            assert!(control.visible, "{} is not behind an arrow", action.name);
            assert!(bar.contains_rect(control.rect));
        }
        // Its grip is a named control, and its frame is over its buttons.
        assert!(frame.control_anywhere("GISToolBarGrip").is_some());
        window.trigger("viewToolBarDockGISTop").expect("docked");
        assert!(matches!(place_of(&window, "GISToolBar"), BarPlace::Top(_)));
        let headless = settled(&mut window);
        assert!(drawn(&headless, "GISToolBar").max.y <= window.ui.docking.drawing.min.y);
    }

    #[test]
    fn a_floating_bar_narrowed_wraps_its_buttons_into_rows_and_hides_none() {
        let mut window = MainWindow::new();
        window
            .trigger("viewToolBarPlace:GISToolBar:float:400,300,400")
            .expect("floated wide");
        let wide = drawn(&settled(&mut window), "GISToolBar");
        window
            .trigger("viewToolBarPlace:GISToolBar:float:400,300,90")
            .expect("floated narrow");
        let headless = settled(&mut window);
        let narrow = drawn(&headless, "GISToolBar");
        assert!(narrow.width() < wide.width());
        assert!(
            narrow.height() > wide.height() + 30.0,
            "two buttons a row take more rows: {narrow:?} against {wide:?}"
        );
        let frame = headless.snapshot();
        let shown = frame
            .in_target("GISToolBar")
            .filter(|control| control.visible && narrow.contains_rect(control.rect))
            .count();
        assert!(shown >= 7, "every button is drawn inside the bar: {shown}");
    }

    #[test]
    fn a_bar_placed_first_on_its_row_is_drawn_first_and_a_new_row_moves_the_views_down() {
        let mut window = MainWindow::new();
        let headless = settled(&mut window);
        let file = drawn(&headless, "FileToolBar");
        let views = window.ui.docking.drawing;
        window
            .trigger("viewToolBarPlace:EditToolBar:top:0:0")
            .expect("placed");
        let headless = settled(&mut window);
        assert!(drawn(&headless, "EditToolBar").min.x < drawn(&headless, "FileToolBar").min.x);
        assert!(drawn(&headless, "FileToolBar").min.x > file.min.x);
        // BIM on a fourth row: the views give it a row's height.
        window
            .trigger("viewToolBarPlace:BIMToolBar:top:3:0")
            .expect("a row of its own");
        assert_eq!(place_of(&window, "BIMToolBar"), BarPlace::Top(3));
        settled(&mut window);
        assert_eq!(
            window.ui.docking.drawing.min.y,
            views.min.y + layout::TOOL_BAR_ROW
        );
        // Surface Edit alone on the third row, moved to the second: the third closes up.
        window
            .trigger("viewToolBarPlace:SurfaceEditToolBar:top:1")
            .expect("moved");
        assert_eq!(place_of(&window, "BIMToolBar"), BarPlace::Top(2));
    }

    #[test]
    fn reset_toolbars_puts_every_bar_back_where_a_new_window_has_it_and_shows_it() {
        let mut window = MainWindow::new();
        let built = window.bar_places();
        for action in [
            "viewToolBarDockDrawRight",
            "viewToolBarDockGISFloat",
            "viewToolBarDockFileBottom",
            "viewToolBarModify",
        ] {
            window.trigger(action).expect(action);
        }
        assert_ne!(window.bar_places(), built);
        window.trigger("viewToolBarsReset").expect("reset");
        assert_eq!(window.bar_places(), built);
        assert!(window.ui.bars.shown("ModifyToolBar"));
        // Reset Window Layout does it too.
        window.trigger("viewToolBarDockDrawRight").expect("moved");
        window.trigger("viewMenuTearOffViewMenu").expect("torn off");
        window.trigger("viewResetLayout").expect("reset");
        assert_eq!(window.bar_places(), built);
        assert!(window.ui.docking.panels.open.is_empty());
    }

    #[test]
    fn the_layout_saved_holds_every_place_and_torn_off_menu_and_restores_them() {
        let mut window = MainWindow::new();
        for action in [
            "viewToolBarDockDrawRight",
            "viewToolBarPlace:GISToolBar:float:500,320,160",
            "viewToolBarDockSurveyBottom",
            "viewMenuTearOffViewMenu",
            "viewMenuPanelDockViewMenuLeft",
            "viewMenuTearOffDrawMenu",
            "viewMenuPanelPlace:drawMenu:float:700,200,240,300",
            "viewMenuBand:left:300",
        ] {
            window.trigger(action).expect(action);
        }
        let text = layout::layout_text(&window);
        let mut restored = MainWindow::new();
        assert!(layout::restore_layout(&mut restored, &text), "{text}");
        assert_eq!(restored.bar_places(), window.bar_places());
        assert_eq!(restored.ui.docking.panels, window.ui.docking.panels);
        assert_eq!(
            place_of(&restored, "GISToolBar"),
            BarPlace::Floating(FloatAt {
                x: 500.0,
                y: 320.0,
                width: 160.0
            })
        );
        assert_eq!(restored.ui.docking.panels.bands[Side::Left.index()], 300.0);
        // Drawn, the restored window shows the panels where they were saved.
        let headless = settled(&mut restored);
        assert!(headless.snapshot().target("viewMenuTornOff").is_some());
        assert!(headless.snapshot().target("drawMenuTornOff").is_some());
    }

    #[test]
    fn a_version_2_layout_restores_its_rows_and_every_other_bar_where_the_window_builds_it() {
        // What every build of 2026-10-15 to 2026-11-18 saved: the top rows alone, Survey moved to
        // the third row and the Edit toolbar hidden.
        let text = "{\"version\":2,\"layers\":true,\"properties\":true,\"commandLine\":true,\
                    \"reference\":true,\"surfaces\":false,\"hidden\":[\"EditToolBar\"],\"rows\":\
                    [\"FileToolBar=0\",\"EditToolBar=0\",\"PropertiesToolBar=0\",\"ViewToolBar=0\",\
                    \"FormatToolBar=0\",\"SurveyToolBar=2\",\"TerrainToolBar=1\",\"GISToolBar=1\",\
                    \"SurfaceEditToolBar=2\",\"BIMToolBar=2\"]}";
        let mut window = MainWindow::new();
        assert!(layout::restore_layout(&mut window, text));
        assert_eq!(place_of(&window, "SurveyToolBar"), BarPlace::Top(2));
        assert_eq!(place_of(&window, "DrawToolBar"), BarPlace::Left(0));
        assert_eq!(place_of(&window, "ModifyToolBar"), BarPlace::Right(0));
        assert!(!window.ui.bars.shown("EditToolBar"));
        // Saved again, it is this version's, and reads back the same.
        let saved = layout::layout_text(&window);
        assert!(saved.contains("\"version\":3"), "{saved}");
        let mut again = MainWindow::new();
        assert!(layout::restore_layout(&mut again, &saved));
        assert_eq!(again.bar_places(), window.bar_places());
    }

    #[test]
    fn a_layout_of_a_later_version_is_refused_and_leaves_the_window_as_it_was() {
        let mut window = MainWindow::new();
        let built = window.bar_places();
        assert!(!layout::restore_layout(
            &mut window,
            "{\"version\":4,\"bars\":[\"DrawToolBar=right:0\"]}"
        ));
        assert_eq!(window.bar_places(), built);
    }

    #[test]
    fn every_toolbar_along_the_top_shows_all_its_actions_at_1280_in_the_default_layout() {
        let mut window = MainWindow::new();
        settled(&mut window);
        let (hiding, bars, rows, widths) = layout::tool_bar_fit(&window, 1280.0);
        assert!(hiding.is_empty(), "{hiding:?} {widths:?}");
        assert_eq!(rows, 3);
        assert_eq!(bars, 10);
    }

    #[test]
    fn a_bar_dragged_by_its_grip_onto_the_right_column_docks_there() {
        let mut window = MainWindow::new();
        let mut headless = settled(&mut window);
        let grip = headless
            .snapshot()
            .control_anywhere("SurveyToolBarGrip")
            .expect("the Survey toolbar's grip is a named control")
            .rect;
        let modify = drawn(&headless, "ModifyToolBar");
        drag(&mut window, &mut headless, grip.center(), modify.center());
        assert_eq!(
            place_of(&window, "SurveyToolBar").docked().map(|(side, _)| side),
            Some(Side::Right)
        );
        let bar = drawn(&headless, "SurveyToolBar");
        assert!(bar.min.x >= window.ui.docking.drawing.max.x);
        assert!(window.ui.docking.bar_drag.is_none(), "the drag is over");
    }

    #[test]
    fn a_bar_dragged_into_the_views_floats_where_it_was_let_go() {
        let mut window = MainWindow::new();
        let mut headless = settled(&mut window);
        let grip = headless
            .snapshot()
            .control_anywhere("TerrainToolBarGrip")
            .expect("the Terrain toolbar's grip")
            .rect;
        let to = window.ui.docking.drawing.center();
        drag(&mut window, &mut headless, grip.center(), to);
        let BarPlace::Floating(at) = place_of(&window, "TerrainToolBar") else {
            panic!("floating: {:?}", place_of(&window, "TerrainToolBar"));
        };
        // Its corner where the grip was, relative to the pointer.
        let grab = grip.center() - drawn_before_corner(&grip);
        assert!((at.x - (to.x - grab.x)).abs() <= 1.0, "{at:?}");
        let bar = drawn(&headless, "TerrainToolBar");
        assert!(bar.contains(to), "{bar:?} holds {to:?}");
    }

    /// The bar's corner a grip at `grip` starts: the grip is its leading padding, at its corner.
    fn drawn_before_corner(grip: &Rect) -> Pos2 {
        grip.min
    }

    #[test]
    fn the_grips_right_click_menu_is_the_bars_place_menu() {
        // The menu a bar's grip opens is its View > Toolbars > Place items, by the same names.
        let mut window = MainWindow::new();
        let composed = window.composed();
        let place = katana_window::drive::steps::find_menu(&composed.menus, "viewToolBarPlaceDraw")
            .expect("View > Toolbars > Place > Draw")
            .clone();
        let names: Vec<String> = place.actions().iter().map(|action| action.name.clone()).collect();
        assert_eq!(
            names,
            [
                "viewToolBarDockDrawTop",
                "viewToolBarDockDrawLeft",
                "viewToolBarDockDrawRight",
                "viewToolBarDockDrawBottom",
                "viewToolBarDockDrawFloat"
            ]
        );
        assert_eq!(
            place
                .actions()
                .iter()
                .filter(|action| action.checked == Some(true))
                .map(|action| action.name.as_str())
                .collect::<Vec<_>>(),
            ["viewToolBarDockDrawLeft"]
        );
        for action in place.actions() {
            assert!(!action.icon.is_empty() && !action.tip.is_empty(), "{}", action.name);
        }
        window.trigger("viewToolBarDockDrawBottom").expect("docked");
        assert_eq!(place_of(&window, "DrawToolBar"), BarPlace::Bottom(0));
    }

    #[test]
    fn a_dock_action_for_no_toolbar_or_a_menu_not_torn_off_is_refused_with_its_reason() {
        let mut window = MainWindow::new();
        let error = window
            .trigger("viewToolBarDockNoSuchBarLeft")
            .expect_err("no such toolbar");
        assert!(error.describe().contains("no toolbar is named"), "{}", error.describe());
        let error = window
            .trigger("viewMenuTearOffNoSuchMenu")
            .expect_err("no such menu");
        assert!(error.describe().contains("no menu is named"), "{}", error.describe());
        let error = window
            .trigger("viewMenuPanelCloseViewMenu")
            .expect_err("not torn off");
        assert!(error.describe().contains("not torn off"), "{}", error.describe());
        // The Drafting bar stays where it is.
        assert!(window.trigger("viewToolBarPlace:DraftingToolBar:left:0").is_err());
    }


    /// Every action of `bar` drawn on it and not behind an arrow.
    fn every_tool_shown(window: &MainWindow, headless: &Headless, bar: &str) {
        let frame = headless.snapshot();
        let composed = window.composed();
        let (toolbar, _) = composed
            .bars
            .iter()
            .find(|(each, _)| each.name == bar)
            .unwrap_or_else(|| panic!("the window has a toolbar {bar}"));
        assert!(!toolbar.actions().is_empty());
        for action in toolbar.actions() {
            let control = frame
                .control(bar, &action.name)
                .unwrap_or_else(|| panic!("{} is drawn on {bar}", action.name));
            assert!(control.visible, "{} of {bar} is not behind an arrow", action.name);
        }
    }

    #[test]
    fn a_bar_docked_to_a_full_side_takes_a_line_of_its_own_and_shows_its_tools() {
        // Modify fills the right column at 1360 x 860, Draw and Annotate the left. Docked to
        // the end of the column already there, Draw was one button long with its tools behind
        // an arrow (drawn=1321,761,39,45, the sixth look audit); as Dock Top does, a side opens
        // a line of its own for a bar that does not fit whole on one it has.
        let mut window = MainWindow::new();
        window.trigger("viewToolBarDockDrawRight").expect("docked right");
        assert_eq!(place_of(&window, "ModifyToolBar"), BarPlace::Right(0));
        assert_eq!(place_of(&window, "DrawToolBar"), BarPlace::Right(1));
        let headless = settled(&mut window);
        let draw = drawn(&headless, "DrawToolBar");
        let modify = drawn(&headless, "ModifyToolBar");
        assert!(draw.height() > 300.0, "Draw has its column's length: {draw:?}");
        assert!(draw.max.x <= modify.min.x + 0.5, "inside Modify's column: {draw:?} {modify:?}");
        // GIS docked left goes beside Draw's old column, now Annotate's, and shows every tool.
        window.trigger("viewToolBarDockGISLeft").expect("docked left");
        let headless = settled(&mut window);
        let gis = place_of(&window, "GISToolBar");
        assert_eq!(gis.docked().map(|(side, _)| side), Some(Side::Left));
        every_tool_shown(&window, &headless, "GISToolBar");
        // A bar that fits whole under another on a side joins its line: Survey docked to the
        // bottom, then Terrain beside it.
        window.trigger("viewToolBarDockSurveyBottom").expect("docked bottom");
        window.trigger("viewToolBarDockTerrainBottom").expect("docked bottom");
        assert_eq!(place_of(&window, "SurveyToolBar"), BarPlace::Bottom(0));
        assert_eq!(place_of(&window, "TerrainToolBar"), BarPlace::Bottom(0));
        let headless = settled(&mut window);
        every_tool_shown(&window, &headless, "TerrainToolBar");
    }

    #[test]
    fn a_broad_bar_docked_down_a_side_takes_a_column_of_its_own_and_narrows_none() {
        // The Surface Edit bar's surface box is 150 points (layout::widget_size); down the left
        // beside Draw a broad bar made the column as broad as itself, Draw's buttons in its first
        // 39 points and a void beside them. This case was the Properties bar's until the seventh
        // look audit, after which that bar no longer docks down a side
        // (`the_properties_bar_is_refused_down_a_side_and_its_side_items_are_greyed`); Surface
        // Edit is the broad bar that still does.
        let mut window = MainWindow::new();
        window.trigger("viewToolBarDockSurfaceEditLeft").expect("docked left");
        assert_eq!(place_of(&window, "DrawToolBar"), BarPlace::Left(0));
        assert_eq!(place_of(&window, "SurfaceEditToolBar"), BarPlace::Left(1));
        let headless = settled(&mut window);
        let draw = drawn(&headless, "DrawToolBar");
        let surface = drawn(&headless, "SurfaceEditToolBar");
        assert!(draw.width() <= layout::SIDE + 0.5, "Draw's column stays narrow: {draw:?}");
        assert!(surface.min.x >= draw.max.x - 0.5, "{surface:?} beside {draw:?}");
        // Placed on Draw's column all the same, by a precise place, each bar is drawn at its own
        // breadth against the window's edge: Draw claims no void beside its buttons.
        window
            .trigger("viewToolBarPlace:SurfaceEditToolBar:left:0")
            .expect("placed");
        let headless = settled(&mut window);
        let draw = drawn(&headless, "DrawToolBar");
        let surface = drawn(&headless, "SurfaceEditToolBar");
        assert!(draw.width() <= layout::SIDE + 0.5, "{draw:?}");
        assert!(surface.width() > 150.0, "{surface:?}");
        assert_eq!(draw.min.x, surface.min.x, "both against the window's edge");
    }

    #[test]
    fn the_properties_bar_is_refused_down_a_side_and_its_side_items_are_greyed() {
        // The seventh look audit: the Properties bar docked left was its 214 point style box over
        // an empty column 218 x 625 points beside the panels. It docks along the top or the
        // bottom, or floats; a side is refused by every door with the reason.
        let mut window = MainWindow::new();
        let before = place_of(&window, "PropertiesToolBar");
        for side in ["Left", "Right"] {
            let error = window
                .trigger(&format!("viewToolBarDockProperties{side}"))
                .expect_err("refused down a side");
            assert!(
                error.describe().contains("docks along the top or the bottom"),
                "{}",
                error.describe()
            );
            assert!(error.describe().contains("PropertiesToolBar"), "{}", error.describe());
        }
        for place in [
            "viewToolBarPlace:PropertiesToolBar:left:0",
            "viewToolBarPlace:PropertiesToolBar:right:1:0",
        ] {
            assert!(window.trigger(place).is_err(), "{place} refused");
        }
        assert_eq!(place_of(&window, "PropertiesToolBar"), before, "and left where it was");
        // View > Toolbars > Place > Properties keeps the four sides, its Left and Right greyed
        // with the reason as their tip; Top, Bottom and Float stay live and still move it.
        let composed = window.composed();
        for side in ["Left", "Right"] {
            let item = composed
                .action(&format!("viewToolBarDockProperties{side}"))
                .expect("the item is still listed");
            assert!(!item.enabled, "{side} greyed");
            assert!(item.tip.contains("docks along the top or the bottom"), "{}", item.tip);
        }
        for side in ["Top", "Bottom", "Float"] {
            let item = composed
                .action(&format!("viewToolBarDockProperties{side}"))
                .expect("the item is listed");
            assert!(item.enabled, "{side} live");
        }
        window.trigger("viewToolBarDockPropertiesBottom").expect("docked along the bottom");
        assert_eq!(
            place_of(&window, "PropertiesToolBar").docked().map(|(side, _)| side),
            Some(Side::Bottom)
        );
        // Every other bar still docks down a side.
        let composed = window.composed();
        let item = composed
            .action("viewToolBarDockSurveyLeft")
            .expect("Survey's Dock Left");
        assert!(item.enabled);
    }

    #[test]
    fn the_properties_bar_dragged_onto_a_side_floats_there_instead() {
        // A drag is the one way to a side that is not an action: over a column, or the strip that
        // opens a new one, the Properties bar floats under the pointer; any other bar docks.
        let left = Rect::from_min_max(Pos2::new(0.0, 143.0), Pos2::new(39.0, 806.0));
        let zones = DropZones {
            lines: vec![(Side::Left, 0, left)],
            bars: Vec::new(),
            new_lines: vec![(
                Side::Left,
                1,
                Rect::from_min_max(Pos2::new(39.0, 143.0), Pos2::new(49.0, 806.0)),
            )],
        };
        for pointer in [Pos2::new(20.0, 400.0), Pos2::new(44.0, 400.0)] {
            let spot = drop_spot(&zones, "PropertiesToolBar", pointer, egui::Vec2::ZERO, 220.0);
            assert!(matches!(spot, DropSpot::Float(_)), "{pointer:?}: {spot:?}");
            let spot = drop_spot(&zones, "SurveyToolBar", pointer, egui::Vec2::ZERO, 220.0);
            assert!(!matches!(spot, DropSpot::Float(_)), "{pointer:?}: {spot:?}");
        }
    }

    #[test]
    fn a_saved_layout_with_the_properties_bar_down_a_side_restores_it_where_the_window_builds_it() {
        let mut window = MainWindow::new();
        let built = place_of(&window, "PropertiesToolBar");
        window.trigger("viewToolBarDockSurveyLeft").expect("docked left");
        let saved = layout::layout_text(&window)
            .replace("\"PropertiesToolBar=top:0\"", "\"PropertiesToolBar=left:0\"");
        assert!(saved.contains("PropertiesToolBar=left:0"), "the premise: {saved}");
        let mut restored = MainWindow::new();
        assert!(layout::restore_layout(&mut restored, &saved));
        assert_eq!(place_of(&restored, "PropertiesToolBar"), built);
        assert_eq!(
            place_of(&restored, "SurveyToolBar").docked().map(|(side, _)| side),
            Some(Side::Left),
            "the rest of the layout restored"
        );
    }

    #[test]
    fn a_grip_is_clear_of_its_bars_name_and_the_windows_edge_and_centred_on_its_buttons() {
        for (icons, size) in [("viewToolBarIconsStandard", 20), ("viewToolBarIconsLarge", 28)] {
            let mut window = MainWindow::new();
            window.trigger(icons).expect(icons);
            assert_eq!(window.ui.bars.icon_size, size);
            let headless = settled(&mut window);
            let frame = headless.snapshot();
            let file = drawn(&headless, "FileToolBar");
            let dots = grip_dots(file.min, layout::bar_breadth(size), false);
            let ink_left = dots.iter().map(|dot| dot.x).fold(f32::MAX, f32::min) - GRIP_DOT;
            let ink_right = dots.iter().map(|dot| dot.x).fold(f32::MIN, f32::max) + GRIP_DOT;
            let name = frame
                .control("FileToolBar", "toolBarName")
                .expect("the File bar shows its name")
                .rect;
            // The dots touched the word ("#File"): a gap of 4 to 6 points now, and clear of the
            // window's edge, which the first bar of a row starts at.
            let gap = name.min.x - ink_right;
            assert!((4.0..=6.0).contains(&gap), "{icons}: {gap} between the grip and File");
            assert_eq!(file.min.x, window.ui.docking.screen.min.x);
            assert!(ink_left - file.min.x >= 1.5, "{icons}: {ink_left} from the edge");
            // Centred on the buttons' middle line, which with large icons is not the line's.
            let button = frame
                .control("FileToolBar", "fileNew")
                .expect("File's New button")
                .rect;
            let middle = dots.iter().map(|dot| dot.y).sum::<f32>() / dots.len() as f32;
            assert!(
                (middle - button.center().y).abs() <= 1.0,
                "{icons}: the grip's middle {middle} against New's {}",
                button.center().y
            );
        }
    }

    #[test]
    fn a_floating_bar_shares_its_buttons_between_its_rows_and_has_no_empty_end() {
        // Terrain floated at the width a menu item floats a bar at was seven buttons over one
        // alone (the sixth look audit): drawn at the least width for its rows, no row is a lone
        // button while another has room to spare.
        let mut window = MainWindow::new();
        window
            .trigger("viewToolBarPlace:TerrainToolBar:float:400,300,320")
            .expect("floated");
        let headless = settled(&mut window);
        let bar = drawn(&headless, "TerrainToolBar");
        assert!(bar.width() <= 320.0, "{bar:?}");
        let frame = headless.snapshot();
        let mut rows: Vec<(f32, usize)> = Vec::new();
        for control in frame
            .in_target("TerrainToolBar")
            .filter(|control| control.visible && bar.contains_rect(control.rect))
            .filter(|control| control.rect.height() > 20.0)
        {
            let y = control.rect.center().y.round();
            match rows.iter_mut().find(|(at, _)| (*at - y).abs() < 2.0) {
                Some((_, count)) => *count += 1,
                None => rows.push((y, 1)),
            }
        }
        rows.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert!(rows.len() >= 2, "the premise: Terrain wraps at 320: {rows:?}");
        let most = rows.iter().map(|(_, count)| *count).max().unwrap_or(0);
        let least = rows.iter().map(|(_, count)| *count).min().unwrap_or(0);
        assert!(most - least <= 2, "rows shared out: {rows:?}");
        every_tool_shown(&window, &headless, "TerrainToolBar");
    }

    #[test]
    fn a_headless_run_reports_where_each_bar_and_torn_off_menu_is() {
        let mut window = MainWindow::new();
        window.trigger("viewToolBarDockDrawRight").expect("docked");
        window.trigger("viewMenuTearOffViewMenu").expect("torn off");
        settled(&mut window);
        // Draw docked right takes a column of its own beside Modify's, which it does not fit
        // under (`a_bar_docked_to_a_full_side_takes_a_line_of_its_own_and_shows_its_tools`): it
        // was put at the end of Modify's column, at=1, until the sixth look audit.
        let draw = window.docking_report("DrawToolBar").expect("a toolbar's report");
        assert!(draw.starts_with("place=right:1 at=0 shown=yes drawn="), "{draw}");
        let layout_report = window.docking_report("toolBarLayout").expect("the layout");
        assert!(layout_report.contains("DrawToolBar=right:1"), "{layout_report}");
        assert!(layout_report.contains("viewMenuTornOff=float:"), "{layout_report}");
        let panel = window.docking_report("viewMenuTornOff").expect("the panel");
        assert!(panel.starts_with("place=float:"), "{panel}");
        assert!(!panel.contains("items=0"), "{panel}");
        assert!(window.docking_report("NoSuchToolBar").is_none());
    }
}

mod torn_off_menus {
    use super::*;

    #[test]
    fn a_torn_off_menu_holds_the_menus_items_and_runs_one_when_it_is_clicked() {
        let mut window = MainWindow::new();
        window.trigger("viewMenuTearOffViewMenu").expect("torn off");
        let mut headless = settled(&mut window);
        let frame = headless.snapshot();
        let panel = frame.target("viewMenuTornOff").expect("its panel is a dock");
        assert_eq!(panel.title, "View");
        let item = frame
            .control("viewMenuTornOff", "viewGrid")
            .expect("View's Grid item is in the panel");
        assert!(item.visible);
        assert!(frame.control("viewMenuTornOff", "DockCloseButton").is_some());
        let grid = window.exec.workspace.grid_visible();
        headless
            .act(&mut window, Posted::press("viewMenuTornOff", "viewGrid"))
            .unwrap_or_else(|problem| panic!("{problem}"));
        assert_ne!(window.exec.workspace.grid_visible(), grid, "Grid was run");
        assert!(
            window.ui.docking.panels.find("viewMenu").is_some(),
            "and the panel stays open"
        );
        // Its Close closes it.
        headless
            .act(&mut window, Posted::press("viewMenuTornOff", "DockCloseButton"))
            .unwrap_or_else(|problem| panic!("{problem}"));
        assert!(window.ui.docking.panels.find("viewMenu").is_none());
    }

    #[test]
    fn the_tear_off_line_at_the_top_of_a_menu_tears_it_off_where_it_was() {
        let mut window = MainWindow::new();
        let mut headless = settled(&mut window);
        assert!(window.open_menu("viewToolBars"));
        headless
            .settle(&mut window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        let line = headless
            .snapshot()
            .control("viewToolBars", "tearOffViewToolBars")
            .expect("the menu has its tear-off line")
            .rect;
        headless
            .act(&mut window, Posted::press("viewToolBars", "tearOffViewToolBars"))
            .unwrap_or_else(|problem| panic!("{problem}"));
        let panel = window
            .ui
            .docking
            .panels
            .find("viewToolBars")
            .expect("torn off")
            .clone();
        let PanelPlace::Floating(rect) = panel.place else {
            panic!("a menu torn off floats: {:?}", panel.place);
        };
        assert_eq!(rect.min, line.min.round(), "where its line was");
        assert!(window.ui.open_menu.is_none(), "and the menu closed");
        let frame = headless.snapshot();
        assert!(frame.control("viewToolBarsTornOff", "viewToolBarNames").is_some());
    }

    #[test]
    fn a_torn_off_menu_docked_to_each_side_of_the_views_takes_a_band_there() {
        for side in Side::ALL {
            let mut window = MainWindow::new();
            let before = {
                settled(&mut window);
                window.ui.docking.drawing
            };
            window.trigger("viewMenuTearOffDrawMenu").expect("torn off");
            window
                .trigger(&format!("viewMenuPanelDockDrawMenu{}", side.title()))
                .expect("docked");
            let headless = settled(&mut window);
            let panel = drawn(&headless, "drawMenuTornOff");
            let views = window.ui.docking.drawing;
            match side {
                Side::Left => assert!(panel.max.x < views.min.x && views.min.x > before.min.x),
                Side::Right => assert!(panel.min.x > views.max.x && views.max.x < before.max.x),
                Side::Top => assert!(panel.max.y < views.min.y && views.min.y > before.min.y),
                Side::Bottom => assert!(panel.min.y > views.max.y && views.max.y < before.max.y),
            }
            // Its band's seam is a named control that sets the band's breadth.
            assert!(
                headless
                    .snapshot()
                    .control_anywhere(&format!("menuBandSeam{}", side.title()))
                    .is_some()
            );
        }
    }

    #[test]
    fn a_torn_off_menu_dragged_by_its_title_bar_to_a_side_of_the_views_docks_there() {
        let mut window = MainWindow::new();
        window.trigger("viewMenuTearOffDrawMenu").expect("torn off");
        let mut headless = settled(&mut window);
        let panel = drawn(&headless, "drawMenuTornOff");
        let views = window.ui.docking.views;
        let title = Pos2::new(panel.min.x + 60.0, panel.min.y + 10.0);
        let to = Pos2::new(views.max.x - 10.0, views.center().y);
        drag(&mut window, &mut headless, title, to);
        assert_eq!(
            window.ui.docking.panels.find("drawMenu").map(|panel| panel.place),
            Some(PanelPlace::Docked(Side::Right))
        );
    }

    /// The pointer clicked on `panel`'s title-bar button `button` as a person clicks it: moved
    /// there, pressed and let go a frame later, with no move between. The harness's own press
    /// (`Posted::press`) marks the button clicked without the pointer, so it passed while a real
    /// click on Float or Minimise did nothing (the owner, 2026-11-19).
    fn click_bar_button(window: &mut MainWindow, headless: &mut Headless, panel: &str, button: &str) {
        let at = headless
            .snapshot()
            .control(panel, button)
            .unwrap_or_else(|| panic!("{panel} has its {button}"))
            .rect
            .center();
        headless.push_event(Event::PointerMoved(at));
        let _ = headless.frame(window);
        for pressed in [true, false] {
            headless.push_event(Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            let _ = headless.frame(window);
        }
        headless
            .settle(window)
            .unwrap_or_else(|problem| panic!("{problem}"));
    }

    #[test]
    fn a_click_on_a_floating_torn_off_menus_dock_button_docks_it_and_float_floats_it_again() {
        let mut window = MainWindow::new();
        window.trigger("viewMenuTearOffDrawMenu").expect("torn off");
        let mut headless = settled(&mut window);
        let place = |window: &MainWindow| {
            window.ui.docking.panels.find("drawMenu").map(|panel| panel.place)
        };
        assert!(matches!(place(&window), Some(PanelPlace::Floating(_))));
        // A floating panel's button docks it, and says so.
        let button = headless
            .snapshot()
            .control("drawMenuTornOff", "DockFloatButton")
            .expect("the panel's Float button")
            .clone();
        assert_eq!(button.text, "Dock", "a floating panel's button docks it");
        click_bar_button(&mut window, &mut headless, "drawMenuTornOff", "DockFloatButton");
        assert_eq!(place(&window), Some(PanelPlace::Docked(Side::Right)));
        let button = headless
            .snapshot()
            .control("drawMenuTornOff", "DockFloatButton")
            .expect("the docked panel's Float button")
            .clone();
        assert_eq!(button.text, "Float");
        click_bar_button(&mut window, &mut headless, "drawMenuTornOff", "DockFloatButton");
        assert!(
            matches!(place(&window), Some(PanelPlace::Floating(_))),
            "{:?}",
            place(&window)
        );
    }

    #[test]
    fn a_floating_torn_off_menu_dragged_by_any_edge_or_corner_is_sized_by_it() {
        // The owner, 2026-11-19: "i cannot resize these menus when i go the edge". Each edge and
        // corner, the pointer pressed a point inside the panel's border and moved 40 points out.
        let out = 40.0;
        /// An edge's name, where on the panel it is pressed, and how far it is dragged.
        type Edge = (&'static str, fn(Rect) -> Pos2, egui::Vec2);
        let edges: [Edge; 5] = [
            ("top", |r: Rect| Pos2::new(r.center().x - 40.0, r.min.y + 1.0), egui::vec2(0.0, -out)),
            ("right", |r: Rect| Pos2::new(r.max.x - 1.0, r.center().y), egui::vec2(out, 0.0)),
            ("bottom", |r: Rect| Pos2::new(r.center().x, r.max.y - 1.0), egui::vec2(0.0, out)),
            ("left", |r: Rect| Pos2::new(r.min.x + 1.0, r.center().y), egui::vec2(-out, 0.0)),
            ("corner", |r: Rect| r.max - egui::vec2(1.0, 1.0), egui::vec2(out, out)),
        ];
        for (edge, grab, by) in edges {
            let mut window = MainWindow::new();
            window.trigger("viewMenuTearOffDrawMenu").expect("torn off");
            let mut headless = settled(&mut window);
            let before = drawn(&headless, "drawMenuTornOff");
            let from = grab(before);
            drag(&mut window, &mut headless, from, from + by);
            let after = drawn(&headless, "drawMenuTornOff");
            let grew = after.size() - before.size();
            // The drag's 40 points, less the half point the pixel snap may take at each end.
            assert!(
                (grew.x - by.x.abs()).abs() <= 1.0 && (grew.y - by.y.abs()).abs() <= 1.0,
                "{edge}: {before:?} -> {after:?}"
            );
            if edge == "left" {
                assert!((after.max.x - before.max.x).abs() <= 1.0, "left keeps its right edge");
            }
            if edge == "top" {
                // Sized from its top, it is not moved as a press on its title bar moves it.
                assert!((after.max.y - before.max.y).abs() <= 1.0, "top keeps its bottom edge");
            }
        }
    }

    #[test]
    fn a_docked_torn_off_menus_band_is_sized_by_a_drag_just_beside_its_seam() {
        let mut window = MainWindow::new();
        window.trigger("viewMenuTearOffDrawMenu").expect("torn off");
        window.trigger("viewMenuPanelDockDrawMenuLeft").expect("docked");
        let mut headless = settled(&mut window);
        let before = drawn(&headless, "drawMenuTornOff");
        let seam = headless
            .snapshot()
            .control_anywhere("menuBandSeamLeft")
            .expect("the band's seam")
            .rect;
        // Four points from the middle of the 4 point seam, two past its edge over the views':
        // within the reach that makes it findable.
        let from = Pos2::new(seam.center().x + 4.0, seam.center().y);
        drag(&mut window, &mut headless, from, from + egui::vec2(60.0, 0.0));
        let after = drawn(&headless, "drawMenuTornOff");
        assert!(
            (after.width() - before.width() - 60.0).abs() <= 1.0,
            "{before:?} -> {after:?}"
        );
    }

    #[test]
    fn a_click_on_a_torn_off_menus_minimise_hides_it_to_the_tray_and_its_button_there_puts_it_back() {
        let mut window = MainWindow::new();
        window.trigger("viewMenuTearOffDrawMenu").expect("torn off");
        let mut headless = settled(&mut window);
        let before = window.ui.docking.panels.find("drawMenu").map(|panel| panel.place);
        click_bar_button(&mut window, &mut headless, "drawMenuTornOff", "DockMinimiseButton");
        let frame = headless.snapshot();
        assert!(frame.target("drawMenuTornOff").is_none(), "the panel is hidden");
        assert!(
            frame.control("MinimisedToolBar", "RestoredrawMenuTornOff").is_some(),
            "and has its button in the tray"
        );
        click_bar_button(&mut window, &mut headless, "MinimisedToolBar", "RestoredrawMenuTornOff");
        assert!(headless.snapshot().target("drawMenuTornOff").is_some(), "back");
        assert_eq!(
            window.ui.docking.panels.find("drawMenu").map(|panel| panel.place),
            before,
            "where it was"
        );
    }
}

mod torn_off_flow {
    use super::*;

    #[test]
    fn a_torn_off_menu_along_the_bottom_flows_its_items_into_columns_none_cut() {
        // In one column, a bottom band of some six items' height showed four of Draw's forty and
        // left the rest of the window's width empty (the sixth look audit).
        let mut window = MainWindow::new();
        window.trigger("viewMenuTearOffDrawMenu").expect("torn off");
        window
            .trigger("viewMenuPanelDockDrawMenuBottom")
            .expect("docked along the bottom");
        let headless = settled(&mut window);
        let panel = drawn(&headless, "drawMenuTornOff");
        let frame = headless.snapshot();
        let items: Vec<Rect> = frame
            .in_target("drawMenuTornOff")
            .filter(|control| control.visible && panel.intersects(control.rect))
            .filter(|control| !control.name.starts_with("Dock"))
            .map(|control| control.rect)
            .collect();
        let mut columns: Vec<f32> = items.iter().map(|rect| rect.min.x.round()).collect();
        columns.sort_by(f32::total_cmp);
        columns.dedup();
        assert!(columns.len() >= 3, "flowed into columns: {columns:?}");
        assert!(items.len() >= 12, "more than one column's worth shown: {}", items.len());
        for rect in &items {
            assert!(rect.max.y <= panel.max.y + 0.5, "{rect:?} is cut at the band's bottom {panel:?}");
        }
        // Down a side it is one column still.
        window
            .trigger("viewMenuPanelDockDrawMenuLeft")
            .expect("docked left");
        let headless = settled(&mut window);
        let panel = drawn(&headless, "drawMenuTornOff");
        let frame = headless.snapshot();
        let mut lefts: Vec<f32> = frame
            .in_target("drawMenuTornOff")
            .filter(|control| control.visible && panel.contains_rect(control.rect))
            .filter(|control| !control.name.starts_with("Dock"))
            .map(|control| control.rect.min.x.round())
            .collect();
        lefts.sort_by(f32::total_cmp);
        lefts.dedup();
        assert_eq!(lefts.len(), 1, "{lefts:?}");
    }

    #[test]
    fn every_menu_of_the_menu_bar_has_an_icon_its_torn_off_panel_shows() {
        // A menu of the menu bar opens from no item and had none: its panel's title bar showed
        // the generic panel icon.
        let window = MainWindow::new();
        let composed = window.composed();
        for menu in &composed.menus {
            assert!(
                katana_window::icons::is_named_icon(menu.icon),
                "{} has an icon: {:?}",
                menu.name,
                menu.icon
            );
        }
        let draw = composed
            .menus
            .iter()
            .find(|menu| menu.name == "drawMenu")
            .expect("the Draw menu");
        let toggle = composed
            .action("viewToolBarDraw")
            .expect("View > Toolbars > Draw");
        assert_eq!(draw.icon, toggle.icon, "the icon its toolbar is listed with");
    }
}

mod docking_model {
    use super::*;

    fn places(list: &[(&str, BarPlace)]) -> Vec<(String, BarPlace)> {
        list.iter()
            .map(|(name, place)| (String::from(*name), *place))
            .collect()
    }

    #[test]
    fn a_line_emptied_by_a_move_closes_up() {
        let mut list = places(&[
            ("A", BarPlace::Top(0)),
            ("B", BarPlace::Top(2)),
            ("C", BarPlace::Left(1)),
        ]);
        normalise(&mut list);
        assert_eq!(
            list,
            places(&[
                ("A", BarPlace::Top(0)),
                ("B", BarPlace::Top(1)),
                ("C", BarPlace::Left(0))
            ])
        );
    }

    #[test]
    fn a_drop_before_a_bar_puts_it_there_and_a_new_line_moves_the_lines_after_it_out() {
        let mut list = places(&[
            ("A", BarPlace::Top(0)),
            ("B", BarPlace::Top(0)),
            ("C", BarPlace::Top(1)),
        ]);
        assert!(apply_drop(
            &mut list,
            "C",
            &DropSpot::Line {
                side: Side::Top,
                line: 0,
                before: Some(String::from("B"))
            }
        ));
        assert_eq!(
            list,
            places(&[
                ("A", BarPlace::Top(0)),
                ("C", BarPlace::Top(0)),
                ("B", BarPlace::Top(0))
            ])
        );
        assert!(apply_drop(
            &mut list,
            "A",
            &DropSpot::NewLine {
                side: Side::Top,
                before: 0
            }
        ));
        assert_eq!(
            list,
            places(&[
                ("A", BarPlace::Top(0)),
                ("C", BarPlace::Top(1)),
                ("B", BarPlace::Top(1))
            ])
        );
        let at = FloatAt {
            x: 10.0,
            y: 20.0,
            width: 30.0,
        };
        assert!(apply_drop(&mut list, "B", &DropSpot::Float(at)));
        assert_eq!(list[2], (String::from("B"), BarPlace::Floating(at)));
        assert!(!apply_drop(&mut list, "Z", &DropSpot::Float(at)), "no such bar");
    }

    #[test]
    fn a_drop_lands_on_the_line_under_the_pointer_beside_it_on_a_new_line_or_floats() {
        let zones = DropZones {
            lines: vec![(
                Side::Top,
                0,
                Rect::from_min_max(Pos2::new(0.0, 26.0), Pos2::new(1000.0, 65.0)),
            )],
            bars: vec![
                (
                    String::from("A"),
                    Side::Top,
                    0,
                    Rect::from_min_max(Pos2::new(0.0, 26.0), Pos2::new(300.0, 65.0)),
                ),
                (
                    String::from("B"),
                    Side::Top,
                    0,
                    Rect::from_min_max(Pos2::new(300.0, 26.0), Pos2::new(600.0, 65.0)),
                ),
            ],
            new_lines: vec![(
                Side::Top,
                1,
                Rect::from_min_max(Pos2::new(0.0, 65.0), Pos2::new(1000.0, 75.0)),
            )],
        };
        let grab = egui::vec2(3.0, 10.0);
        assert_eq!(
            drop_spot(&zones, "X", Pos2::new(100.0, 40.0), grab, 200.0),
            DropSpot::Line {
                side: Side::Top,
                line: 0,
                before: Some(String::from("A"))
            }
        );
        assert_eq!(
            drop_spot(&zones, "X", Pos2::new(500.0, 40.0), grab, 200.0),
            DropSpot::Line {
                side: Side::Top,
                line: 0,
                before: None
            },
            "past B's middle: at the line's end"
        );
        assert_eq!(
            drop_spot(&zones, "B", Pos2::new(200.0, 40.0), grab, 200.0),
            DropSpot::Line {
                side: Side::Top,
                line: 0,
                before: None
            },
            "the bar dragged is not one it goes before"
        );
        assert_eq!(
            drop_spot(&zones, "X", Pos2::new(500.0, 70.0), grab, 200.0),
            DropSpot::NewLine {
                side: Side::Top,
                before: 1
            }
        );
        assert_eq!(
            drop_spot(&zones, "X", Pos2::new(500.0, 400.0), grab, 200.0),
            DropSpot::Float(FloatAt {
                x: 497.0,
                y: 390.0,
                width: 200.0
            })
        );
    }

    #[test]
    fn a_torn_off_menu_docks_to_the_nearest_side_of_the_views_within_its_zone() {
        let views = Rect::from_min_max(Pos2::new(300.0, 150.0), Pos2::new(1000.0, 700.0));
        assert_eq!(panel_drop_side(views, Pos2::new(310.0, 400.0)), Some(Side::Left));
        assert_eq!(panel_drop_side(views, Pos2::new(990.0, 400.0)), Some(Side::Right));
        assert_eq!(panel_drop_side(views, Pos2::new(600.0, 155.0)), Some(Side::Top));
        assert_eq!(panel_drop_side(views, Pos2::new(600.0, 690.0)), Some(Side::Bottom));
        assert_eq!(panel_drop_side(views, Pos2::new(600.0, 400.0)), None, "the middle floats");
        assert_eq!(panel_drop_side(views, Pos2::new(100.0, 400.0)), None, "far outside");
    }

    #[test]
    fn the_docking_actions_are_read_from_their_names() {
        assert_eq!(
            parse_action("viewToolBarDockSurfaceEditBottom"),
            Some(DockingAction::Dock {
                key: String::from("SurfaceEdit"),
                side: Some(Side::Bottom)
            })
        );
        assert_eq!(
            parse_action("viewToolBarDockGISFloat"),
            Some(DockingAction::Dock {
                key: String::from("GIS"),
                side: None
            })
        );
        assert_eq!(
            parse_action("viewMenuTearOffViewToolBars"),
            Some(DockingAction::TearOff {
                menu: String::from("viewToolBars")
            })
        );
        assert_eq!(
            parse_action("viewMenuPanelDockDrawMenuLeft"),
            Some(DockingAction::PanelDock {
                menu: String::from("drawMenu"),
                side: Some(Side::Left)
            })
        );
        assert_eq!(
            parse_action("viewToolBarPlace:GISToolBar:float:1,2,3"),
            Some(DockingAction::Place {
                bar: String::from("GISToolBar"),
                side: None,
                line: 0,
                index: 0,
                float: Some(FloatAt {
                    x: 1.0,
                    y: 2.0,
                    width: 3.0
                })
            })
        );
        assert_eq!(parse_action("viewToolBarsReset"), Some(DockingAction::Reset));
        // Not one: a toolbar's toggle, its names and icon sizes, malformed places.
        for name in [
            "viewToolBarSurvey",
            "viewToolBarNames",
            "viewToolBarIconsLarge",
            "viewToolBarDock",
            "viewToolBarDockLeft",
            "viewToolBarPlace:GISToolBar:sideways:0",
            "viewToolBarPlace:GISToolBar:float:1,2",
            "viewMenuBand:middle:200",
        ] {
            assert_eq!(parse_action(name), None, "{name}");
        }
    }

    #[test]
    fn a_place_is_written_and_read_back_the_same() {
        for place in [
            BarPlace::Top(2),
            BarPlace::Left(0),
            BarPlace::Right(1),
            BarPlace::Bottom(0),
            BarPlace::Floating(FloatAt {
                x: 400.0,
                y: -12.0,
                width: 320.0,
            }),
        ] {
            assert_eq!(BarPlace::from_text(&place.text()), Some(place), "{}", place.text());
        }
        assert_eq!(BarPlace::from_text("float:1,2"), None);
        assert_eq!(BarPlace::from_text("top:x"), None);
        assert_eq!(BarPlace::from_text(""), None);
    }

    #[test]
    fn a_menu_flows_into_columns_its_sections_whole_where_they_fit_and_no_rule_first() {
        use FlowItem::{Item, Section, Separator};
        // Sections 20 high, items 24, 100 a column, nothing between them.
        let kinds = [Section, Item, Item, Section, Item, Item, Item];
        let heights = [20.0, 24.0, 24.0, 20.0, 24.0, 24.0, 24.0];
        // The first group takes 68; the second's 92 does not fit under it, and fits a column of
        // its own.
        assert_eq!(
            flow_columns(&kinds, &heights, 100.0, 0.0),
            vec![vec![0, 1, 2], vec![3, 4, 5, 6]]
        );
        // A group taller than a column runs on, its title never last in one: 20 + 3 x 24 = 92,
        // and the fourth item starts the next.
        let tall = [Section, Item, Item, Item, Item, Item];
        let tall_heights = [20.0, 24.0, 24.0, 24.0, 24.0, 24.0];
        assert_eq!(
            flow_columns(&tall, &tall_heights, 100.0, 0.0),
            vec![vec![0, 1, 2, 3], vec![4, 5]]
        );
        // A title that would end a column goes to the next with its first item: 3 x 24 = 72, and
        // 72 + 20 + 24 is past 90.
        let late = [Item, Item, Item, Section, Item];
        let late_heights = [24.0, 24.0, 24.0, 20.0, 24.0];
        assert_eq!(
            flow_columns(&late, &late_heights, 90.0, 0.0),
            vec![vec![0, 1, 2], vec![3, 4]]
        );
        // A rule that would begin a column is left out; one between items is kept. The spacing
        // counts between items: 3 x 24 + 2 x 4 = 80 fits 82, a fourth does not.
        let ruled = [Item, Item, Item, Separator, Item, Separator, Item];
        let ruled_heights = [24.0, 24.0, 24.0, 9.0, 24.0, 9.0, 24.0];
        assert_eq!(
            flow_columns(&ruled, &ruled_heights, 82.0, 4.0),
            vec![vec![0, 1, 2], vec![4, 5, 6]]
        );
        // Every column's items fit the room.
        for (kinds, heights, room) in [
            (&kinds[..], &heights[..], 100.0),
            (&tall[..], &tall_heights[..], 100.0),
            (&late[..], &late_heights[..], 90.0),
        ] {
            for column in flow_columns(kinds, heights, room, 0.0) {
                let used: f32 = column.iter().map(|&index| heights[index]).sum();
                assert!(used <= room, "{column:?}: {used} of {room}");
            }
        }
        // Nothing: no column.
        assert!(flow_columns(&[], &[], 100.0, 0.0).is_empty());
    }

    #[test]
    fn a_wrapped_bar_is_drawn_at_the_least_width_for_its_rows() {
        // Eight 33 point buttons in 255 points: seven a row (6 + 7 x 33 + 6 x 2 + 6 = 255), the
        // eighth alone under them. The least width for two rows is four a row:
        // 6 + 4 x 33 + 3 x 2 + 6 = 150.
        let sizes = [(33, 33); 8];
        let separators = [false; 8];
        let ragged = wrap_strip(&sizes, &separators, 255);
        assert_eq!(ragged.rows, 2);
        assert_eq!(ragged.offsets[7], (6, 38), "the eighth alone");
        let width = balanced_wrap_width(&sizes, &separators, 255);
        assert_eq!(width, 150);
        let shared = wrap_strip(&sizes, &separators, width);
        assert_eq!(shared.rows, 2);
        assert_eq!(shared.offsets[4], (6, 38), "the fifth starts the second row");
        // In one row it is the row's own width; at its widest item's it stays there.
        assert_eq!(balanced_wrap_width(&sizes, &separators, 400), 6 + 8 * 33 + 7 * 2 + 6);
        assert_eq!(balanced_wrap_width(&sizes, &separators, 45), 45);
        assert_eq!(balanced_wrap_width(&[], &[], 100), 12);
    }

    #[test]
    fn a_toolbars_separator_is_a_rule_across_the_bar_not_a_dot() {
        // A separator's own size is 11 by 9: drawn 4 points in from either end of its 9, its
        // rule was a point long. Its cell spans the bar's breadth, and the rule crosses it 4
        // points short of either side along a row, 5 down a column.
        let item = StripItem::Separator;
        assert_eq!(separator_cell(&item, (11, 9), 33, false), (11, 33));
        assert_eq!(separator_cell(&item, (11, 9), 33, true), (33, 9));
        let row = separator_rule(Rect::from_min_size(Pos2::new(100.0, 3.0), egui::vec2(11.0, 33.0)), false);
        assert_eq!(row[1].y - row[0].y, 25.0);
        assert_eq!(row[0].x, 105.5);
        let column = separator_rule(Rect::from_min_size(Pos2::new(3.0, 100.0), egui::vec2(33.0, 9.0)), true);
        assert_eq!(column[1].x - column[0].x, 23.0);
        // Any other item keeps its size.
        let widget = StripItem::Widget {
            name: String::from("w"),
            width: 40,
            height: 20,
        };
        assert_eq!(separator_cell(&widget, (40, 20), 33, false), (40, 20));
    }

    #[test]
    fn a_floating_bars_items_wrap_in_rows_and_no_separator_begins_or_ends_one() {
        // Three 33 point buttons, a separator, two more, in 90 points: the padding is 6 a side,
        // so two buttons a row (6 + 33 + 2 + 33 = 74; a third would end at 109).
        let button = (33, 33);
        let separator = (11, 9);
        let sizes = [button, button, button, separator, button, button];
        let separators = [false, false, false, true, false, false];
        let layout = wrap_strip(&sizes, &separators, 90);
        assert_eq!(layout.rows, 3);
        assert_eq!(layout.offsets[0], (6, 3));
        assert_eq!(layout.offsets[1], (41, 3));
        assert_eq!(layout.offsets[2], (6, 38));
        // The separator fits after the third button on its row, then the fourth wraps: the
        // separator, last on its row, parts nothing and is not drawn.
        assert!(!layout.shown[3]);
        assert_eq!(layout.offsets[4], (6, 73));
        assert_eq!(layout.size, (90, 3 * 33 + 2 * 2 + 2 * 3));
        // Wide enough for all: one row, the separator drawn.
        let one = wrap_strip(&sizes, &separators, 400);
        assert_eq!(one.rows, 1);
        assert!(one.shown.iter().all(|shown| *shown));
        // Nothing: the padding alone.
        assert_eq!(wrap_strip(&[], &[], 100).rows, 0);
        // An item wider than the room is a row of its own.
        let wide = wrap_strip(&[(200, 33), button], &[false, false], 90);
        assert_eq!(wide.rows, 2);
    }
}
