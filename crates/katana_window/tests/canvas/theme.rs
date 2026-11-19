//! The twins of tests/qt_widgets/test_theme.cpp's Theme cases that are the theme's (theme::style):
//! a menu section shows its title, a long choice list scrolls, the command line's font is fixed
//! pitch and every font follows the text size, the text sizes read back from their names, and a
//! checked menu item shows a tick before its icon. The two title-bar cases are D's (dock_chrome)
//! and Icons is I's.
//!
//! The C++ grabbed each widget into an image and counted its ink. The egui twins read what egui
//! was asked to paint - its shapes, the input of every renderer - where the C++ read the pixels:
//! a text's ink is its glyphs' quads (the galley's mesh bounds), a rule's ink its stroke, and a
//! pixel row holds ink when its centre lies inside them. The window's own renderer arrives with
//! the soft renderer (D); what is compared here is what any renderer draws.

use std::collections::BTreeSet;

use egui::epaint::ClippedShape;
use egui::epaint::ColorMode;
use egui::{Modifiers, Order, PointerButton, Pos2, Rect, Shape, TextStyle, Ui};
use katana_window::canvas::{Font, FontMetrics, FontSize, TextMetrics, resolved_family};
use katana_window::drive::{self, Action, Choice, Harness, action_item};
use katana_window::shell::popup_menu;
use katana_window::theme::palette;
use katana_window::theme::style::{
    MAX_VISIBLE_ITEMS, MENU_CHECK_WIDTH, MENU_GUTTER_WIDTH, apply, menu_section, menu_separator,
    monospace_font, monospace_font_id, overlay_font, set_text_size, text_size, ui_font,
};
use katana_window::theme::{TextSize, text_size_from, text_size_steps};

use crate::double_eq;

/// The pixel rows of `area` whose centres lie inside `ink`.
fn mark(ink: Rect, area: Rect, rows: &mut BTreeSet<i32>) {
    let inside = ink.intersect(area);
    if inside.width() < 0.0 || inside.height() < 0.0 {
        return;
    }
    let first = (inside.top() - 0.5).ceil() as i32;
    let last = (inside.bottom() - 0.5).floor() as i32;
    for row in first..=last {
        rows.insert(row);
    }
}

fn collect(shape: &Shape, area: Rect, rows: &mut BTreeSet<i32>) {
    match shape {
        Shape::Vec(shapes) => {
            for each in shapes {
                collect(each, area, rows);
            }
        }
        Shape::Text(text) => {
            if !text.galley.is_empty() {
                mark(
                    text.galley.mesh_bounds.translate(text.pos.to_vec2()),
                    area,
                    rows,
                );
            }
        }
        Shape::LineSegment { points, stroke } if !stroke.is_empty() => {
            let ink = Rect::from_two_pos(points[0], points[1]).expand(stroke.width / 2.0);
            mark(ink, area, rows);
        }
        _ => {}
    }
}

/// Rows of `area` holding a text's or a rule's ink (test_theme.cpp's inkRows): a line of text is
/// several rows, a rule one or two, whatever the font.
fn ink_rows(shapes: &[ClippedShape], area: Rect) -> usize {
    let mut rows = BTreeSet::new();
    for clipped in shapes {
        collect(&clipped.shape, area, &mut rows);
    }
    rows.len()
}

/// What is drawn inside `row`, placed from the row's corner: what two rows alike draw alike.
fn shapes_in(shapes: &[ClippedShape], row: Rect) -> Vec<Shape> {
    let room = row.expand(0.5);
    shapes
        .iter()
        .flat_map(|clipped| match &clipped.shape {
            Shape::Vec(inner) => inner.clone(),
            other => vec![other.clone()],
        })
        .filter(|shape| room.contains_rect(shape.visual_bounding_rect()))
        .map(|mut shape| {
            shape.translate(-row.min.to_vec2());
            shape
        })
        .collect()
}

