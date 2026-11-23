//! The main window's keyboard shortcuts (main_window::shortcuts): real key events sent to the
//! window as the headless runner draws it, and what each key reached - the action its menu item
//! shows it beside, a text box, the plan view, or nothing while something is in front.
//!
//! Ctrl is sent as winit reports it off a Mac: the Ctrl key, which is egui's "command" too.

use egui::{Event, Key, KeyboardShortcut, Modifiers, PointerButton, Pos2};
use katana_commands::{EntityAttributes, create_polyline};
use katana_entity::{EntityId, Geometry};
use katana_geometry::{Box2, Point2, Polyline2};
use katana_render::camera::Projection;
use katana_window::MainWindow;
use katana_window::drive::headless::Headless;
use katana_window::drive::{ControlKind, ControlRecord, DrivenWindow, Posted};
use katana_window::main_window::panels::{InputDialog, InputPurpose};
use katana_window::main_window::shortcuts;

fn settle(window: &mut MainWindow, headless: &mut Headless) {
    headless
        .settle(window)
        .unwrap_or_else(|problem| panic!("{problem}"));
}

/// A new window with nobody at it - no file dialog, no question, no settings file - drawn until
/// it is idle, and the runner that drew it.
fn headless_window() -> (MainWindow, Headless) {
    let mut window = MainWindow::new();
    window.set_headless(true);
    let mut headless = Headless::new();
    settle(&mut window, &mut headless);
    (window, headless)
}

fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        command: true,
        ..Modifiers::NONE
    }
}

/// The keys held for a sequence, as a keyboard sends them.
fn held_for(pattern: Modifiers) -> Modifiers {
    let mut held = if pattern.command || pattern.ctrl {
        ctrl()
    } else {
        Modifiers::NONE
    };
    held.shift = pattern.shift;
    held.alt = pattern.alt;
    held
}

fn key_event(key: Key, modifiers: Modifiers, pressed: bool) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers,
    }
}

/// `key` pressed and let go with `modifiers` held, and the window drawn until it is idle.
fn press(window: &mut MainWindow, headless: &mut Headless, key: Key, modifiers: Modifiers) {
    headless.push_event(key_event(key, modifiers, true));
    headless.push_event(key_event(key, modifiers, false));
    settle(window, headless);
}

/// `key` pressed and not let go: one frame. A second call before `release` is the system's
/// repeat of a held key, which egui marks as one.
fn hold(window: &mut MainWindow, headless: &mut Headless, key: Key, modifiers: Modifiers) {
    headless.push_event(key_event(key, modifiers, true));
    let _ = headless.frame(window);
}

fn release(window: &mut MainWindow, headless: &mut Headless, key: Key, modifiers: Modifiers) {
    headless.push_event(key_event(key, modifiers, false));
    settle(window, headless);
}

/// A left click at `at` as a person makes one: the pointer moved there, pressed, and let go a
/// frame later.
fn click(window: &mut MainWindow, headless: &mut Headless, at: Pos2) {
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
    settle(window, headless);
}

/// What the last press reached (shortcuts::taken).
fn taken(headless: &Headless) -> Vec<String> {
    shortcuts::taken(headless.context())
}

fn focused(headless: &Headless) -> Option<egui::Id> {
    headless.context().memory(|memory| memory.focused())
}

fn entities(window: &MainWindow) -> usize {
    window.exec.document.model().entities.size()
}

fn selected(window: &MainWindow) -> usize {
    window.exec.document.selection().size()
}

fn line(window: &mut MainWindow, text: &str) {
    assert!(window.run_command(text), "{text} was refused");
}

/// The first view drawn whose name starts with `prefix` ("PlanView", "RenderView").
fn view_drawn(headless: &Headless, prefix: &str) -> ControlRecord {
    headless
        .snapshot()
        .controls
        .iter()
        .find(|control| control.kind == ControlKind::View && control.name.starts_with(prefix))
        .unwrap_or_else(|| panic!("a {prefix} is drawn"))
        .clone()
}

fn command_input(headless: &Headless) -> ControlRecord {
    headless
        .snapshot()
        .control_anywhere("commandInput")
        .expect("the command line's field is drawn")
        .clone()
}

/// The command line's field clicked, so that it has the keyboard.
fn click_command_line(window: &mut MainWindow, headless: &mut Headless) -> egui::Id {
    let field = command_input(headless);
    click(window, headless, field.rect.center());
    assert_eq!(
        focused(headless),
        Some(field.id),
        "a click gives the command line the keyboard"
    );
    field.id
}

