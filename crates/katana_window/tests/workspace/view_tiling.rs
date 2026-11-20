//! Tiling the open views (the owner's request of 2026-11-21 for "a good way to arrange them";
//! docs/desktop.md, "Tiling the open views"). Not twins: the C++ window had the presets only.
//!
//! View > Viewport Layout's Tile items and VIEWS TILE arrange the views that are open - opening
//! none, closing none - each in a place of its own of equal size, the active view first and the
//! rest in the order opened. Every expected place is worked from the workspace's own rectangle
//! and its separator (`share`), never read back from a layout: `k` places across `extent` points
//! are (extent - (k - 1) separators) / k each, to the point egui_dock's pixel grid leaves over.

mod view_tiling {
    use egui::{Pos2, Rect, vec2};
    use katana_cad::view_set::ViewId;
    use katana_cad::view_verbs::TileOutcome;
    use katana_cad::viewport_layout::{TileKind, ViewKind};
    use katana_window::MainWindow;
    use katana_window::drive::MenuItem;
    use katana_window::drive::steps::find_menu;

    use crate::support::Site;

    fn rect(w: &Site, id: ViewId) -> Rect {
        w.views.dock_rect(id).unwrap_or_else(|| {
            panic!(
                "view {id} is docked (minimised={} floating={})",
                w.views.is_minimised(id),
                w.views.is_floating(id)
            )
        })
    }

    /// The length of each of `parts` equal places along `extent` points, a separator between
    /// each two, rounded down to a whole point.
    fn share(w: &Site, extent: f32, parts: usize) -> f32 {
        let gaps = w.views.separator() * (parts - 1) as f32;
        ((extent - gaps) / parts as f32).floor()
    }

    /// Within the point an odd remainder leaves over.
    fn near(actual: Rect, x: f32, y: f32, width: f32, height: f32) -> bool {
        (actual.min.x - x).abs() <= 1.0
            && (actual.min.y - y).abs() <= 1.0
            && (actual.width() - width).abs() <= 1.0
            && (actual.height() - height).abs() <= 1.0
    }

    /// Every view of `ids` is seen in a place of its own: docked, the page its place shows (a view
    /// tabbed behind another is not on screen), and no two places overlapping.
    fn each_has_its_own_place(w: &Site, ids: &[ViewId]) {
        for &id in ids {
            assert!(w.views.on_screen(id), "view {id} is seen");
            assert!(!w.views.is_floating(id), "view {id} is docked");
            assert!(!w.views.is_minimised(id), "view {id} is not in the tray");
        }
        for (at, &one) in ids.iter().enumerate() {
            for &other in &ids[at + 1..] {
                let (a, b) = (rect(w, one), rect(w, other));
                assert!(
                    !a.intersects(b),
                    "views {one} and {other} share a place: {a:?} and {b:?}"
                );
            }
        }
    }

    /// The plan the workspace starts with, then a 3D view and a section, the section active.
    fn three_views() -> (Site, [ViewId; 3]) {
        let mut w = Site::new();
        let plan = w.active();
        let model = w.open(ViewKind::Model3D);
        let section = w.open(ViewKind::Section);
        assert_eq!(w.active(), section, "the view opened last is the active one");
        (w, [plan, model, section])
    }

    #[test]
    fn tiling_three_views_gives_each_its_own_place_two_over_one_the_active_view_first() {
        let (mut w, [plan, model, section]) = three_views();
        let outcome = w.views.tile(TileKind::Grid);
        w.frame();
        assert_eq!(
            outcome,
            TileOutcome {
                tiled: 3,
                docked: 0,
                minimised: 0
            }
        );
        // None opened, none closed.
        assert_eq!(w.views.view_ids(), [plan, model, section]);
        each_has_its_own_place(&w, &[plan, model, section]);

        // Three in a grid are two over one (tile_rows): the active section first, the plan -
        // opened before the 3D view - beside it, and the 3D view across the whole width below.
        let area = w.views.rect();
        let gap = w.views.separator();
        let half_width = share(&w, area.width(), 2);
        let half_height = share(&w, area.height(), 2);
        assert!(near(rect(&w, section), 0.0, 0.0, half_width, half_height));
        assert!(near(
            rect(&w, plan),
            half_width + gap,
            0.0,
            half_width,
            half_height
        ));
        assert!(near(
            rect(&w, model),
            0.0,
            half_height + gap,
            area.width(),
            half_height
        ));
        // The places and the separators between them are the whole workspace.
        assert_eq!(
            rect(&w, section).width() + gap + rect(&w, plan).width(),
            area.width()
        );
        assert_eq!(
            rect(&w, section).height() + gap + rect(&w, model).height(),
            area.height()
        );
        // The active view is the one it was.
        assert_eq!(w.active(), section);
    }