/// Whether a shape is drawn in the accent's blue: blue at least twice red and not near black -
/// the theme's greys have blue within a few steps of red, and the accent #4c9ffe 254 against 76.
fn is_blue(shape: &Shape) -> bool {
    let blue =
        |color: egui::Color32| color.b() > 40 && u16::from(color.b()) > 2 * u16::from(color.r());
    match shape {
        Shape::Rect(rect) => blue(rect.stroke.color) || blue(rect.fill),
        Shape::LineSegment { stroke, .. } => blue(stroke.color),
        Shape::Path(path) => matches!(path.stroke.color, ColorMode::Solid(color) if blue(color)),
        _ => false,
    }
}

/// The font's size in points; every font of the theme but the overlay's is given in points.
fn points(font: &Font) -> f64 {
    match font.size {
        FontSize::Points(points) => points,
        FontSize::Pixels(pixels) => panic!("a font in points was expected, not {pixels} px"),
    }
}

/// The text size put back when a case that changes it ends (test_theme.cpp's TextSizeGuard).
struct TextSizeGuard(egui::Context);

impl Drop for TextSizeGuard {
    fn drop(&mut self) {
        set_text_size(&self.0, TextSize::Standard);
    }
}

mod theme {
    use super::*;

    #[test]
    fn a_menu_section_shows_its_title_under_the_stylesheet() {
        // The Survey, Terrain and GIS menus had titled sections that drew as bare lines: Fusion
        // does not support sections, and the stylesheet drew the separators itself. Both at once,
        // as the application has them.
        let mut harness = Harness::new();
        apply(harness.context());
        let items = [
            Action::new("surfaceFromDrawing", "Surface From Drawing"),
            Action::new("cutSection", "Cut Section"),
        ];
        let at = Pos2::new(40.0, 40.0);
        let mut title_area = Rect::NOTHING;
        let mut line_area = Rect::NOTHING;
        let mut font_height = 0.0;
        let mut draw = |ui: &mut Ui| {
            let ctx = ui.ctx().clone();
            popup_menu(&ctx, "sectionMenu", at, |ui| {
                title_area = menu_section(ui, "Surfaces").rect;
                action_item(ui, &items[0]);
                line_area = menu_separator(ui).rect;
                action_item(ui, &items[1]);
                font_height = ui.text_style_height(&TextStyle::Body);
            });
        };
        harness.show(&mut draw).expect("the menu is drawn");
        let output = harness.frame(&mut draw);

        assert!(title_area.is_positive());
        assert!(line_area.is_positive());
        // Room for a line of text, where a plain separator is a sliver.
        assert!(
            title_area.height() >= font_height,
            "title {} high, a line of the menu's font {font_height}",
            title_area.height()
        );
        assert!(line_area.height() < title_area.height());
        // The title is letters, rows of ink; the plain separator one rule.
        let title = ink_rows(&output.shapes, title_area);
        let line = ink_rows(&output.shapes, line_area);
        output.drop_without_applying_deltas();
        assert!(line >= 1, "the plain separator draws its rule");
        assert!(line <= 2);
        assert!(title >= line + 4, "title {title} rows, line {line}");
    }