fn type_text(window: &mut MainWindow, headless: &mut Headless, text: &str) {
    headless.push_event(Event::Text(String::from(text)));
    settle(window, headless);
}

/// A polyline through (0,0), (10,0) and (20,0), framed in the plan view: its id.
fn polyline(window: &mut MainWindow, headless: &mut Headless) -> EntityId {
    window
        .exec
        .document
        .execute(create_polyline(
            Polyline2::new(
                vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(10.0, 0.0),
                    Point2::new(20.0, 0.0),
                ],
                false,
            ),
            EntityAttributes::default(),
        ))
        .unwrap_or_else(|error| panic!("{}", error.describe()));
    let id = window.exec.document.last_created_entities()[0];
    line(window, "ZOOM EXTENTS");
    settle(window, headless);
    id
}

fn vertex_count(window: &MainWindow, id: EntityId) -> Option<usize> {
    let entity = window.exec.document.model().entities.find(id)?;
    match &entity.geometry {
        Geometry::Polyline(polyline) => Some(polyline.vertices.len()),
        other => panic!("not a plain polyline: {other:?}"),
    }
}

fn projection(window: &MainWindow) -> Projection {
    let workspace = &window.exec.workspace;
    let view = workspace.active_render_view().expect("a 3D view is open");
    workspace
        .state(view)
        .expect("the 3D view has its state")
        .camera
        .projection()
}

mod keyboard_shortcuts {
    use super::*;

    // ---- a key reaches its action ---------------------------------------------------------------

    #[test]
    fn ctrl_z_undoes_the_line_just_drawn_and_ctrl_y_draws_it_again() {
        let (mut window, mut headless) = headless_window();
        line(&mut window, "LINE 0,0 10,0");
        assert_eq!(entities(&window), 1);

        press(&mut window, &mut headless, Key::Z, ctrl());
        assert_eq!(taken(&headless), ["editUndo"]);
        assert_eq!(entities(&window), 0, "Ctrl+Z took the line back");

        press(&mut window, &mut headless, Key::Y, ctrl());
        assert_eq!(taken(&headless), ["editRedo"]);
        assert_eq!(entities(&window), 1, "Ctrl+Y drew it again");
    }

    #[test]
    fn f9_switches_the_3d_view_between_perspective_and_orthographic() {
        let (mut window, mut headless) = headless_window();
        line(&mut window, "VIEWS OPEN 3d");
        settle(&mut window, &mut headless);
        let before = projection(&window);

        press(&mut window, &mut headless, Key::F9, Modifiers::NONE);
        assert_eq!(taken(&headless), ["viewTogglePerspective"]);
        let after = projection(&window);
        assert_ne!(after, before, "F9 switched the projection");
        assert!(
            [Projection::Perspective, Projection::Orthographic].contains(&after),
            "{after:?}"
        );

        press(&mut window, &mut headless, Key::F9, Modifiers::NONE);
        assert_eq!(projection(&window), before, "and F9 again switched it back");
    }

    #[test]
    fn a_key_whose_action_is_disabled_runs_nothing() {
        let (mut window, mut headless) = headless_window();
        // Nothing has been done, so there is nothing to undo or to redo.
        for name in ["editUndo", "editRedo"] {
            let action = window.composed().action(name).expect("the Edit menu has it");
            assert!(!action.enabled, "{name} is greyed in a new window");
        }
        let errors = window.exec.log.errors_logged();
        let log = window.exec.log.text();

        press(&mut window, &mut headless, Key::Z, ctrl());
        assert!(taken(&headless).is_empty(), "{:?}", taken(&headless));
        press(&mut window, &mut headless, Key::Y, ctrl());
        assert!(taken(&headless).is_empty(), "{:?}", taken(&headless));

        // Not refused either, as a click on a greyed item is not: the key did nothing at all.
        assert_eq!(window.exec.log.errors_logged(), errors);
        assert_eq!(window.exec.log.text(), log);
    }