    #[test]
    fn tiling_five_views_gives_three_over_two_each_row_in_equal_widths() {
        let mut w = Site::new();
        let plan = w.active();
        let model = w.open(ViewKind::Model3D);
        let section = w.open(ViewKind::Section);
        let elevation = w.open(ViewKind::Elevation);
        let second = w.open(ViewKind::Plan);
        assert_eq!(w.active(), second);
        assert_eq!(
            w.run("VIEWS TILE"),
            "tiled=5 kind=grid docked=0 minimised=0"
        );
        each_has_its_own_place(&w, &[plan, model, section, elevation, second]);

        // Five in a grid are three over two: the active plan, then the first three opened, fill
        // the top row and the first place of the second.
        let area = w.views.rect();
        let gap = w.views.separator();
        let third = share(&w, area.width(), 3);
        let half_width = share(&w, area.width(), 2);
        let half_height = share(&w, area.height(), 2);
        let top = [second, plan, model];
        for (column, &id) in top.iter().enumerate() {
            assert!(
                near(
                    rect(&w, id),
                    column as f32 * (third + gap),
                    0.0,
                    third,
                    half_height
                ),
                "view {id} is {:?}",
                rect(&w, id)
            );
        }
        let bottom = [section, elevation];
        for (column, &id) in bottom.iter().enumerate() {
            assert!(
                near(
                    rect(&w, id),
                    column as f32 * (half_width + gap),
                    half_height + gap,
                    half_width,
                    half_height
                ),
                "view {id} is {:?}",
                rect(&w, id)
            );
        }
        // Equal within a row to the point, and each row the whole width.
        let widths: Vec<f32> = top.iter().map(|&id| rect(&w, id).width()).collect();
        assert!((widths[0] - widths[1]).abs() <= 1.0 && (widths[1] - widths[2]).abs() <= 1.0);
        assert_eq!(widths.iter().sum::<f32>() + 2.0 * gap, area.width());
        assert_eq!(
            rect(&w, section).width() + gap + rect(&w, elevation).width(),
            area.width()
        );
    }

    #[test]
    fn side_by_side_and_stacked_cut_the_workspace_in_equal_thirds_not_a_half_and_two_quarters() {
        let (mut w, [plan, model, section]) = three_views();
        let area = w.views.rect();
        let gap = w.views.separator();
        // The order: the active section, then the plan and the 3D view as opened.
        let order = [section, plan, model];

        assert_eq!(
            w.run("VIEWS TILE COLUMNS"),
            "tiled=3 kind=columns docked=0 minimised=0"
        );
        each_has_its_own_place(&w, &order);
        let third = share(&w, area.width(), 3);
        for (column, &id) in order.iter().enumerate() {
            assert!(
                near(
                    rect(&w, id),
                    column as f32 * (third + gap),
                    0.0,
                    third,
                    area.height()
                ),
                "view {id} is {:?}",
                rect(&w, id)
            );
            assert_eq!(rect(&w, id).height(), area.height());
        }
        let across: f32 = order.iter().map(|&id| rect(&w, id).width()).sum();
        assert_eq!(across + 2.0 * gap, area.width());

        assert_eq!(
            w.run("VIEWS TILE ROWS"),
            "tiled=3 kind=rows docked=0 minimised=0"
        );
        each_has_its_own_place(&w, &order);
        let third = share(&w, area.height(), 3);
        for (row, &id) in order.iter().enumerate() {
            assert!(
                near(
                    rect(&w, id),
                    0.0,
                    row as f32 * (third + gap),
                    area.width(),
                    third
                ),
                "view {id} is {:?}",
                rect(&w, id)
            );
            assert_eq!(rect(&w, id).width(), area.width());
        }
        let down: f32 = order.iter().map(|&id| rect(&w, id).height()).sum();
        assert_eq!(down + 2.0 * gap, area.height());
    }

    #[test]
    fn a_floating_view_is_docked_into_the_tiling_and_nothing_of_the_drawing_changes() {
        let (mut w, [plan, model, section]) = three_views();
        w.press(model, "DockFloatButton");
        assert!(w.views.is_floating(model));
        // Pressing a view's Float is working in that view: it is the active one.
        assert_eq!(w.active(), model);
        let modified = w.document().is_modified();
        let undoable = w.document().history().undo_count();

        assert_eq!(
            w.run("VIEWS TILE COLUMNS"),
            "tiled=3 kind=columns docked=1 minimised=0"
        );
        assert!(!w.views.is_floating(model));
        each_has_its_own_place(&w, &[plan, model, section]);
        // The active view first, then the others as opened.
        assert!(rect(&w, model).min.x < rect(&w, plan).min.x);
        assert!(rect(&w, plan).min.x < rect(&w, section).min.x);
        // A layout is the window's, not the drawing's: no command ran, so there is nothing to
        // undo and nothing to save.
        assert_eq!(w.document().is_modified(), modified);
        assert_eq!(w.document().history().undo_count(), undoable);
    }