    #[test]
    fn a_long_choice_list_scrolls_rather_than_running_off_the_screen() {
        // Fusion dropped a list that cannot be typed into as a popup the height of every item,
        // whatever maxVisibleItems said: a drawing's two hundred styles ran off the screen.
        let mut harness = Harness::new();
        apply(harness.context());
        let items: Vec<String> = (0..200).map(|at| format!("Style {at}")).collect();
        let mut chosen = Choice::at(0);
        let mut draw = |ui: &mut Ui| {
            drive::choice(ui, "styleBox", &items, &mut chosen);
        };
        harness.show(&mut draw).expect("the box is drawn");
        let centre = harness
            .control("styleBox")
            .expect("the box is recorded")
            .rect
            .center();
        // A click on the box opens its list.
        harness.pointer_move(centre).pointer_button(
            centre,
            PointerButton::Primary,
            true,
            Modifiers::NONE,
        );
        harness.frame(&mut draw).drop_without_applying_deltas();
        harness.pointer_button(centre, PointerButton::Primary, false, Modifiers::NONE);
        harness.frame(&mut draw).drop_without_applying_deltas();
        harness.run(&mut draw).expect("the open list settles");

        let ctx = harness.context().clone();
        let list = ctx
            .memory(|memory| {
                memory
                    .areas()
                    .top_layer_id(Order::Foreground)
                    .and_then(|layer| memory.area_rect(layer.id))
            })
            .expect("the list is open");
        // A row of the list: a selectable line of the list's font, its padding, the spacing.
        let style = ctx.global_style();
        let font = style.text_styles[&TextStyle::Body].clone();
        let line = ctx.fonts_mut(|fonts| fonts.row_height(&font));
        let row = (line + 2.0 * style.spacing.button_padding.y).max(style.spacing.interact_size.y)
            + style.spacing.item_spacing.y;
        assert!(row > 0.0);
        // maxVisibleItems rows and the frame, not two hundred.
        assert!(
            list.height() <= (MAX_VISIBLE_ITEMS + 2) as f32 * row,
            "the list is {} high, a row {row}",
            list.height()
        );
    }

    #[test]
    fn the_command_lines_font_is_fixed_pitch_and_every_font_follows_the_text_size() {
        let mut harness = Harness::new();
        let ctx = harness.context().clone();
        apply(&ctx);
        let _guard = TextSizeGuard(ctx.clone());
        set_text_size(&ctx, TextSize::Standard);
        // egui's fonts are there from the first frame (QFontInfo of the font as resolved).
        harness.frame(|_| {}).drop_without_applying_deltas();
        let fixed = monospace_font_id(0.0);
        let (narrow, wide) = ctx.fonts_mut(|fonts| {
            (
                fonts.glyph_width(&fixed, 'i'),
                fonts.glyph_width(&fixed, 'W'),
            )
        });
        assert_eq!(narrow, wide, "the command line's face is fixed pitch");
        // And a painter's fixed-pitch font resolves to a fixed-pitch face on this machine.
        let metrics = TextMetrics::screen();
        let mono = monospace_font(0.0);
        assert_eq!(
            metrics.horizontal_advance(&mono, "iiii"),
            metrics.horizontal_advance(&mono, "WWWW"),
            "{:?}",
            resolved_family(&mono)
        );

        let standard = points(&ui_font());
        assert!(standard > 0.0);
        assert!(double_eq(points(&monospace_font(0.0)), standard));
        assert!(double_eq(points(&monospace_font(1.0)), standard + 1.0));
        assert_eq!(overlay_font(12).size, FontSize::Pixels(12));

        // Large is two points more, everywhere, and the application font with it.
        set_text_size(&ctx, TextSize::Large);
        assert_eq!(text_size(), TextSize::Large);
        assert!(double_eq(points(&ui_font()), standard + 2.0));
        let body = ctx.global_style().text_styles[&TextStyle::Body].size;
        assert_eq!(body, ((standard + 2.0) * 96.0 / 72.0) as f32);
        assert!(double_eq(points(&monospace_font(0.0)), standard + 2.0));
        // 12 px scaled as the points are.
        assert_eq!(
            overlay_font(12).size,
            FontSize::Pixels((12.0 * (standard + 2.0) / standard).round() as i32)
        );
    }