    // The whole table, as the window composes it when the key is pressed: whatever a workbench or
    // a menu adds is reached by its key with nothing added to the pass.
    #[test]
    fn every_enabled_shortcut_of_a_new_window_reaches_its_own_action_and_no_other() {
        let (table, _) = headless_window();
        let with_keys: Vec<(String, KeyboardShortcut, bool)> = table
            .composed()
            .actions()
            .into_iter()
            .filter_map(|action| {
                let keys = action.shortcut?;
                Some((action.name, keys, action.enabled))
            })
            .collect();
        // Quit has no key in a headless window (actions::quit_action), so no press here closes
        // the window; and a headless window opens no file dialog, so Open, Save As and Import
        // are pressed like any other and only say what to type instead.
        assert!(
            with_keys.iter().all(|(name, ..)| name != "fileQuit"),
            "a headless window gives Quit no key"
        );

        let mut pressed: Vec<String> = Vec::new();
        for (name, keys, enabled) in &with_keys {
            // A greyed action leaves its key alone (a_key_whose_action_is_disabled_runs_nothing).
            if !enabled {
                continue;
            }
            // A window of its own: an action that opens a dialog leaves the next key under it.
            let (mut window, mut headless) = headless_window();
            press(
                &mut window,
                &mut headless,
                keys.logical_key,
                held_for(keys.modifiers),
            );
            assert_eq!(
                taken(&headless),
                std::slice::from_ref(name),
                "{name}, pressed as {keys:?}"
            );
            pressed.push(name.clone());
        }

        // The loop reached the table: these are in every new window, enabled, each with the key
        // its menu item shows (docs/desktop.md's menus; the Drafting bar's F8).
        for known in [
            "fileNew",
            "fileOpen",
            "fileSave",
            "fileSaveAs",
            "filePlot",
            "fileSheets",
            "formatLayers",
            "editSelectAll",
            "editErase",
            "editDeselect",
            "viewZoomExtents",
            "viewGrid",
            "viewSnap",
            "viewTogglePerspective",
            "helpCommandReference",
            "actionOrtho",
        ] {
            assert!(
                pressed.iter().any(|name| name == known),
                "{known} was not pressed; pressed: {pressed:?}"
            );
        }
    }

    // ---- a held key -----------------------------------------------------------------------------

    #[test]
    fn a_held_key_runs_its_action_once_and_a_held_ctrl_z_steps_on_through_the_history() {
        let (mut window, mut headless) = headless_window();
        let grid = window.exec.workspace.grid_visible();
        // The press and three of the system's repeats: an action run at each would leave the
        // grid as it was.
        hold(&mut window, &mut headless, Key::F7, Modifiers::NONE);
        assert_eq!(taken(&headless), ["viewGrid"]);
        for _ in 0..3 {
            hold(&mut window, &mut headless, Key::F7, Modifiers::NONE);
            assert!(taken(&headless).is_empty(), "a repeat ran {:?}", taken(&headless));
        }
        release(&mut window, &mut headless, Key::F7, Modifiers::NONE);
        assert_eq!(
            window.exec.workspace.grid_visible(),
            !grid,
            "one press of F7 is one change of the grid"
        );

        // Undo is what holding its key is for: the press and two repeats take three lines back.
        for y in ["0", "5", "10"] {
            line(&mut window, &format!("LINE 0,{y} 10,{y}"));
        }
        assert_eq!(entities(&window), 3);
        for left in [2, 1, 0] {
            hold(&mut window, &mut headless, Key::Z, ctrl());
            assert_eq!(taken(&headless), ["editUndo"]);
            assert_eq!(entities(&window), left);
        }
        release(&mut window, &mut headless, Key::Z, ctrl());
        assert_eq!(entities(&window), 0);
    }

    // ---- a text box -----------------------------------------------------------------------------