    #[test]
    fn a_minimised_view_stays_in_the_tray_and_comes_back_onto_the_tiling_not_over_it() {
        let (mut w, [plan, model, section]) = three_views();
        w.press(section, "DockMinimiseButton");
        assert!(w.views.is_minimised(section));

        assert_eq!(
            w.run("VIEWS TILE"),
            "tiled=2 kind=grid docked=0 minimised=1"
        );
        // Still in the tray, and the two that show share the workspace side by side.
        assert!(w.views.is_minimised(section));
        assert_eq!(w.views.tray_actions().len(), 1);
        each_has_its_own_place(&w, &[plan, model]);
        let area = w.views.rect();
        let gap = w.views.separator();
        let half_width = share(&w, area.width(), 2);
        let plan_place = rect(&w, plan);
        let model_place = rect(&w, model);
        assert!(near(plan_place, 0.0, 0.0, half_width, area.height()));
        assert!(near(
            model_place,
            half_width + gap,
            0.0,
            half_width,
            area.height()
        ));

        // Its tray button brings it back as a page on the first tile. It once would have put
        // back the whole layout it was minimised from, undoing the tiling.
        assert!(w.views.restore_view(section));
        w.frame();
        assert!(!w.views.is_minimised(section));
        assert!(w.views.on_screen(section));
        assert!(!w.views.is_floating(section));
        assert_eq!(rect(&w, model), model_place, "the other tile is where it was");
        assert_eq!(rect(&w, section).min.x, plan_place.min.x);
        assert_eq!(rect(&w, section).width(), plan_place.width());
        // The plan is the page behind it.
        assert!(!w.views.on_screen(plan));
    }

    #[test]
    fn a_maximised_view_is_restored_before_the_views_are_tiled() {
        let mut w = Site::new();
        let plan = w.active();
        let model = w.open(ViewKind::Model3D);
        w.views.toggle_maximised(plan);
        w.frame();
        assert!(w.views.is_maximised(plan));
        assert!(!w.views.on_screen(model));

        assert_eq!(
            w.run("VIEWS TILE ROWS"),
            "tiled=2 kind=rows docked=0 minimised=0"
        );
        assert!(!w.views.is_maximised(plan));
        each_has_its_own_place(&w, &[plan, model]);
        // The plan, made active by its Maximise, on top.
        let area = w.views.rect();
        let half_height = share(&w, area.height(), 2);
        assert!(near(rect(&w, plan), 0.0, 0.0, area.width(), half_height));
        assert!(near(
            rect(&w, model),
            0.0,
            half_height + w.views.separator(),
            area.width(),
            half_height
        ));
    }

    #[test]
    fn tiling_one_view_or_none_changes_nothing_and_says_how_many_without_an_error() {
        let mut w = Site::new();
        let plan = w.active();
        let whole = rect(&w, plan);
        assert_eq!(whole, w.views.rect());
        for line in ["VIEWS TILE", "VIEWS TILE COLUMNS", "VIEWS TILE ROWS"] {
            let reply = w.run(line);
            assert!(
                reply.starts_with("tiled=1 ") && reply.ends_with(" docked=0 minimised=0"),
                "{line}: {reply}"
            );
            assert_eq!(rect(&w, plan), whole, "{line}");
        }
        // No view in the dock area at all: the one there is in the tray, and stays there.
        w.press(plan, "DockMinimiseButton");
        assert!(w.views.is_minimised(plan));
        assert_eq!(
            w.run("VIEWS TILE"),
            "tiled=0 kind=grid docked=0 minimised=1"
        );
        assert!(w.views.is_minimised(plan));
        assert_eq!(w.views.view_ids(), [plan]);
    }

    #[test]
    fn a_tiling_word_the_verb_does_not_take_is_refused_and_moves_no_view() {
        let (mut w, ids) = three_views();
        let before: Vec<Rect> = ids.iter().map(|&id| rect(&w, id)).collect();
        let refused = w
            .interpret("VIEWS TILE diagonal")
            .expect_err("no tiling is called diagonal");
        assert!(
            refused.contains("VIEWS TILE takes GRID, COLUMNS or ROWS, not 'diagonal'"),
            "{refused}"
        );
        w.frame();
        let after: Vec<Rect> = ids.iter().map(|&id| rect(&w, id)).collect();
        assert_eq!(after, before);
    }

    // ---- the window: View > Viewport Layout -----------------------------------------------------