    #[test]
    fn text_sizes_read_back_from_their_names_and_nothing_else_does() {
        for size in [
            TextSize::Small,
            TextSize::Standard,
            TextSize::Large,
            TextSize::ExtraLarge,
        ] {
            assert_eq!(text_size_from(palette::to_string(size)), Some(size));
        }
        assert_eq!(text_size_from("huge"), None);
        assert_eq!(text_size_from(""), None);
        // The stored spelling only.
        assert_eq!(text_size_from("Large"), None);
        assert_eq!(text_size_steps(TextSize::Small), -1);
        assert_eq!(text_size_steps(TextSize::ExtraLarge), 4);
    }

    #[test]
    fn a_checked_menu_item_shows_a_tick_before_its_icon_and_an_unchecked_one_does_not() {
        // Grid, Object Snap, every toolbar of View > Toolbars: egui's selectable button drew a
        // checkable item that was on as a pill filled with the selection's blue, fourteen of them
        // a column of pressed keys, and no item showed its icon (the look of 2026-11-16). Two
        // items alike but for the check, one above the other, the unchecked one the baseline:
        // what differs is the checked one's tick - the accent, in the check column before the
        // icon - and nothing else; both show their icon after the check column.
        let mut harness = Harness::new();
        apply(harness.context());
        katana_window::icons::install(
            harness.context(),
            katana_window::icons::default_rasteriser().expect("the window's rasteriser"),
        );
        let on = Action::new("gridOn", "Grid").icon("Grid").checkable(true);
        let off = Action::new("gridOff", "Grid").icon("Grid").checkable(false);
        let at = Pos2::new(40.0, 40.0);
        let mut on_row = Rect::NOTHING;
        let mut off_row = Rect::NOTHING;
        let mut padding = 0.0;
        let mut draw = |ui: &mut Ui| {
            let ctx = ui.ctx().clone();
            popup_menu(&ctx, "gridMenu", at, |ui| {
                egui::containers::menu::menu_style(ui.style_mut());
                padding = ui.spacing().button_padding.x;
                on_row = action_item(ui, &on).rect;
                off_row = action_item(ui, &off).rect;
            });
        };
        harness.show(&mut draw).expect("the menu is drawn");
        let output = harness.frame(&mut draw);

        assert_eq!(on_row.size(), off_row.size());
        let on_shapes = shapes_in(&output.shapes, on_row);
        let off_shapes = shapes_in(&output.shapes, off_row);
        output.drop_without_applying_deltas();
        let differ: Vec<&Shape> = on_shapes
            .iter()
            .filter(|&shape| !off_shapes.contains(shape))
            .collect();
        assert!(!differ.is_empty(), "checked and unchecked look alike");
        assert!(differ.iter().copied().any(is_blue), "the tick is the accent");
        let ticked = differ.iter().fold(Rect::NOTHING, |all, shape| {
            all.union(shape.visual_bounding_rect())
        });
        // Shapes are placed from the row's corner: the check column is the gutter's first
        // MENU_CHECK_WIDTH, after the item's button padding.
        assert!(
            ticked.left() >= padding - 0.5 && ticked.right() <= padding + MENU_CHECK_WIDTH + 0.5,
            "the tick in the check column: {ticked:?}"
        );
        assert!(ticked.width() >= 6.0 && ticked.height() >= 4.0, "a tick, not a dot: {ticked:?}");
        // The icon - an image, the only mesh a menu item paints - in the gutter after the check
        // column, on the unchecked item as on the checked one.
        for shapes in [&on_shapes, &off_shapes] {
            let icon = shapes
                .iter()
                .find(|shape| matches!(shape, Shape::Mesh(_)))
                .map(Shape::visual_bounding_rect)
                .expect("the item shows its icon");
            assert!(
                icon.left() >= padding + MENU_CHECK_WIDTH - 0.5
                    && icon.right() <= padding + MENU_GUTTER_WIDTH + 0.5,
                "the icon in its column: {icon:?}"
            );
        }
    }