    #[test]
    fn in_a_dialogs_field_delete_and_escape_are_the_fields_and_reach_no_action() {
        let (mut window, mut headless) = headless_window();
        line(&mut window, "LINE 0,0 10,0");
        window.select_all();
        window
            .trigger("editSelectById")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        settle(&mut window, &mut headless);
        headless
            .act(
                &mut window,
                Posted::fill("selectByIdDialog", "selectByIdIds", "123"),
            )
            .unwrap_or_else(|problem| panic!("{problem}"));
        let field = headless
            .snapshot()
            .control("selectByIdDialog", "selectByIdIds")
            .expect("the dialog has its field")
            .clone();
        click(&mut window, &mut headless, field.rect.center());
        assert_eq!(focused(&headless), Some(field.id), "the field has the keyboard");
        let value = |headless: &Headless| {
            headless
                .snapshot()
                .control("selectByIdDialog", "selectByIdIds")
                .expect("the dialog has its field")
                .value
                .clone()
        };

        // Home, then Delete: the first character goes from the field, and the selected line is
        // not erased - Delete is Edit > Erase Selection's key everywhere else.
        press(&mut window, &mut headless, Key::Home, Modifiers::NONE);
        press(&mut window, &mut headless, Key::Delete, Modifiers::NONE);
        assert_eq!(value(&headless), "23");
        assert!(taken(&headless).is_empty(), "{:?}", taken(&headless));
        assert_eq!((entities(&window), selected(&window)), (1, 1));

        // Escape leaves the field, and that is all it does: Edit > Deselect, whose key it is,
        // would have cleared the selection.
        press(&mut window, &mut headless, Key::Escape, Modifiers::NONE);
        assert_eq!(focused(&headless), None, "Escape left the field");
        assert!(taken(&headless).is_empty(), "{:?}", taken(&headless));
        assert_eq!(selected(&window), 1);

        // With no text box holding the keyboard the same key is Deselect's.
        press(&mut window, &mut headless, Key::Escape, Modifiers::NONE);
        assert_eq!(taken(&headless), ["editDeselect"]);
        assert_eq!(selected(&window), 0);
    }

    #[test]
    fn with_the_command_line_focused_ctrl_and_function_keys_still_reach_their_actions() {
        let (mut window, mut headless) = headless_window();
        let field = click_command_line(&mut window, &mut headless);

        // A line typed and entered there, as a person draws one: the field keeps the keyboard.
        type_text(&mut window, &mut headless, "LINE 0,0 10,0");
        assert_eq!(window.ui.panels.command_text, "LINE 0,0 10,0");
        press(&mut window, &mut headless, Key::Enter, Modifiers::NONE);
        assert_eq!(entities(&window), 1);
        assert_eq!(focused(&headless), Some(field));

        // Ctrl+Z undoes the DRAWING, and the field's own undo does not bring the line's text
        // back: the key was the action's alone.
        press(&mut window, &mut headless, Key::Z, ctrl());
        assert_eq!(taken(&headless), ["editUndo"]);
        assert_eq!(entities(&window), 0);
        assert!(
            window.ui.panels.command_text.is_empty(),
            "{}",
            window.ui.panels.command_text
        );
        assert_eq!(focused(&headless), Some(field));

        press(&mut window, &mut headless, Key::Y, ctrl());
        assert_eq!(taken(&headless), ["editRedo"]);
        assert_eq!(entities(&window), 1);

        // Ctrl+A selects the drawing, not the field's text.
        press(&mut window, &mut headless, Key::A, ctrl());
        assert_eq!(taken(&headless), ["editSelectAll"]);
        assert_eq!(selected(&window), 1);

        let grid = window.exec.workspace.grid_visible();
        press(&mut window, &mut headless, Key::F7, Modifiers::NONE);
        assert_eq!(taken(&headless), ["viewGrid"]);
        assert_eq!(window.exec.workspace.grid_visible(), !grid);
    }

    #[test]
    fn delete_on_the_command_line_edits_what_is_typed_and_erases_the_selection_once_it_is_empty() {
        let (mut window, mut headless) = headless_window();
        line(&mut window, "LINE 0,0 10,0");
        window.select_all();
        let _ = click_command_line(&mut window, &mut headless);
        type_text(&mut window, &mut headless, "ab");
        press(&mut window, &mut headless, Key::Home, Modifiers::NONE);

        // Text in the field: Delete takes a character of it, twice, and the line stays.
        for left in ["b", ""] {
            press(&mut window, &mut headless, Key::Delete, Modifiers::NONE);
            assert_eq!(window.ui.panels.command_text, left);
            assert!(taken(&headless).is_empty(), "{:?}", taken(&headless));
            assert_eq!((entities(&window), selected(&window)), (1, 1));
        }

        // Nothing typed: Delete is Erase Selection's, through the line a click on it runs.
        press(&mut window, &mut headless, Key::Delete, Modifiers::NONE);
        assert_eq!(taken(&headless), ["editErase"]);
        assert_eq!(entities(&window), 0);
        assert!(window.exec.log.text().contains("> ERASE"));
    }