    /// The main window with a 3D view and a section opened beside its plan, laid out 900 x 600.
    fn window_with_three_views() -> MainWindow {
        let mut window = MainWindow::new();
        assert!(window.exec.run_command("VIEWS OPEN 3d"));
        assert!(window.exec.run_command("VIEWS OPEN section"));
        window
            .exec
            .workspace
            .set_area(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 600.0)));
        window
    }

    /// Where each view of the window is docked, in the order opened.
    fn places(window: &MainWindow) -> Vec<(ViewId, Rect)> {
        let workspace = &window.exec.workspace;
        workspace
            .view_ids()
            .into_iter()
            .map(|id| {
                let place = workspace
                    .dock_rect(id)
                    .unwrap_or_else(|| panic!("view {id} is docked"));
                (id, place)
            })
            .collect()
    }

    #[test]
    fn viewport_layout_offers_the_tilings_above_the_presets_each_with_an_icon_and_its_verb() {
        let window = MainWindow::new();
        let composed = window.composed();
        let menu = find_menu(&composed.menus, "viewLayoutMenu")
            .expect("View has Viewport Layout")
            .clone();
        let rows: Vec<String> = menu
            .items
            .iter()
            .map(|item| match item {
                MenuItem::Section(title) => format!("[{title}]"),
                MenuItem::Action(action) => action.name.clone(),
                MenuItem::Menu(sub) => sub.name.clone(),
                MenuItem::Separator => String::from("-"),
            })
            .collect();
        assert_eq!(
            rows,
            [
                "[Arrange the open views]",
                "viewTile",
                "viewTileSideBySide",
                "viewTileStacked",
                "[Presets]",
                "viewLayoutSingle",
                "viewLayoutSplitVertical",
                "viewLayoutSplitHorizontal",
                "viewLayoutThreeLeft",
                "viewLayoutThreeTop",
                "viewLayoutQuad",
            ]
        );
        let expected = [
            ("viewTile", "Tile", "LayoutTile", "(VIEWS TILE)"),
            (
                "viewTileSideBySide",
                "Tile Side by Side",
                "LayoutTileColumns",
                "(VIEWS TILE COLUMNS)",
            ),
            (
                "viewTileStacked",
                "Tile Stacked",
                "LayoutTileRows",
                "(VIEWS TILE ROWS)",
            ),
        ];
        let actions = menu.actions();
        for (name, text, icon, verb) in expected {
            let action = actions
                .iter()
                .find(|action| action.name == name)
                .unwrap_or_else(|| panic!("{name} is on the menu"));
            assert_eq!(action.text, text);
            assert_eq!(action.icon, icon);
            assert!(action.tip.contains(verb), "{}", action.tip);
            assert!(action.enabled, "{name}");
            // No key of its own: the shortcut table has none that is conventionally Tile's.
            assert!(action.shortcut.is_none(), "{name}");
        }
    }

    #[test]
    fn each_tile_item_runs_its_verb_and_arranges_the_views_as_the_typed_line_does() {
        for (action, line, reply) in [
            (
                "viewTile",
                "VIEWS TILE GRID",
                "tiled=3 kind=grid docked=0 minimised=0",
            ),
            (
                "viewTileSideBySide",
                "VIEWS TILE COLUMNS",
                "tiled=3 kind=columns docked=0 minimised=0",
            ),
            (
                "viewTileStacked",
                "VIEWS TILE ROWS",
                "tiled=3 kind=rows docked=0 minimised=0",
            ),
        ] {
            let mut clicked = window_with_three_views();
            let untiled = places(&clicked);
            let undoable = clicked.exec.document.history().undo_count();
            clicked
                .trigger(action)
                .unwrap_or_else(|error| panic!("{action}: {}", error.describe()));
            // One path: the item's line is in the command log with its reply, as a typed one is.
            let log = clicked.exec.log.text();
            assert!(log.contains(line), "{action}: {log}");
            assert!(log.contains(reply), "{action}: {log}");

            let mut typed = window_with_three_views();
            assert!(typed.exec.run_command(line), "{line}");
            assert_eq!(places(&clicked), places(&typed), "{action}");

            // And it arranged them: three places, none shared, not where opening left them (a
            // half and two quarters).
            let tiled = places(&clicked);
            assert_eq!(tiled.len(), 3);
            assert_ne!(tiled, untiled, "{action}");
            for (at, (one, a)) in tiled.iter().enumerate() {
                for (other, b) in &tiled[at + 1..] {
                    assert!(!a.intersects(*b), "{action}: views {one} and {other}");
                }
            }
            // It is no edit of the drawing.
            assert_eq!(
                clicked.exec.document.history().undo_count(),
                undoable,
                "{action}"
            );
        }
    }
}