    #[test]
    fn a_menus_widest_plain_row_ends_as_far_in_from_its_right_edge_as_its_icons_start_from_its_left()
    {
        // The seventh look audit: a menu is as wide as its widest row, and where that row had no
        // shortcut or arrow ("Sheets and Plotting Commands...", "Project Coordinate System...")
        // its text ran to 3 pixels from the popup's right edge, while the icons stood 15 in from
        // its left and a shortcut 16 in from its right. The plain row is made the widest here, a
        // row with a shortcut beside it; both must keep at least the icons' inset on the right.
        let mut harness = Harness::new();
        apply(harness.context());
        katana_window::icons::install(
            harness.context(),
            katana_window::icons::default_rasteriser().expect("the window's rasteriser"),
        );
        let plain_text = "Sheets and Plotting Commands Reference...";
        let menu = drive::Menu::new("helpMenu", "&Help")
            .action(
                Action::new("helpCommandReference", "Command Reference...")
                    .icon("Grid")
                    .shortcut(egui::KeyboardShortcut::new(Modifiers::NONE, egui::Key::F1)),
            )
            .action(Action::new("helpSheetsCommands", plain_text).icon("Grid"));
        let at = Pos2::new(40.0, 40.0);
        let mut popup = Rect::NOTHING;
        let mut draw = |ui: &mut Ui| {
            let ctx = ui.ctx().clone();
            let mut chosen = None;
            popup = popup_menu(&ctx, "helpMenu", at, |ui| {
                drive::menu_items_ui(ui, &menu, &mut chosen);
            })
            .rect;
        };
        harness.show(&mut draw).expect("the menu is drawn");
        let output = harness.frame(&mut draw);
        let plain_row = harness
            .control("helpSheetsCommands")
            .expect("the plain row is recorded")
            .rect;
        let shortcut_row = harness
            .control("helpCommandReference")
            .expect("the shortcut row is recorded")
            .rect;
        let shapes: Vec<Shape> = output
            .shapes
            .iter()
            .flat_map(|clipped| match &clipped.shape {
                Shape::Vec(inner) => inner.clone(),
                other => vec![other.clone()],
            })
            .collect();
        output.drop_without_applying_deltas();
        let text_ink = |wanted: &str| {
            shapes
                .iter()
                .find_map(|shape| match shape {
                    Shape::Text(text) if text.galley.text() == wanted => {
                        Some(text.galley.mesh_bounds.translate(text.pos.to_vec2()))
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{wanted:?} is drawn"))
        };
        let icon = shapes
            .iter()
            .filter(|shape| matches!(shape, Shape::Mesh(_)))
            .map(Shape::visual_bounding_rect)
            .find(|icon| plain_row.expand(0.5).contains_rect(*icon))
            .expect("the plain row shows its icon");
        let left_inset = icon.left() - popup.left();
        assert!(left_inset > 0.0, "the icon inside the popup: {icon:?} in {popup:?}");
        // The rows are the column's whole width, the popup's frame around them.
        assert!(
            (plain_row.width() - shortcut_row.width()).abs() < 0.5,
            "the rows alike wide: {plain_row:?} {shortcut_row:?}"
        );
        let plain_ink = text_ink(plain_text);
        assert!(
            popup.right() - plain_ink.right() >= left_inset - 0.5,
            "the plain row's text ends {} in from the right, its icon starts {left_inset} in from \
             the left",
            popup.right() - plain_ink.right()
        );
        let shortcut_ink = text_ink("F1");
        assert!(
            popup.right() - shortcut_ink.right() >= left_inset - 0.5,
            "the shortcut ends {} in from the right, the icon starts {left_inset} in from the left",
            popup.right() - shortcut_ink.right()
        );
        // The plain row is the widest: its text and the trail after it set the popup's width,
        // so the trail is no wider than the icons' inset either - the two sides match.
        assert!(
            popup.right() - plain_ink.right() <= left_inset + MENU_CHECK_WIDTH,
            "no wide empty strip after the widest row's text: {}",
            popup.right() - plain_ink.right()
        );
    }
}