    #[test]
    fn escape_on_the_command_line_is_the_command_lines_own_and_runs_deselect_no_second_time() {
        let (mut window, mut headless) = headless_window();
        line(&mut window, "LINE 0,0 10,0");
        // A tool running, something selected, and something typed for the tool: one Escape there
        // clears what is typed and cancels in the drawing once - the tool ends, the selection
        // stays. A second cancel at the same press would have gone on to clear the selection.
        line(&mut window, "LINE");
        assert!(!window.exec.workspace.active_tool_id().is_empty(), "LINE runs");
        window.select_all();
        assert_eq!(selected(&window), 1);
        let _ = click_command_line(&mut window, &mut headless);
        type_text(&mut window, &mut headless, "5,5");

        press(&mut window, &mut headless, Key::Escape, Modifiers::NONE);
        assert!(taken(&headless).is_empty(), "{:?}", taken(&headless));
        assert!(
            window.ui.panels.command_text.is_empty(),
            "{}",
            window.ui.panels.command_text
        );
        assert!(
            window.exec.workspace.active_tool_id().is_empty(),
            "the command line's Escape ended the tool"
        );
        assert_eq!(selected(&window), 1, "and went no further");
    }

    // ---- something in front ---------------------------------------------------------------------

    #[test]
    fn under_a_modal_dialog_no_shortcut_runs() {
        let (mut window, mut headless) = headless_window();
        line(&mut window, "LINE 0,0 10,0");
        let grid = window.exec.workspace.grid_visible();
        window.ui.dialogs.input = Some(InputDialog::text(
            "Vertical Exaggeration",
            "Multiply elevations by:",
            "1",
            InputPurpose::Exaggeration,
        ));
        settle(&mut window, &mut headless);
        assert!(
            headless
                .snapshot()
                .target("inputDialog")
                .is_some_and(|dialog| dialog.modal),
            "the question is up, and modal"
        );

        press(&mut window, &mut headless, Key::Z, ctrl());
        assert!(taken(&headless).is_empty(), "{:?}", taken(&headless));
        assert_eq!(entities(&window), 1, "nothing was undone under the dialog");
        press(&mut window, &mut headless, Key::F7, Modifiers::NONE);
        assert!(taken(&headless).is_empty(), "{:?}", taken(&headless));
        assert_eq!(window.exec.workspace.grid_visible(), grid);

        // The dialog gone, the same keys are the window's again.
        window.ui.dialogs.input = None;
        settle(&mut window, &mut headless);
        press(&mut window, &mut headless, Key::Z, ctrl());
        assert_eq!(taken(&headless), ["editUndo"]);
        assert_eq!(entities(&window), 0);
    }

    #[test]
    fn while_a_menu_is_open_the_keys_are_the_menus() {
        let (mut window, mut headless) = headless_window();
        line(&mut window, "LINE 0,0 10,0");
        window.select_all();
        let grid = window.exec.workspace.grid_visible();
        assert!(window.open_menu("fileMenu"));
        settle(&mut window, &mut headless);

        // Escape is for the menu, not for Deselect behind it; nor does F7 reach the grid.
        press(&mut window, &mut headless, Key::Escape, Modifiers::NONE);
        assert!(taken(&headless).is_empty(), "{:?}", taken(&headless));
        assert_eq!(selected(&window), 1);
        press(&mut window, &mut headless, Key::F7, Modifiers::NONE);
        assert!(taken(&headless).is_empty(), "{:?}", taken(&headless));
        assert_eq!(window.exec.workspace.grid_visible(), grid);

        window.ui.open_menu = None;
        settle(&mut window, &mut headless);
        press(&mut window, &mut headless, Key::F7, Modifiers::NONE);
        assert_eq!(taken(&headless), ["viewGrid"]);
        assert_eq!(window.exec.workspace.grid_visible(), !grid);
    }

    #[test]
    fn with_the_sheet_editor_in_front_the_main_windows_actions_take_no_key() {
        let (mut window, mut headless) = headless_window();
        let grid = window.exec.workspace.grid_visible();
        window.show_sheets();
        settle(&mut window, &mut headless);
        assert!(
            headless.snapshot().target("sheetEditor").is_some(),
            "the sheet editor is drawn"
        );

        // F7 is the main window's grid and nothing of the editor's: with the editor over the
        // window it reaches neither.
        press(&mut window, &mut headless, Key::F7, Modifiers::NONE);
        assert!(taken(&headless).is_empty(), "{:?}", taken(&headless));
        assert_eq!(window.exec.workspace.grid_visible(), grid);
        // Nor does the main window's Undo answer the editor's Ctrl+Z as well.
        press(&mut window, &mut headless, Key::Z, ctrl());
        assert!(taken(&headless).is_empty(), "{:?}", taken(&headless));

        if let Some(editor) = window.ui.dialogs.sheets.as_mut() {
            editor.hide();
        }
        settle(&mut window, &mut headless);
        press(&mut window, &mut headless, Key::F7, Modifiers::NONE);
        assert_eq!(taken(&headless), ["viewGrid"]);
        assert_eq!(window.exec.workspace.grid_visible(), !grid);
    }

