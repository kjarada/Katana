//! The twins of tests/qt_widgets/test_theme.cpp's Theme cases that are the theme's (theme::style):
//! a menu section shows its title, a long choice list scrolls, the command line's font is fixed
//! pitch and every font follows the text size, the text sizes read back from their names, and a
//! checked menu item's icon is framed. The two title-bar cases are D's (dock_chrome) and Icons is
//! I's.
//!
//! The C++ grabbed each widget into an image and counted its ink. The egui twins read what egui
//! was asked to paint - its shapes, the input of every renderer - where the C++ read the pixels:
//! a text's ink is its glyphs' quads (the galley's mesh bounds), a rule's ink its stroke, and a
//! pixel row holds ink when its centre lies inside them. The window's own renderer arrives with
//! the soft renderer (D); what is compared here is what any renderer draws.

use std::collections::BTreeSet;

use egui::epaint::ClippedShape;
use egui::{Modifiers, Order, PointerButton, Pos2, Rect, Shape, TextStyle, TextureId, Ui};
use katana_window::canvas::{
    Brush, Canvas, Color, Font, FontMetrics, FontSize, Image, Pen, PointF, RasterCanvas, RectF,
    TextMetrics, resolved_family,
};
use katana_window::drive::{self, Action, Choice, Harness, action_item};
use katana_window::shell::popup_menu;
use katana_window::theme::palette;
use katana_window::theme::style::{
    MAX_VISIBLE_ITEMS, apply, menu_icon_cell, menu_section, menu_separator, monospace_font,
    monospace_font_id, overlay_font, set_text_size, text_size, ui_font,
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
        _ => false,
    }
}

/// A grid of three lines each way, 16 px: the Grid icon's shape, painted on the canvas as the
/// icons are.
fn grid_icon() -> Image {
    let mut canvas = RasterCanvas::new(16, 16);
    canvas.set_pen(Pen::new(Color::WHITE));
    for at in [2.0, 8.0, 14.0] {
        canvas.draw_line(PointF::new(at + 0.5, 1.0), PointF::new(at + 0.5, 15.0));
        canvas.draw_line(PointF::new(1.0, at + 0.5), PointF::new(15.0, at + 0.5));
    }
    canvas.fill_rect(
        RectF::new(7.0, 7.0, 3.0, 3.0),
        &Brush::solid(palette::accent()),
    );
    canvas.into_image()
}

/// One row of a menu: an item with an icon, checked or not - the icon's cell, the text, and the
/// stylesheet's QMenu::item padding-right after it.
fn menu_row(ui: &mut Ui, icon: TextureId, checked: bool) -> Rect {
    ui.horizontal(|ui| {
        menu_icon_cell(ui, Some(icon), checked);
        ui.label("Grid");
        ui.add_space(28.0);
    })
    .response
    .rect
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
    fn a_checked_menu_item_with_an_icon_is_framed_and_an_unchecked_one_is_not() {
        // Grid, Object Snap, Link This View: the stylesheet drew a checkable item that has an
        // icon with no check mark at all, and on looked exactly like off. Two items alike but
        // for the check, one above the other, the unchecked one the baseline: what differs is
        // the checked one's frame - the accent's blue, round the icon, which is 16 px across -
        // and nothing else.
        let mut harness = Harness::new();
        apply(harness.context());
        let texture = harness.context().load_texture(
            "grid",
            grid_icon().to_color_image(),
            egui::TextureOptions::default(),
        );
        let icon = texture.id();
        let at = Pos2::new(40.0, 40.0);
        let mut on_row = Rect::NOTHING;
        let mut off_row = Rect::NOTHING;
        let mut draw = |ui: &mut Ui| {
            let ctx = ui.ctx().clone();
            popup_menu(&ctx, "gridMenu", at, |ui| {
                on_row = menu_row(ui, icon, true);
                off_row = menu_row(ui, icon, false);
            });
        };
        harness.show(&mut draw).expect("the menu is drawn");
        let output = harness.frame(&mut draw);

        assert_eq!(on_row.size(), off_row.size());
        let on = shapes_in(&output.shapes, on_row);
        let off = shapes_in(&output.shapes, off_row);
        output.drop_without_applying_deltas();
        let differ: Vec<&Shape> = on.iter().filter(|&shape| !off.contains(shape)).collect();
        assert!(!differ.is_empty(), "checked and unchecked look alike");
        let blue = differ.iter().copied().any(is_blue);
        let framed = differ.iter().fold(Rect::NOTHING, |all, shape| {
            all.union(shape.visual_bounding_rect())
        });
        assert!(blue, "the frame is the accent");
        assert!(framed.width() >= 16.0, "round the icon: {framed:?}");
        assert!(framed.height() >= 16.0, "round the icon: {framed:?}");
        assert!(
            framed.right() < on_row.width() / 2.0,
            "the icon's cell, not the words: {framed:?} in a row {} wide",
            on_row.width()
        );
        drop(texture);
    }
}