    #[test]
    fn home_fits_the_page_in_the_sheet_editor_while_its_canvas_has_the_keyboard() {
        let (mut window, mut headless) = headless_window();
        line(&mut window, "SHEET NEW Plan");
        window.show_sheets();
        settle(&mut window, &mut headless);
        // A click on the canvas, as a person working on a sheet has made: it has the keyboard.
        let canvas = headless
            .snapshot()
            .control_anywhere("sheetCanvas")
            .expect("the sheet canvas is drawn")
            .clone();
        click(&mut window, &mut headless, canvas.rect.center());
        let editor = window.ui.dialogs.sheets.as_mut().expect("the sheet editor");
        let fitted = editor.canvas().zoom();
        // A corner of the paper, 50 mm square: closer than the page fitted.
        editor
            .canvas_mut()
            .zoom_to(&Box2::new(Point2::new(0.0, 0.0), Point2::new(50.0, 50.0)));
        settle(&mut window, &mut headless);
        let close = window
            .ui
            .dialogs
            .sheets
            .as_ref()
            .expect("the sheet editor")
            .canvas()
            .zoom();
        assert!(close > fitted, "the premise: zoomed in, {close} from {fitted}");
        // The canvas having the keyboard is no reason for the editor's keys to be dead: only a
        // box being typed in keeps them.
        assert!(
            focused(&headless).is_some(),
            "the premise: the clicked canvas has the keyboard"
        );

        press(&mut window, &mut headless, Key::Home, Modifiers::NONE);
        let after = window
            .ui
            .dialogs
            .sheets
            .as_ref()
            .expect("the sheet editor")
            .canvas()
            .zoom();
        assert!(
            (after - fitted).abs() < 1e-9,
            "Home is Fit Page: {after}, fitted {fitted}"
        );
    }

    // ---- the views' own keys --------------------------------------------------------------------

    #[test]
    fn delete_in_a_focused_plan_view_erases_the_selection_once_through_erase() {
        let (mut window, mut headless) = headless_window();
        line(&mut window, "LINE 0,0 10,0");
        line(&mut window, "ZOOM EXTENTS");
        settle(&mut window, &mut headless);
        // A click on bare paper, a quarter of the way in from the view's corner (the line runs
        // through its middle): the view has the keyboard.
        let view = view_drawn(&headless, "PlanView");
        let bare = view.rect.min + view.rect.size() * 0.25;
        click(&mut window, &mut headless, bare);
        assert_eq!(focused(&headless), Some(view.id), "the click gave the view the keyboard");
        window.select_all();
        assert_eq!(selected(&window), 1);
        let errors = window.exec.log.errors_logged();
        let steps = window.exec.document.history().undo_count();

        // The view has a Delete of its own, and Edit > Erase Selection holds the key: the press
        // is Erase's alone. Both taking it, the view deleted first and ERASE was then refused
        // for want of a selection.
        press(&mut window, &mut headless, Key::Delete, Modifiers::NONE);
        assert_eq!(taken(&headless), ["editErase"]);
        assert_eq!(entities(&window), 0);
        assert!(window.exec.log.text().contains("> ERASE"));
        assert_eq!(window.exec.log.errors_logged(), errors, "{}", window.exec.log.text());
        assert_eq!(
            window.exec.document.history().undo_count(),
            steps + 1,
            "one press is one undo step"
        );

        // And Ctrl+Z over the view, with no tool running, is the drawing's Undo.
        press(&mut window, &mut headless, Key::Z, ctrl());
        assert_eq!(taken(&headless), ["editUndo"]);
        assert_eq!(entities(&window), 1);
    }

    #[test]
    fn delete_with_a_hot_vertex_takes_the_vertex_and_leaves_the_polyline() {
        let (mut window, mut headless) = headless_window();
        let id = polyline(&mut window, &mut headless);
        window.select_all();
        // Shift and a click on the middle vertex makes it hot, as a person's click does.
        let _ = window
            .pointer_at("10,0,shift", true)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let plan = window
            .exec
            .workspace
            .active_plan_view()
            .expect("a plan view is open");
        assert_eq!(
            window
                .exec
                .workspace
                .with_plan_view(plan, |view, _| view.grip_controller().hot().len()),
            Some(1),
            "the middle vertex is hot"
        );

        // The key is the view's (PlanView::shortcut_override): Erase, which holds Delete, would
        // have taken the whole polyline.
        press(&mut window, &mut headless, Key::Delete, Modifiers::NONE);
        assert!(taken(&headless).is_empty(), "{:?}", taken(&headless));
        assert_eq!(vertex_count(&window, id), Some(2), "the vertex went, the polyline stays");
    }

    #[test]
    fn ctrl_z_while_a_tool_runs_steps_the_tool_back_and_undoes_nothing_under_it() {
        let (mut window, mut headless) = headless_window();
        line(&mut window, "LINE 0,0 10,0");
        line(&mut window, "LINE");
        assert!(!window.exec.workspace.active_tool_id().is_empty(), "LINE runs");
        line(&mut window, "0,5");
        settle(&mut window, &mut headless);

        // The first line is on the undo stack, and Ctrl+Z is not for it while the tool holds a
        // point: it is the tool's own step back.
        assert!(window.exec.document.history().can_undo());
        press(&mut window, &mut headless, Key::Z, ctrl());
        assert!(taken(&headless).is_empty(), "{:?}", taken(&headless));
        assert_eq!(entities(&window), 1, "the drawing was not undone under the tool");
        assert!(!window.exec.workspace.active_tool_id().is_empty(), "the tool runs on");

        // The drawing's Undo is Escape and then Ctrl+Z (docs/tools.md).
        press(&mut window, &mut headless, Key::Escape, Modifiers::NONE);
        assert_eq!(taken(&headless), ["editDeselect"]);
        assert!(window.exec.workspace.active_tool_id().is_empty(), "Escape ended the tool");
        press(&mut window, &mut headless, Key::Z, ctrl());
        assert_eq!(taken(&headless), ["editUndo"]);
        assert_eq!(entities(&window), 0);
    }

    #[test]
    fn escape_ends_a_waiting_point_pick_and_keeps_the_selection() {
        let (mut window, mut headless) = headless_window();
        line(&mut window, "LINE 0,0 10,0");
        window.select_all();
        let plan = window
            .exec
            .workspace
            .active_plan_view()
            .expect("a plan view is open");
        let waiting = |window: &MainWindow| {
            window
                .exec
                .workspace
                .with_plan_view(plan, |view, _| view.point_pick_waiting())
        };
        let _ = window
            .exec
            .workspace
            .with_plan_view(plan, |view, _| view.await_point_pick(true));
        assert_eq!(waiting(&window), Some(true));

        // The pick's Escape: Deselect, which holds the key, would have cleared the selection and
        // left the pick waiting.
        press(&mut window, &mut headless, Key::Escape, Modifiers::NONE);
        assert!(taken(&headless).is_empty(), "{:?}", taken(&headless));
        assert_eq!(waiting(&window), Some(false));
        assert_eq!(selected(&window), 1);

        // With no pick waiting, Escape is Deselect's.
        press(&mut window, &mut headless, Key::Escape, Modifiers::NONE);
        assert_eq!(taken(&headless), ["editDeselect"]);
        assert_eq!(selected(&window), 0);
    }

    #[test]
    fn ctrl_p_over_the_3d_view_opens_plot_and_is_not_also_the_views_p() {
        let (mut window, mut headless) = headless_window();
        line(&mut window, "VIEWS OPEN 3d");
        settle(&mut window, &mut headless);
        let view = view_drawn(&headless, "RenderView");
        click(&mut window, &mut headless, view.rect.center());
        assert_eq!(focused(&headless), Some(view.id), "the click gave the view the keyboard");
        let before = projection(&window);

        // P alone switches a focused 3D view's projection. Ctrl+P is File > Plot to PDF's, and
        // the view never sees its P.
        press(&mut window, &mut headless, Key::P, ctrl());
        assert_eq!(taken(&headless), ["filePlot"]);
        assert_eq!(projection(&window), before);
        assert!(
            headless.snapshot().target("plotDrawingDialog").is_some(),
            "Plot to PDF opened"
        );
    }
}
