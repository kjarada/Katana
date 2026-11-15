//! The twins of tests/qt_widgets/tools/test_flyout_button.cpp: the signs of a menu on a toolbar
//! button. A tool family's corner triangle is drawn beside its icon, never on it; a press on the
//! icon runs the tool while the triangle, a press held and a right click open the family; the
//! button is framed only while the tool its icon shows runs, its triangle lit while any of its
//! tools does, a device pixel clear of the frame; Undo's and Redo's arrows have a strip of their
//! own beside the icon; the check the window runs headless (--check-toolbars) sees an arrow
//! drawn over an icon; and a toolbar's overflow arrow is drawn whole, in whole device pixels. Its
//! TitleBarButtons suite is D's and WS's.
//!
//! The toolbars are built as the window builds them - fill_tool_menus for the families - and
//! drawn through the drive harness; what is painted is painted through the canvas with
//! test_canvas's raster. Geometry is asserted as rectangles; where pixels are compared it is a
//! button against itself or against a plain button with the same icon, never against a count.
//! Every twin holds at either ratio, 1.0 and the C++'s 125 % rerun (`RATIOS`), as its C++ holds
//! at any device-pixel ratio.

use egui::{Key, Modifiers, PointerButton, Pos2, Rect, Sense, Ui, Vec2};
use katana_cad::interactive_tool::tool_catalog;
use katana_window::canvas::{Canvas, Color, Image, IntRect, RectF};
use katana_window::drive::{Action, Harness, Menu, ToolBar, ToolBarItem};
use katana_window::theme::palette;
use katana_window::tools::flyout_button::{
    ButtonIcon, ButtonPaint, ButtonState, Flyout, FlyoutResponse, MeasuredButton, MenuSign,
    MenuZone, OVERFLOW_BUTTON, SignHolder, Strip, StripItem, StripResponse, ToolButtonLook,
    ToolButtonResponse, difference_box, drawn_parts, flyout_button, icon_rect, mark_rect,
    menu_sign_clashes, paint_overflow_arrow, paint_tool_button, rects_intersect, split_button_at,
    split_paint, split_strip, strip_sign_holder, tool_bar_strip, tooltip_runs, whole_device_pixels,
    wrap_words,
};
use katana_window::tools::{ToolActions, ToolMenuTargets, fill_tool_menus};

use crate::RATIOS;
use crate::test_canvas::{pieces_of, raster};

/// The families the catalogue has, all in the Draw category: every tool named "Circle, ...",
/// "Arc, ...", "Ellipse, ..." or "Vertices, ...".
const FAMILIES: [&str; 4] = ["Circle", "Arc", "Ellipse", "Vertices"];

/// qRound: halves up.
fn q_round(value: f64) -> i32 {
    (value + 0.5).floor() as i32
}

/// The device pixel under the centre of widget pixel (x, y): where an image at `ratio` shows what
/// the button drew there.
fn device_pixel(x: i32, y: i32, ratio: f64) -> (u32, u32) {
    (
        ((f64::from(x) + 0.5) * ratio).floor() as u32,
        ((f64::from(y) + 0.5) * ratio).floor() as u32,
    )
}

/// Whether every pixel of `inner` is in `outer` (QRect::contains).
fn contains_rect(outer: &IntRect, inner: &IntRect) -> bool {
    inner.x >= outer.x
        && inner.right() <= outer.right()
        && inner.y >= outer.y
        && inner.bottom() <= outer.bottom()
}

/// The button `paint` rendered `offset` widget pixels in from the image's corner, at `ratio`
/// (QWidget::render into an image of the button's size and the offset, times the ratio).
fn render_at(paint: &ButtonPaint, ratio: f64, offset: i32) -> Image {
    let width = q_round(f64::from(paint.width + offset) * ratio) as u32;
    let height = q_round(f64::from(paint.height + offset) * ratio) as u32;
    let raster = raster();
    katana_window::icons::Rasteriser::paint(
        &raster,
        width,
        height,
        ratio,
        &mut |canvas: &mut dyn Canvas| {
            canvas.translate(f64::from(offset), f64::from(offset));
            paint_tool_button(canvas, paint);
        },
    )
}

/// A vertical Draw toolbar as the window makes it, and beside the families a plain button
/// showing the Circle family's icon: the baseline a family button must match everywhere but its
/// mark.
struct DrawBar {
    targets: ToolMenuTargets,
    actions: ToolActions,
    icon_size: i32,
}

/// The plain button's object name: a checkable action that is no tool.
const PLAIN: &str = "plainCircle";

fn draw_bar(icon_size: i32) -> DrawBar {
    let mut targets = ToolMenuTargets::default();
    targets
        .menus
        .insert(String::from("Draw"), Menu::new("drawMenu", "Draw"));
    targets
        .tool_bars
        .insert(String::from("Draw"), ToolBar::new("DrawToolBar", "Draw"));
    let actions = fill_tool_menus(tool_catalog(), &mut targets);
    let plain = Action::new(PLAIN, "Plain Circle")
        .icon("draw.circle")
        .checkable(false);
    if let Some(bar) = targets.tool_bars.get_mut("Draw") {
        bar.items.push(ToolBarItem::Action(plain));
    }
    DrawBar {
        targets,
        actions,
        icon_size,
    }
}

impl DrawBar {
    fn strip(&self) -> Strip {
        Strip {
            name: String::from("DrawToolBar"),
            title: String::from("Draw"),
            vertical: true,
            icon_size: self.icon_size,
            items: self
                .actions
                .strip_items(&self.targets.tool_bars["Draw"], &|_| None),
        }
    }

    fn family(&self, name: &str) -> Flyout {
        self.actions
            .flyout(&format!("toolFamilyButton.Draw.{name}"))
            .expect("the Draw toolbar has the family's button")
    }

    fn set_plain_checked(&mut self, on: bool) {
        if let Some(bar) = self.targets.tool_bars.get_mut("Draw") {
            for item in &mut bar.items {
                if let ToolBarItem::Action(action) = item
                    && action.name == PLAIN
                {
                    action.checked = Some(on);
                }
            }
        }
    }
}

/// The plain button with the Circle family's icon, at `icon_size`, in `state`.
fn twin_paint(icon_size: i32, state: ButtonState) -> ButtonPaint {
    let look = ToolButtonLook::TOOL;
    let (width, height) = look.button_size(icon_size);
    ButtonPaint {
        width,
        height,
        look,
        icon: ButtonIcon::Named(String::from("draw.circle")),
        icon_size,
        state,
        sign: MenuSign::None,
        ground: Color::TRANSPARENT,
    }
}

/// What a family button at `size` px icons draws, against the design. By hand, under the theme a
/// button is its icon, Qt's 3 px, 4 px of padding and a 1 px border each side: 13 px more than
/// its icon, the icon's square centred 6 px in. The corner mark is 5 px, inset 2 px from the
/// edge: at (s + 13 - 1 - 2) - 4 = s + 6, which leaves (s + 6) - (6 + s - 1) - 1 = 0 px between it
/// and the icon's square - they meet, never overlap - at every size s.
///
/// On a scaled screen, in the image's device pixels: the icon a pixmap of s times the ratio; the
/// mark a square of 5 px times the ratio taken down to whole pixels, set 2 px times the ratio
/// (taken down) in from the button's last device pixel and at least a whole device pixel short of
/// the first its 1 px border touches, widget pixel s + 12 times the ratio, taken down. At 125%
/// and 20 px icons: the button's 33 px are 41.25, rendered as 41, its last pixel 40, and its
/// border from 40.0; the mark 6 px ending at 38, 33..38; the icon's 25 px from 7.5, drawn from 8
/// - 8..32.
fn expect_drawn_as_designed(paint: &ButtonPaint, size: i32, ratio: f64, at: &str) {
    let parts = drawn_parts(paint, ratio, &raster());
    let icon_device = parts
        .icon_device
        .unwrap_or_else(|| panic!("{at}: no icon drawn"));
    let sign_device = parts
        .sign_device
        .unwrap_or_else(|| panic!("{at}: no mark drawn"));
    if ratio == 1.0 {
        assert_eq!(parts.icon, Some(IntRect::new(6, 6, size, size)), "{at}");
        assert_eq!(
            parts.sign,
            Some(IntRect::new(size + 6, size + 6, 5, 5)),
            "{at}"
        );
    }
    let side = q_round(f64::from(size) * ratio);
    assert_eq!(
        (icon_device.width, icon_device.height),
        (side, side),
        "{at}"
    );
    let leg = whole_device_pixels(5, ratio);
    let last = q_round(f64::from(size + 13) * ratio) - 1;
    let corner =
        (last - whole_device_pixels(2, ratio)).min(whole_device_pixels(size + 12, ratio) - 2);
    assert_eq!(
        sign_device,
        IntRect::new(corner - leg + 1, corner - leg + 1, leg, leg),
        "{at}"
    );
    assert!(!rects_intersect(&sign_device, &icon_device), "{at}");
}

/// One frame of `flyout` drawn alone, at 20 px icons on a vertical bar.
fn show(harness: &mut Harness, flyout: &Flyout) -> FlyoutResponse {
    let mut shown = None;
    harness.frame(|ui| shown = Some(flyout_button(ui, flyout, 20, true)));
    shown.expect("the button is drawn")
}

/// Widget pixel (x, y) of a button whose corner is `origin`, at the pixel's centre.
fn point(origin: Pos2, x: i32, y: i32) -> Pos2 {
    origin + Vec2::new(x as f32 + 0.5, y as f32 + 0.5)
}

/// Somewhere on the window away from every button: the C++ tests' canvas centre.
const AWAY: Pos2 = Pos2::new(600.0, 400.0);

/// A press and a release with `button` at `at`, a frame each: what the two frames did.
fn click(
    harness: &mut Harness,
    flyout: &Flyout,
    at: Pos2,
    button: PointerButton,
) -> Vec<FlyoutResponse> {
    harness
        .pointer_move(at)
        .pointer_button(at, button, true, Modifiers::NONE);
    let pressed = show(harness, flyout);
    harness.pointer_button(at, button, false, Modifiers::NONE);
    let released = show(harness, flyout);
    vec![pressed, released]
}

/// Escape, which closes an open menu, as the C++ tests' MenuWatch closes it.
fn close_menu(harness: &mut Harness, flyout: &Flyout) {
    harness.key(Key::Escape, Modifiers::NONE);
    let after = show(harness, flyout);
    assert!(!after.menu_open, "Escape did not close the family's menu");
}

/// A frame of the Draw toolbar in `room` points.
fn show_strip(harness: &mut Harness, built: &DrawBar, room: f32) -> StripResponse {
    let strip = built.strip();
    let mut shown = None;
    harness.frame(|ui| {
        shown = Some(tool_bar_strip(
            ui,
            &strip,
            room,
            &mut |_ui: &mut Ui, _rect: Rect, _name: &str| false,
        ));
    });
    shown.expect("the toolbar is drawn")
}

mod tool_family_buttons {
    use super::*;

    #[test]
    fn each_family_is_a_flyout_button_whose_click_runs_its_first_tool() {
        for ratio in RATIOS {
            let built = draw_bar(20);
            let mut harness = Harness::new().with_pixels_per_point(ratio as f32);
            let shown = show_strip(&mut harness, &built, 2400.0);
            let snapshot = harness.snapshot();
            for name in FAMILIES {
                let button = format!("toolFamilyButton.Draw.{name}");
                let family = built
                    .actions
                    .family(&button)
                    .unwrap_or_else(|| panic!("{name}: no family"));
                assert_eq!(family.menu.name, format!("toolFamily.Draw.{name}"));
                assert!(!family.menu.items.is_empty(), "{name}");
                let flyout = built.family(name);
                let first = flyout.menu.items.iter().find_map(|item| match item {
                    katana_window::drive::MenuItem::Action(action) => Some(action.name.clone()),
                    _ => None,
                });
                assert_eq!(Some(flyout.default.name.clone()), first, "{name}");
                // Not a split button: its menu opens from its corner, a press held or a right
                // click, never from a strip over the icon.
                assert_eq!(
                    flyout.paint(20, ButtonState::default(), false).look,
                    ToolButtonLook::TOOL
                );
                let control = snapshot
                    .control("DrawToolBar", &button)
                    .unwrap_or_else(|| panic!("{name}: not drawn on the toolbar"));
                assert!(control.visible, "{name}");
                assert!(!shown.hidden.contains(&button), "{name}");
            }
            assert_eq!(built.family("Circle").default.name, "draw.circle");
        }
    }

    #[test]
    fn the_corner_mark_and_its_press_zone_stay_off_the_icon_at_every_toolbar_icon_size() {
        // View > Toolbars offers 16, 20 and 28 px icons. The widget's geometry by hand, as
        // expect_drawn_as_designed works it: 13 px more than the icon, the icon at 6, the 5 px
        // mark at the icon's size and 6.
        for ratio in RATIOS {
            for size in [16, 20, 28] {
                let built = draw_bar(size);
                for name in FAMILIES {
                    let at = format!("{name} at {size} px, ratio {ratio}");
                    let flyout = built.family(name);
                    let paint = flyout.paint(size, ButtonState::default(), false);
                    assert_eq!((paint.width, paint.height), (size + 13, size + 13), "{at}");
                    let icon = icon_rect(&paint.look, paint.width, paint.height, size);
                    let mark = mark_rect(paint.width, paint.height, icon);
                    assert_eq!(icon, IntRect::new(6, 6, size, size), "{at}");
                    assert_eq!(mark, IntRect::new(size + 6, size + 6, 5, 5), "{at}");
                    assert!(!rects_intersect(&mark, &icon), "{at}");
                    // Inside the 1 px border.
                    let inside = IntRect::new(1, 1, paint.width - 2, paint.height - 2);
                    assert!(contains_rect(&inside, &mark), "{at}");
                    // The press zone holds the whole mark and none of the icon.
                    let zone = MenuZone::new(paint.width, paint.height, icon);
                    assert!(zone.contains_rect(mark), "{at}");
                    assert!(!zone.intersects(icon), "{at}");

                    // And the geometry is what is drawn: the icon in its square, and what the
                    // menu adds is the mark - a triangle whose right column and bottom row are
                    // whole, so its ink spans exactly its square.
                    let before = paint.clone();
                    expect_drawn_as_designed(&paint, size, ratio, &at);
                    // Measured leaves the button as it was.
                    assert_eq!(paint, before, "{at}");
                }
            }
        }
    }

    #[test]
    fn a_family_is_framed_only_while_the_tool_its_icon_shows_runs() {
        // Ink against a baseline: the Circle family beside a plain button with the same icon.
        // They may differ only inside the mark - at rest, under the pointer, running the
        // family's first tool, the one its icon shows (the plain button running too), and
        // running another of its tools, 2 Points, with the plain button AT REST. A frame is the
        // toolbar's one sign that a button's tool runs, and a frame round the Centre Radius icon
        // says Centre Radius runs. While any of its tools runs the mark is the accent, the
        // running frame's colour; at rest it is muted, and under the pointer, off its corner,
        // the text colour.
        for ratio in RATIOS {
            let mut built = draw_bar(20);
            let circle = built.family("Circle");
            let first = circle.default.name.clone();
            let other = circle
                .menu
                .items
                .iter()
                .filter_map(|item| match item {
                    katana_window::drive::MenuItem::Action(action) => Some(action.name.clone()),
                    _ => None,
                })
                .nth(1)
                .expect("the family has a second tool");
            assert_ne!(other, first);
            let mark = drawn_parts(
                &circle.paint(20, ButtonState::default(), false),
                ratio,
                &raster(),
            )
            .sign_device
            .expect("no mark drawn");
            if ratio == 1.0 {
                let paint = circle.paint(20, ButtonState::default(), false);
                assert_eq!(mark, mark_rect(33, 33, icon_rect(&paint.look, 33, 33, 20)));
            }
            let states = [
                ("at rest", false, false, false, palette::text_muted()),
                ("under the pointer", true, false, false, palette::text()),
                (
                    "running the tool its icon shows",
                    false,
                    true,
                    false,
                    palette::accent(),
                ),
                (
                    "running another of its tools",
                    false,
                    false,
                    true,
                    palette::accent(),
                ),
            ];
            for (name, hover, own, another, ink) in states {
                let running = if own {
                    first.as_str()
                } else if another {
                    other.as_str()
                } else {
                    ""
                };
                built.actions.set_active(running);
                let circle = built.family("Circle");
                assert_eq!(circle.family_running(), own || another, "{name}");
                assert_eq!(circle.default.checked == Some(true), own, "{name}");
                let state = ButtonState {
                    hovered: hover,
                    checked: own,
                    ..ButtonState::default()
                };
                let family = circle.paint(20, state, false).render(&raster(), ratio);
                let twin = twin_paint(20, state).render(&raster(), ratio);
                let differs = difference_box(&family, &twin)
                    .unwrap_or_else(|| panic!("{name}: the mark is not drawn"));
                assert!(
                    contains_rect(&mark, &differs),
                    "{name}: the family differs from the plain button at {differs:?}"
                );
                // The right-angled corner, inked by every row of the triangle.
                assert_eq!(
                    family.pixel(mark.right() as u32, mark.bottom() as u32),
                    ink,
                    "{name}"
                );
            }
        }
    }

    #[test]
    fn the_button_is_painted_again_when_any_of_its_tools_starts_or_stops() {
        // The screen shows what a frame paints, and a tool of the family started from the
        // command line paints no frame of its own: the frame after it must paint the mark lit,
        // and the frame after it stops, unlit.
        for ratio in RATIOS {
            let mut built = draw_bar(20);
            let mut harness = Harness::new().with_pixels_per_point(ratio as f32);
            let ink = |response: &FlyoutResponse| match response.paint.sign {
                MenuSign::Mark(ink) => ink,
                _ => Color::TRANSPARENT,
            };
            let at_rest = show(&mut harness, &built.family("Circle"));
            assert_eq!(ink(&at_rest), palette::text_muted());
            let other = built
                .family("Circle")
                .menu
                .items
                .iter()
                .filter_map(|item| match item {
                    katana_window::drive::MenuItem::Action(action) => Some(action.name.clone()),
                    _ => None,
                })
                .nth(1)
                .expect("the family has a second tool");
            built.actions.set_active(&other);
            let started = show(&mut harness, &built.family("Circle"));
            assert_eq!(
                ink(&started),
                palette::accent(),
                "not painted again when its second tool started"
            );
            built.actions.set_active("");
            let stopped = show(&mut harness, &built.family("Circle"));
            assert_eq!(
                ink(&stopped),
                palette::text_muted(),
                "not painted again when its second tool stopped"
            );
        }
    }

    #[test]
    fn a_family_takes_its_toolbars_new_icon_size_as_the_toolbars_own_buttons_do() {
        // View > Toolbars changes a bar's icon size after its buttons are made. The bar's own
        // buttons follow it; a family on the bar must too, and then draws as one made at that
        // size would. Made at 20 px and switched, a C++ family kept its 33 x 33 - at 28 its icon
        // was drawn at 23 px beside the others' 28, its mark 3 px against the icon's corner.
        for ratio in RATIOS {
            for size in [16, 28] {
                let mut built = draw_bar(20);
                built.icon_size = size;
                let mut harness = Harness::new().with_pixels_per_point(ratio as f32);
                show_strip(&mut harness, &built, 2400.0);
                let snapshot = harness.snapshot();
                let twin = snapshot
                    .control("DrawToolBar", PLAIN)
                    .expect("the bar's own button is drawn");
                assert_eq!(
                    twin.rect.size(),
                    Vec2::new((size + 13) as f32, (size + 13) as f32),
                    "the bar's own button at {size}"
                );
                for name in FAMILIES {
                    let at = format!("{name} switched to {size} px, ratio {ratio}");
                    let button = snapshot
                        .control("DrawToolBar", &format!("toolFamilyButton.Draw.{name}"))
                        .unwrap_or_else(|| panic!("{at}: not drawn"));
                    assert_eq!(
                        button.rect.size(),
                        Vec2::new((size + 13) as f32, (size + 13) as f32),
                        "{at}"
                    );
                    let paint = built
                        .family(name)
                        .paint(size, ButtonState::default(), false);
                    let icon = icon_rect(&paint.look, paint.width, paint.height, size);
                    assert_eq!(icon, IntRect::new(6, 6, size, size), "{at}");
                    assert_eq!(
                        mark_rect(paint.width, paint.height, icon),
                        IntRect::new(size + 6, size + 6, 5, 5),
                        "{at}"
                    );
                    expect_drawn_as_designed(&paint, size, ratio, &at);
                }
            }
        }
    }

    #[test]
    fn the_mark_is_the_same_whole_staircase_of_device_pixels_wherever_the_button_sits() {
        // At 125% and 150% a widget pixel is not a whole number of device pixels. The triangle
        // is painted a device row at a time: the k-th row from its top is k pixels ending at its
        // right column. Its legs are 5 px times the ratio taken down, 5, 6, 7, 8 and 10 device
        // pixels at 100, 125, 150, 175 and 200%, wherever the button sits. Its corner is 2 px
        // times the ratio, taken down, in from the button's last device pixel, and at least a
        // whole device pixel short of the first the frame's 1 px border touches: widget pixel 32
        // of the 33 px button, plus its offset, times the ratio, taken down. Rendered at each
        // ratio with the button 0 to 3 widget pixels in, and at the screen's own ratio.
        for screen in RATIOS {
            let built = draw_bar(20);
            let circle = built.family("Circle");
            let paint = circle.paint(20, ButtonState::default(), false);
            let mut without = paint.clone();
            without.sign = MenuSign::None;
            let mut cases = Vec::new();
            for ratio in [1.0, 1.25, 1.5, 1.75, 2.0] {
                for offset in 0..4 {
                    cases.push((ratio, offset, false));
                }
            }
            cases.push((screen, 0, true));
            for (ratio, offset, grabbed) in cases {
                let with = render_at(&paint, ratio, offset);
                let bare = render_at(&without, ratio, offset);
                // Each row of the mark: how many pixels, the first and last, where.
                let mut rows: Vec<[i32; 4]> = Vec::new();
                for y in 0..with.height() {
                    let (mut count, mut left, mut right) = (0, -1, -1);
                    for x in 0..with.width() {
                        if with.pixel(x, y) != bare.pixel(x, y) {
                            count += 1;
                            if left < 0 {
                                left = x as i32;
                            }
                            right = x as i32;
                        }
                    }
                    if count > 0 {
                        rows.push([count, left, right, y as i32]);
                    }
                }
                let at = format!(
                    "{} {ratio}, {offset} px in",
                    if grabbed {
                        "grabbed at the screen's ratio of"
                    } else {
                        "rendered at a ratio of"
                    }
                );
                let leg = rows.len() as i32;
                assert_eq!(leg, whole_device_pixels(5, ratio), "{at}");
                for (k, row) in (0_i32..).zip(&rows) {
                    let [count, left, right, y] = *row;
                    assert_eq!(count, k + 1, "{at}, row {k}");
                    assert_eq!(right - left + 1, count, "{at}, row {k} has a gap");
                    assert_eq!(right, rows[0][2], "{at}, row {k} off the right column");
                    assert_eq!(y, rows[0][3] + k, "{at}, row {k} not below the last");
                }
                let last = q_round(f64::from(33 + offset) * ratio) - 1;
                let frame = whole_device_pixels(32 + offset, ratio);
                let corner = (last - whole_device_pixels(2, ratio)).min(frame - 2);
                let bottom = rows.last().expect("the mark has rows");
                assert_eq!(bottom[2], corner, "{at}: the corner's column");
                assert_eq!(bottom[3], corner, "{at}: the corner's row");
            }
        }
    }

    #[test]
    fn a_running_familys_mark_keeps_a_whole_device_pixel_of_air_from_its_frame() {
        // While the tool its icon shows runs, a family is framed in the accent, and its mark is
        // the accent too. The frame is the theme's 1 px border: at 125%, the owner's display,
        // 1.25 device pixels, which fall across two device columns wherever the button starts
        // off the device grid. The mark ends a whole device pixel before the first device column
        // and row the border touches. By hand at 125% with the button 2 px in, 2.5 device
        // pixels: the border, widget pixel 32, is device 42.5..43.75, touching columns 42 and 43;
        // the mark's 6 px end at 40, leaving 41 clear, and start at 35, the pixel after the
        // icon's. Rendered at each ratio with the button 0 to 3 widget pixels in, at each
        // toolbar icon size, the mark kept its size and stayed off the icon.
        let name = |colour: Color| format!("#{:02x}{}", colour.a, &colour.name()[1..]);
        for _screen in RATIOS {
            for size in [16, 20, 28] {
                let mut built = draw_bar(size);
                let first = built.family("Circle").default.name.clone();
                built.actions.set_active(&first);
                let circle = built.family("Circle");
                assert_eq!(circle.default.checked, Some(true));
                let state = ButtonState {
                    checked: true,
                    ..ButtonState::default()
                };
                let paint = circle.paint(size, state, false);
                let mut without = paint.clone();
                without.sign = MenuSign::None;
                let mut solid = paint.clone();
                solid.icon = ButtonIcon::Solid;
                let mut clear = paint.clone();
                clear.icon = ButtonIcon::Clear;
                for ratio in [1.0, 1.25, 1.5, 1.75, 2.0] {
                    for offset in 0..4 {
                        let at = format!("{size} px icons at a ratio of {ratio}, {offset} px in");
                        let with = render_at(&paint, ratio, offset);
                        let mark = difference_box(&with, &render_at(&without, ratio, offset))
                            .unwrap_or_else(|| panic!("{at}: no mark drawn"));
                        let icon = difference_box(
                            &render_at(&solid, ratio, offset),
                            &render_at(&clear, ratio, offset),
                        )
                        .unwrap_or_else(|| panic!("{at}: no icon drawn"));
                        assert_eq!(mark.width, whole_device_pixels(5, ratio), "{at}");
                        assert_eq!(mark.height, whole_device_pixels(5, ratio), "{at}");
                        assert!(mark.x > icon.right(), "{at}: the mark is on the icon");
                        assert!(mark.y > icon.bottom(), "{at}: the mark is on the icon");
                        // The running button's ground, 3 px in from its left edge halfway down:
                        // inside the border, left of the icon.
                        let (gx, gy) = device_pixel(offset + 3, offset + (size + 13) / 2, ratio);
                        let ground = with.pixel(gx, gy);
                        assert_eq!(name(ground), name(palette::raised()), "{at}");
                        let beside = with.pixel((mark.right() + 1) as u32, mark.y as u32);
                        let below = with.pixel(mark.x as u32, (mark.bottom() + 1) as u32);
                        assert_eq!(
                            name(beside),
                            name(ground),
                            "{at}: device column {}, after the mark's {}..{}",
                            mark.right() + 1,
                            mark.x,
                            mark.right()
                        );
                        assert_eq!(
                            name(below),
                            name(ground),
                            "{at}: device row {}, below the mark's {}..{}",
                            mark.bottom() + 1,
                            mark.y,
                            mark.bottom()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_click_on_the_icon_runs_the_tool_while_the_mark_a_hold_or_a_right_click_opens_the_family() {
        for ratio in RATIOS {
            let built = draw_bar(20);
            let circle = built.family("Circle");
            let mut harness = Harness::new().with_pixels_per_point(ratio as f32);
            let origin = show(&mut harness, &circle).response.rect.min;
            let paint = circle.paint(20, ButtonState::default(), false);
            let icon = icon_rect(&paint.look, 33, 33, 20);
            let mark = mark_rect(33, 33, icon);
            // QRect::center: the middle pixel, halves taken down.
            let on_icon = point(
                origin,
                (icon.x + icon.right()) / 2,
                (icon.y + icon.bottom()) / 2,
            );
            // The icon's own bottom-right pixel: on the old split button this was under the
            // arrow's strip, and opened the menu.
            let icon_corner = point(origin, icon.right(), icon.bottom());
            let on_mark = point(
                origin,
                (mark.x + mark.right()) / 2,
                (mark.y + mark.bottom()) / 2,
            );
            let mut started: Vec<String> = Vec::new();
            let mut shown = 0;
            let mut tally = |responses: &[FlyoutResponse], started: &mut Vec<String>| {
                for response in responses {
                    started.extend(response.triggered.clone());
                    shown += usize::from(response.opened);
                }
                shown
            };

            let frames = click(&mut harness, &circle, on_icon, PointerButton::Primary);
            assert_eq!(tally(&frames, &mut started), 0);
            assert_eq!(started, ["draw.circle"]);

            let frames = click(&mut harness, &circle, icon_corner, PointerButton::Primary);
            assert_eq!(tally(&frames, &mut started), 0);
            assert_eq!(
                started.len(),
                2,
                "a click on the icon's corner runs the tool"
            );

            // The mark opens the family at once, and runs nothing.
            let frames = click(&mut harness, &circle, on_mark, PointerButton::Primary);
            assert!(frames[0].opened, "the menu opens on the press itself");
            assert_eq!(tally(&frames, &mut started), 1);
            assert_eq!(started.len(), 2);
            close_menu(&mut harness, &circle);

            // A right click opens it too, rather than the window's list of toolbars.
            let frames = click(&mut harness, &circle, on_icon, PointerButton::Secondary);
            assert_eq!(tally(&frames, &mut started), 2);
            assert_eq!(started.len(), 2);
            close_menu(&mut harness, &circle);

            // What --press does (QAbstractButton::click): the tool, no menu.
            harness.press("toolFamilyButton.Draw.Circle");
            let mut pressed = Vec::new();
            harness
                .run(|ui| pressed.push(flyout_button(ui, &circle, 20, true)))
                .expect("the press is taken");
            assert_eq!(tally(&pressed, &mut started), 2);
            assert_eq!(started.len(), 3);

            // A press held on the icon past the popup delay opens the family, and letting go then
            // runs nothing.
            harness.pointer_move(on_icon).pointer_button(
                on_icon,
                PointerButton::Primary,
                true,
                Modifiers::NONE,
            );
            let mut frames = 0;
            let mut opened = false;
            while frames < 100 && !opened {
                let response = show(&mut harness, &circle);
                opened = response.opened;
                started.extend(response.triggered);
                frames += 1;
            }
            assert!(opened, "no menu after holding for {frames} frames");
            // A frame is a sixtieth of a second on the harness's clock; the first was the press.
            let waited = f64::from(frames - 1) / 60.0;
            assert!(
                waited >= 0.6 - 0.05,
                "the menu came before the delay: {waited} s"
            );
            harness.pointer_button(on_icon, PointerButton::Primary, false, Modifiers::NONE);
            let released = show(&mut harness, &circle);
            started.extend(released.triggered);
            assert_eq!(
                started.len(),
                3,
                "a held press that opened the menu also ran the tool; started: {}",
                started.join(", ")
            );
        }
    }

    #[test]
    fn with_no_tool_running_the_mark_is_lit_in_the_accent_only_while_the_pointer_is_on_its_corner()
    {
        // The corner opens the family where the rest of the button runs the tool, so the mark
        // says which the pointer is over. The pointer is moved as a person moves it.
        for ratio in RATIOS {
            let built = draw_bar(20);
            let circle = built.family("Circle");
            let mut harness = Harness::new().with_pixels_per_point(ratio as f32);
            let origin = show(&mut harness, &circle).response.rect.min;
            let paint = circle.paint(20, ButtonState::default(), false);
            let icon = icon_rect(&paint.look, 33, 33, 20);
            let mark = mark_rect(33, 33, icon);
            let zone = MenuZone::new(33, 33, icon);
            let mut move_to = |at: Pos2| {
                harness.pointer_move(at);
                let response = show(&mut harness, &circle);
                let ink = match response.paint.sign {
                    MenuSign::Mark(ink) => ink,
                    _ => Color::TRANSPARENT,
                };
                (ink, response.hovered, response.pointer_on_mark)
            };
            let on_icon = point(
                origin,
                (icon.x + icon.right()) / 2,
                (icon.y + icon.bottom()) / 2,
            );
            let on_mark = point(
                origin,
                (mark.x + mark.right()) / 2,
                (mark.y + mark.bottom()) / 2,
            );
            move_to(AWAY);
            let (ink, hovered, on) = move_to(on_icon);
            assert_eq!(ink, palette::text());
            assert!(hovered);
            assert!(!on);
            let (ink, _, on) = move_to(on_mark);
            assert_eq!(ink, palette::accent());
            assert!(on);
            // Off the mark but still in its corner, beside the icon: the press there opens the
            // family too, so the mark stays lit.
            let (bx, by) = (icon.right() + 2, mark.y - 3);
            assert!(zone.contains(bx, by));
            let (ink, _, _) = move_to(point(origin, bx, by));
            assert_eq!(ink, palette::accent());
            let (ink, _, on) = move_to(on_icon);
            assert_eq!(ink, palette::text());
            assert!(!on);
            let (ink, _, _) = move_to(on_mark);
            assert_eq!(ink, palette::accent());
            let (ink, hovered, on) = move_to(AWAY);
            assert_eq!(ink, palette::text_muted());
            assert!(!hovered);
            assert!(!on);
        }
    }

    #[test]
    fn the_mark_goes_out_when_the_pointer_leaves_for_the_familys_menu() {
        // The corner's press opens the family, and to pick from it the pointer leaves the button
        // while the menu is open. Once the menu closes the mark must be out - muted - with
        // nothing running: in the C++ a mark lit by the hover events alone stayed lit in the
        // accent, the colour of a tool running, until the pointer next crossed the button.
        for ratio in RATIOS {
            let built = draw_bar(20);
            let circle = built.family("Circle");
            let mut harness = Harness::new().with_pixels_per_point(ratio as f32);
            let origin = show(&mut harness, &circle).response.rect.min;
            let paint = circle.paint(20, ButtonState::default(), false);
            let icon = icon_rect(&paint.look, 33, 33, 20);
            let mark = mark_rect(33, 33, icon);
            let on_icon = point(
                origin,
                (icon.x + icon.right()) / 2,
                (icon.y + icon.bottom()) / 2,
            );
            let on_mark = point(
                origin,
                (mark.x + mark.right()) / 2,
                (mark.y + mark.bottom()) / 2,
            );
            for at in [AWAY, on_icon, on_mark] {
                harness.pointer_move(at);
                show(&mut harness, &circle);
            }
            harness.pointer_move(on_mark);
            let lit = show(&mut harness, &circle);
            assert!(lit.pointer_on_mark, "the pointer did not reach the corner");
            assert_eq!(lit.paint.sign, MenuSign::Mark(palette::accent()));

            // Pressed on the corner, the family opens; the pointer goes over the window and the
            // menu is closed, as Escape or a click elsewhere closes it.
            harness.pointer_button(on_mark, PointerButton::Primary, true, Modifiers::NONE);
            let opened = show(&mut harness, &circle);
            assert!(opened.opened, "the corner's press did not open the family");
            harness.pointer_move(AWAY);
            let left = show(&mut harness, &circle);
            assert!(
                !left.hovered,
                "the button still thought the pointer was on it"
            );
            close_menu(&mut harness, &circle);
            harness.pointer_button(AWAY, PointerButton::Primary, false, Modifiers::NONE);
            let after = show(&mut harness, &circle);
            assert!(!after.hovered);
            assert!(!after.pointer_on_mark);
            assert!(!circle.family_running());
            assert_eq!(after.paint.sign, MenuSign::Mark(palette::text_muted()));
        }
    }

    #[test]
    fn on_its_corner_the_status_bar_names_the_family_and_elsewhere_the_tool() {
        // The status bar is the window's other line about what is under the pointer: the tool's
        // tip on the button, and on the corner, where a press opens the family, the family's own
        // line, the Draw menu's for its submenu.
        for ratio in RATIOS {
            let built = draw_bar(20);
            let vertices = built.family("Vertices");
            let family_tip = vertices.family_tip.clone();
            assert!(
                family_tip.starts_with("Vertices: Insert Vertex / Delete Vertex / "),
                "{family_tip}"
            );
            let tool_tip = vertices.default.tip.clone();
            assert!(!tool_tip.is_empty());
            assert_ne!(tool_tip, family_tip);
            let mut harness = Harness::new().with_pixels_per_point(ratio as f32);
            let origin = show(&mut harness, &vertices).response.rect.min;
            let paint = vertices.paint(20, ButtonState::default(), false);
            let icon = icon_rect(&paint.look, 33, 33, 20);
            let mark = mark_rect(33, 33, icon);
            let on_icon = point(
                origin,
                (icon.x + icon.right()) / 2,
                (icon.y + icon.bottom()) / 2,
            );
            let on_mark = point(
                origin,
                (mark.x + mark.right()) / 2,
                (mark.y + mark.bottom()) / 2,
            );
            let mut status = String::new();
            let mut move_to = |at: Pos2, status: &mut String| {
                harness.pointer_move(at);
                if let Some(line) = show(&mut harness, &vertices).status_tip {
                    *status = line;
                }
            };
            move_to(AWAY, &mut status);
            move_to(on_icon, &mut status);
            assert_eq!(status, tool_tip);
            move_to(on_mark, &mut status);
            assert_eq!(status, family_tip);
            move_to(on_icon, &mut status);
            assert_eq!(status, tool_tip);
            // Off the button the line is cleared, as QWidget clears it on Leave.
            move_to(on_mark, &mut status);
            move_to(AWAY, &mut status);
            assert!(status.is_empty(), "{status}");
        }
    }

    /// A family of letters' menu, each item named as it reads.
    fn letters(names: &[&str]) -> Menu {
        let mut menu = Menu::new("letters", "Letters");
        for name in names {
            menu = menu.action(Action::new(name, name));
        }
        menu
    }

    /// The family's line of the tooltip, what follows the tool's own.
    fn family_line(tip: &str) -> &str {
        tip.find("<br><span").map_or("", |from| &tip[from..])
    }

    /// A name as the tooltip lists it: its spaces no-break, escaped for rich text.
    fn unbroken(name: &str) -> String {
        name.replace(' ', "\u{a0}")
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }

    #[test]
    fn the_tooltip_shown_says_how_to_reach_the_other_tools_and_names_five_at_most() {
        // By hand, on families of letters and one two-word name: with five others each is named,
        // in the menu's order, slashes between; with six, four are and "and 2 more" stands for
        // the rest. A name's own spaces are non-breaking (U+00A0). The tool the click runs is the
        // tooltip's own, never an other.
        for _ratio in RATIOS {
            let own = "<b>A</b> (LETTER)";
            let flyout_of = |names: &[&str]| Flyout {
                name: String::from("lettersButton"),
                menu: letters(names),
                default: Action::new("A", "A"),
                tool_tip: String::from(own),
                family_tip: String::new(),
            };
            let six = flyout_of(&["A", "B", "C", "D", "E", "Two Words"]);
            let seven = flyout_of(&["A", "B", "C", "D", "E", "F", "G"]);
            let muted = palette::text_muted().name();
            assert_eq!(
                six.tooltip_with_menu(),
                format!(
                    "<b>A</b> (LETTER)<br><span style='color:{muted}'>Hold, right-click or press \
                     the corner triangle for its other tools: B / C / D / E / Two\u{a0}Words</span>"
                )
            );
            assert_eq!(
                seven.tooltip_with_menu(),
                format!(
                    "<b>A</b> (LETTER)<br><span style='color:{muted}'>Hold, right-click or press \
                     the corner triangle for its other tools: B / C / D / E and 2 more</span>"
                )
            );
            // Shown, not only built: the button's own tooltip is the tool's, and the family's
            // line is added when the tooltip is asked for.
            assert_eq!(seven.tool_tip, own);
        }
    }

    #[test]
    fn each_familys_tooltip_shown_names_its_other_tools_or_four_and_a_count() {
        // The catalogue's families, as the window makes them: the rule above on each - Circle's
        // five others and Arc's four all named, Ellipse's two, and of the Vertices family's
        // seventeen, four and "and 13 more": the seventeen all named were a wall of six lines
        // under the tool's own two.
        for _ratio in RATIOS {
            let built = draw_bar(20);
            for name in FAMILIES {
                let flyout = built.family(name);
                let tip = flyout.tooltip_with_menu();
                assert!(!tip.is_empty(), "{name}: no tooltip shown");
                assert!(tip.starts_with(&flyout.tool_tip), "{tip}");
                let others: Vec<String> = flyout
                    .menu
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        katana_window::drive::MenuItem::Action(action)
                            if action.name != flyout.default.name =>
                        {
                            Some(unbroken(&action.text.replace('&', "")))
                        }
                        _ => None,
                    })
                    .collect();
                assert!(!others.is_empty(), "{name}");
                let named = if others.len() <= 5 {
                    others.join(" / ")
                } else {
                    format!("{} and {} more", others[..4].join(" / "), others.len() - 4)
                };
                assert_eq!(
                    family_line(&tip),
                    format!(
                        "<br><span style='color:{}'>Hold, right-click or press the corner \
                         triangle for its other tools: {named}</span>",
                        palette::text_muted().name()
                    ),
                    "{name}"
                );
            }
            assert_eq!(built.family("Circle").menu.items.len(), 6);
            let vertices = built.family("Vertices").tooltip_with_menu();
            assert!(
                family_line(&vertices).ends_with(" and 13 more</span>"),
                "{vertices}"
            );
        }
    }

    #[test]
    fn the_tooltip_wraps_between_the_other_tools_names_never_inside_one() {
        // A tooltip long enough is laid out wrapped, and with plain spaces a name was broken over
        // two lines: Circle's read "... 2 Points / 3" then "Points and 2 more". Laid out here at
        // every width from 60 to 400 px as the tooltip lays it out - broken at spaces, a word
        // wider than the line running past it - each name the tooltip gives is whole on one
        // line, measured in the window's own font.
        for ratio in RATIOS {
            let built = draw_bar(20);
            let mut harness = Harness::new().with_pixels_per_point(ratio as f32);
            harness.frame(|_ui| {});
            let ctx = harness.context().clone();
            let font = egui::TextStyle::Body.resolve(&ctx.global_style());
            let measure = |text: &str| {
                ctx.fonts_mut(|fonts| {
                    fonts
                        .layout_no_wrap(String::from(text), font.clone(), egui::Color32::WHITE)
                        .size()
                        .x
                })
            };
            for name in FAMILIES {
                let flyout = built.family(name);
                let names: Vec<String> = flyout
                    .menu
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        katana_window::drive::MenuItem::Action(action)
                            if action.name != flyout.default.name =>
                        {
                            Some(action.text.replace('&', ""))
                        }
                        _ => None,
                    })
                    .collect();
                let tip = flyout.tooltip_with_menu();
                let plain: String = tooltip_runs(family_line(&tip))
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect();
                // How many of the widths wrapped the line of names: the check is only a check
                // where there are lines to break it over.
                let mut wrapped = 0;
                for width in (60..=400).step_by(2) {
                    for paragraph in plain.split('\n').filter(|paragraph| !paragraph.is_empty()) {
                        let lines = wrap_words(paragraph, width as f32, &measure);
                        assert!(!lines.is_empty(), "{name} not laid out at {width} px");
                        wrapped += usize::from(lines.len() > 1);
                        // The paragraph with its no-break spaces as spaces, to find the names by,
                        // and the line each of its characters is on.
                        let text: String = paragraph
                            .chars()
                            .map(|character| {
                                if character == '\u{a0}' {
                                    ' '
                                } else {
                                    character
                                }
                            })
                            .collect();
                        let mut line_of = Vec::new();
                        for (number, line) in lines.iter().enumerate() {
                            for _ in 0..=line.chars().count() {
                                line_of.push(number);
                            }
                        }
                        for tool in &names {
                            let Some(at) = text.find(tool.as_str()) else {
                                continue; // one of "and N more"
                            };
                            let first = text[..at].chars().count();
                            let last = first + tool.chars().count() - 1;
                            assert_eq!(
                                line_of[first], line_of[last],
                                "{name}: \"{tool}\" broken over two lines at {width} px"
                            );
                        }
                    }
                }
                assert!(wrapped > 0, "{name}: never wrapped between 60 and 400 px");
            }
        }
    }

    #[test]
    fn while_another_of_its_tools_runs_the_tooltip_names_it() {
        // Another of the family's tools running lights the mark and frames nothing (the icon is
        // not its), so the tooltip says which runs, in the accent the mark is lit in. The tool
        // its icon shows running is said by its frame; nothing running, nothing is said.
        for _ratio in RATIOS {
            let mut built = draw_bar(20);
            let running = format!(
                "<br><span style='color:{}'>Running now: ",
                palette::accent().name()
            );
            assert!(
                !built
                    .family("Vertices")
                    .tooltip_with_menu()
                    .contains(&running)
            );
            built.actions.set_active("draw.vertex.delete");
            let with_delete = built.family("Vertices").tooltip_with_menu();
            assert!(
                with_delete.ends_with(&format!("{running}Delete\u{a0}Vertex</span>")),
                "{with_delete}"
            );
            let first = built.family("Vertices").default.name.clone();
            built.actions.set_active(&first);
            assert!(
                !built
                    .family("Vertices")
                    .tooltip_with_menu()
                    .contains(&running)
            );
            built.actions.set_active("");
        }
    }
}

/// Undo as the window makes it - a split button with its history, on a horizontal Edit toolbar.
fn undo() -> (Action, Menu) {
    let action = Action::new("editUndo", "Undo").icon("Undo");
    let history = Menu::new("editUndoMenu", "").action(Action::new("history1", "1 CREATE_POINT"));
    (action, history)
}

/// One frame of Undo's split button at 20 px icons.
fn show_undo(harness: &mut Harness) -> ToolButtonResponse {
    let (action, history) = undo();
    let (width, height) = ToolButtonLook::SPLIT.button_size(20);
    let mut shown = None;
    harness.frame(|ui| {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(width as f32, height as f32), Sense::hover());
        shown = Some(split_button_at(
            ui,
            rect,
            "editUndoButton",
            &action,
            &history,
            20,
            false,
        ));
    });
    shown.expect("the button is drawn")
}

mod split_buttons {
    use super::*;

    #[test]
    fn undos_arrow_has_a_strip_of_its_own_beside_the_icon() {
        // The strip the menu opens from, and the arrow drawn in it, are clear of the icon's
        // square: the theme pads the split button for the strip. By hand: 20 px of icon, Qt's 3,
        // 4 px of padding on the left and 13 on the right, a 1 px border each side - 42 px, the
        // contents at 5..27 with the icon's square centred at 6..25. The strip is laid in the
        // BORDER rectangle, its right 12 px: 30..41, two pixels of padding past the contents.
        // Fusion draws its arrow 8 px wide, (12 - 8) / 2 = 2 px into the strip: 32..39, two
        // pixels in from the button's edge.
        for ratio in RATIOS {
            let paint = split_paint(ToolButtonLook::SPLIT, "Undo", 20, ButtonState::default());
            assert_eq!((paint.width, paint.height), (42, 33));
            let icon = IntRect::new(6, 6, 20, 20); // by hand, above
            let parts = drawn_parts(&paint, ratio, &raster());
            let icon_device = parts.icon_device.expect("no icon drawn");
            let sign_device = parts.sign_device.expect("no arrow drawn");
            assert!(!rects_intersect(&sign_device, &icon_device));

            let strip = split_strip(paint.width, paint.height);
            assert_eq!(strip.x, 30);
            assert_eq!(strip.right(), 41);
            assert!(!rects_intersect(&strip, &icon));
            if parts.ratio == 1.0 {
                assert_eq!(parts.icon, Some(icon));
                let sign = parts.sign.expect("the arrow in widget pixels");
                assert_eq!(sign.x, 32);
                assert_eq!(sign.right(), 39);
            }
            // At any ratio the arrow is inside the strip and short of the button's last device
            // pixel by at least the 2 px times the ratio, taken down.
            let right = q_round(f64::from(strip.right() + 1) * ratio) - 1;
            let bottom = q_round(f64::from(paint.height) * ratio) - 1;
            let left = whole_device_pixels(strip.x, ratio);
            let strip_on_device = IntRect::new(left, 0, right - left + 1, bottom + 1);
            assert!(contains_rect(&strip_on_device, &sign_device));
            assert!(
                sign_device.right()
                    <= q_round(f64::from(paint.width) * ratio) - 1 - whole_device_pixels(2, ratio)
            );

            // A click on the icon's right edge undoes; the strip opens the history.
            let mut harness = Harness::new().with_pixels_per_point(ratio as f32);
            let origin = show_undo(&mut harness).response.rect.min;
            let mut press = |at: Pos2| {
                harness.pointer_move(at).pointer_button(
                    at,
                    PointerButton::Primary,
                    true,
                    Modifiers::NONE,
                );
                let pressed = show_undo(&mut harness);
                harness.pointer_button(at, PointerButton::Primary, false, Modifiers::NONE);
                let released = show_undo(&mut harness);
                (
                    pressed.triggered || released.triggered,
                    pressed.opened || released.opened,
                )
            };
            let (undone, shown) = press(point(origin, icon.right(), (icon.y + icon.bottom()) / 2));
            assert!(undone);
            assert!(!shown);
            let (undone, shown) = press(point(
                origin,
                (strip.x + strip.right()) / 2,
                (strip.y + strip.bottom()) / 2,
            ));
            assert!(shown);
            assert!(!undone);
        }
    }

    #[test]
    fn undos_strip_takes_the_buttons_hover_with_no_shade_of_its_own() {
        // The strip has the whole button's hover, never a shade of its own: one lay on it with
        // the pointer on the icon too in the C++ - a band of the pressed colour, darker than the
        // rest of the button, that read as pressed. Under the pointer the strip is the button's
        // hover colour. Compared beside the arrow (the strip's upper rows, below its rounded
        // corner) and in the padding left of the icon.
        for ratio in RATIOS {
            let paint = split_paint(ToolButtonLook::SPLIT, "Undo", 20, ButtonState::default());
            let strip = split_strip(paint.width, paint.height);
            let parts = drawn_parts(&paint, ratio, &raster());
            let in_strip = ((strip.x + strip.right()) / 2 - 1, strip.y + 5);
            let in_body = (3, paint.height / 2);
            let strip_pixel = device_pixel(in_strip.0, in_strip.1, ratio);
            let body_pixel = device_pixel(in_body.0, in_body.1, ratio);
            assert!(
                (strip_pixel.1 as i32) < parts.sign_device.expect("an arrow").y,
                "no room above the arrow"
            );
            assert!(
                !parts
                    .icon_device
                    .expect("an icon")
                    .contains(body_pixel.0 as i32, body_pixel.1 as i32)
            );
            let mut harness = Harness::new().with_pixels_per_point(ratio as f32);
            let origin = show_undo(&mut harness).response.rect.min;
            harness.pointer_move(AWAY);
            show_undo(&mut harness);
            // The icon's middle, by hand as in the test above: its square at 6..25.
            harness.pointer_move(point(origin, 16, 16));
            let hovered = show_undo(&mut harness);
            assert!(hovered.hovered);
            let image = hovered.paint.render(&raster(), ratio);
            assert_eq!(image.pixel(body_pixel.0, body_pixel.1), palette::hover());
            assert_eq!(image.pixel(strip_pixel.0, strip_pixel.1), palette::hover());
        }
    }
}

/// A title bar's view kind switcher as theme.cpp sizes it (34 x 22, a 1 px border), its 14 px
/// icon and its menu's indicator, with `look`'s padding.
fn kind_switcher(look: ToolButtonLook) -> MeasuredButton {
    MeasuredButton {
        name: String::from("ViewKindButton"),
        has_menu: true,
        icon_only: true,
        paint: ButtonPaint {
            width: 34,
            height: 22,
            look,
            icon: ButtonIcon::Named(String::from("ViewPlan")),
            icon_size: 14,
            state: ButtonState::default(),
            sign: MenuSign::Indicator(palette::text()),
            ground: Color::TRANSPARENT,
        },
    }
}

mod tool_bar_signs {
    use super::*;

    #[test]
    fn the_check_sees_a_split_buttons_arrow_drawn_over_its_icon() {
        // --check-toolbars must be able to fail. The same Undo button with every tool button's
        // padding and border and nothing for the strip - as before the split buttons were
        // padded - is a clash; with the theme's padding it is not.
        for ratio in RATIOS {
            let holder = |look: ToolButtonLook| SignHolder {
                holder: String::from("toolbar button EditToolBar"),
                buttons: vec![MeasuredButton {
                    name: String::from("editUndoButton"),
                    has_menu: true,
                    icon_only: true,
                    paint: split_paint(look, "Undo", 20, ButtonState::default()),
                }],
            };
            let unpadded = menu_sign_clashes(&[holder(ToolButtonLook::TOOL)], ratio, &raster());
            assert_eq!(unpadded.buttons, 1);
            assert_eq!(
                unpadded.clashes.len(),
                1,
                "{}",
                unpadded.measured.join("\n")
            );
            assert!(
                unpadded.clashes[0]
                    .starts_with("toolbar button EditToolBar > editUndoButton: its menu's sign"),
                "{}",
                unpadded.clashes[0]
            );
            assert!(
                unpadded.clashes[0].contains("is drawn over its icon"),
                "{}",
                unpadded.clashes[0]
            );
            let themed = menu_sign_clashes(&[holder(ToolButtonLook::SPLIT)], ratio, &raster());
            assert_eq!(themed.buttons, 1);
            assert!(themed.clashes.is_empty(), "{}", themed.clashes.join("\n"));
            assert_eq!(themed.measured.len(), 1);
            assert!(
                themed.measured[0].contains("clear of its icon"),
                "{}",
                themed.measured[0]
            );
        }
    }

    #[test]
    fn the_check_sees_a_view_kind_switchers_arrow_drawn_over_its_icon() {
        // The check reaches the docks' title bars too. The kind switcher with only the 2 px of
        // padding every title bar button has on its right, as it had before theme.cpp padded a
        // title bar button with a menu, is a clash under its dock's name.
        for ratio in RATIOS {
            let holders = [SignHolder {
                holder: String::from("title bar button View1"),
                buttons: vec![kind_switcher(ToolButtonLook::TITLE_BAR)],
            }];
            let signs = menu_sign_clashes(&holders, ratio, &raster());
            assert!(signs.buttons >= 1);
            assert_eq!(signs.clashes.len(), 1, "{}", signs.measured.join("\n"));
            assert!(
                signs.clashes[0]
                    .starts_with("title bar button View1 > ViewKindButton: its menu's sign"),
                "{}",
                signs.clashes[0]
            );
            assert!(
                signs.clashes[0].contains("is drawn over its icon"),
                "{}",
                signs.clashes[0]
            );
            // Padded by its indicator's box, as theme.cpp pads it, it is clear.
            let padded = [SignHolder {
                holder: String::from("title bar button View1"),
                buttons: vec![kind_switcher(ToolButtonLook::TITLE_BAR_MENU)],
            }];
            assert!(
                menu_sign_clashes(&padded, ratio, &raster())
                    .clashes
                    .is_empty()
            );
        }
    }

    #[test]
    fn the_check_judges_a_familys_mark_against_its_icon_in_the_screens_own_pixels() {
        // A family's mark starts the pixel after its icon's square ends, so on a scaled screen
        // the two meet at a widget pixel: at 125% the icon ends at device column 32 and the mark
        // starts at 33, and each box taken back to the widget's pixels, rounded outwards, reached
        // pixel 26 - four clashes that were not on the screen. The check judges in device pixels.
        for ratio in RATIOS {
            let built = draw_bar(20);
            let signs = menu_sign_clashes(&[strip_sign_holder(&built.strip())], ratio, &raster());
            assert_eq!(signs.buttons, FAMILIES.len());
            assert!(signs.clashes.is_empty(), "{}", signs.clashes.join("\n"));
            for line in &signs.measured {
                assert!(
                    line.starts_with("toolbar button DrawToolBar > toolFamilyButton.Draw."),
                    "{line}"
                );
                assert_eq!(
                    line.contains("(device pixels at a ratio of"),
                    ratio != 1.0,
                    "{line}"
                );
            }
            // The boxes themselves, both ways: in device pixels apart, and on a scaled screen
            // meeting in the widget's.
            let parts = drawn_parts(
                &built
                    .family("Circle")
                    .paint(20, ButtonState::default(), false),
                ratio,
                &raster(),
            );
            assert_eq!(parts.ratio, ratio);
            let (icon, sign) = (
                parts.icon_device.expect("an icon"),
                parts.sign_device.expect("a mark"),
            );
            assert!(!rects_intersect(&sign, &icon));
            assert!(sign.x > icon.right());
            if ratio == 1.0 {
                assert_eq!(parts.icon, parts.icon_device);
                assert_eq!(parts.sign, parts.sign_device);
            }
        }
    }
}

/// The overflow button as the bar drew it, over the bar's own ground (a grab of it shows the bar
/// under it), rendered at `ratio`.
fn overflow_image(shown: &StripResponse, ratio: f64) -> Image {
    let paint = shown
        .overflow_paint
        .clone()
        .expect("the overflow button is drawn");
    ButtonPaint {
        ground: palette::window(),
        ..paint
    }
    .render(&raster(), ratio)
}

mod tool_bar_overflow {
    use super::*;

    #[test]
    fn a_toolbar_too_short_for_its_buttons_shows_its_whole_overflow_arrow() {
        // The overflow button is 12 px deep; the padding and border every tool button has left
        // its arrow 2 px, a dot. In the button's image the arrow is its two chevrons, each whole:
        // two pieces of the muted colour exactly, neither touching the button's edge, where a
        // clipped arrow would.
        for ratio in RATIOS {
            let built = draw_bar(20);
            let mut harness = Harness::new().with_pixels_per_point(ratio as f32);
            let shown = show_strip(&mut harness, &built, 300.0);
            assert!(shown.overflow, "the Draw toolbar fitted in 300 px");
            let drawn = overflow_image(&shown, ratio);
            assert_eq!(pieces_of(&drawn, palette::text_muted()), 2);
            for x in 0..drawn.width() {
                assert_ne!(drawn.pixel(x, 0), palette::text_muted(), "{x}");
                assert_ne!(
                    drawn.pixel(x, drawn.height() - 1),
                    palette::text_muted(),
                    "{x}"
                );
            }
        }
    }

    #[test]
    fn the_overflow_arrow_is_painted_in_whole_device_pixels_at_every_scale() {
        // Every pixel of the arrow is clear or the muted colour, at 100, 125, 150 and 200% and
        // at the sizes it is shown at (10 px deep in its button, 12 as the style's extent),
        // pointing down and right: two chevrons, each a piece of its own. It was two pixmaps, 12
        // and 24 px, antialiased; at 125% and 150% Qt scaled the 24 down, and the chevrons came
        // out in grey halos.
        for _screen in RATIOS {
            for down in [true, false] {
                for (width, height) in [(20, 10), (10, 20), (12, 12)] {
                    for ratio in [1.0, 1.25, 1.5, 2.0] {
                        let at = format!("{width}x{height} at {ratio}, down {down}");
                        // QIcon::pixmap(size, ratio): the engine paints the size times the ratio
                        // in pixels.
                        let device_width = q_round(f64::from(width) * ratio) as u32;
                        let device_height = q_round(f64::from(height) * ratio) as u32;
                        let image = katana_window::icons::Rasteriser::paint(
                            &raster(),
                            device_width,
                            device_height,
                            1.0,
                            &mut |canvas: &mut dyn Canvas| {
                                paint_overflow_arrow(
                                    canvas,
                                    RectF::new(
                                        0.0,
                                        0.0,
                                        f64::from(device_width),
                                        f64::from(device_height),
                                    ),
                                    down,
                                    palette::text_muted(),
                                );
                            },
                        );
                        let mut inked = 0;
                        for y in 0..image.height() {
                            for x in 0..image.width() {
                                let pixel = image.pixel(x, y);
                                if pixel.a == 0 {
                                    continue;
                                }
                                inked += 1;
                                assert_eq!(pixel, palette::text_muted(), "{at} at {x},{y}");
                            }
                        }
                        assert!(inked > 0, "{at}");
                        assert_eq!(pieces_of(&image, palette::text_muted()), 2, "{at}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_arrow_is_lit_while_a_tool_whose_button_it_hides_runs() {
        // At the window's first size the Draw toolbar hides the Vertices family behind its
        // overflow arrow, and with Delete Vertex running nothing on the toolbars said a tool ran.
        // The arrow's chevrons take the accent then, and only then - not for a tool whose button
        // shows, whose own frame says it, and not once the window is tall enough to show the
        // family.
        for ratio in RATIOS {
            let mut built = draw_bar(20);
            let mut harness = Harness::new().with_pixels_per_point(ratio as f32);
            let shown = show_strip(&mut harness, &built, 300.0);
            assert!(shown.overflow, "the Draw toolbar fitted in 300 px");
            let vertices = String::from("toolFamilyButton.Draw.Vertices");
            assert!(
                shown.hidden.contains(&vertices),
                "the Vertices family fitted in 300 px"
            );
            assert_eq!(
                pieces_of(&overflow_image(&shown, ratio), palette::text_muted()),
                2
            );

            built.actions.set_active("draw.vertex.delete");
            let shown = show_strip(&mut harness, &built, 300.0);
            let image = overflow_image(&shown, ratio);
            assert_eq!(
                pieces_of(&image, palette::accent()),
                2,
                "not lit with Delete Vertex running"
            );
            assert_eq!(pieces_of(&image, palette::text_muted()), 0);

            // A tool on a button the toolbar shows: the first shown.
            let showing = built
                .strip()
                .items
                .iter()
                .find_map(|item| match item {
                    StripItem::Action {
                        action, tool: true, ..
                    } if !shown.hidden.contains(&action.name) => Some(action.name.clone()),
                    _ => None,
                })
                .expect("no tool's button shows");
            // And Delete Vertex stops: one tool runs at a time.
            built.actions.set_active(&showing);
            assert_ne!(
                built
                    .actions
                    .action("draw.vertex.delete")
                    .and_then(|action| action.checked),
                Some(true)
            );
            let shown = show_strip(&mut harness, &built, 300.0);
            assert_eq!(
                pieces_of(&overflow_image(&shown, ratio), palette::text_muted()),
                2,
                "lit for {showing}"
            );

            // Tall enough for every button, the family shows and the arrow goes.
            built.actions.set_active("draw.vertex.delete");
            let shown = show_strip(&mut harness, &built, 300.0);
            assert_eq!(
                pieces_of(&overflow_image(&shown, ratio), palette::accent()),
                2
            );
            let tall = show_strip(&mut harness, &built, 2400.0);
            assert!(!tall.hidden.contains(&vertices));
            assert!(!tall.lit);
            built.actions.set_active("");
        }
    }

    #[test]
    fn the_arrow_is_not_lit_for_a_checked_button_behind_it_that_is_no_tool() {
        // The window's Select heads the Draw toolbar, checkable and checked while NO tool runs;
        // a toolbar short enough hides it too. Only a tool's button lights the arrow. Here the
        // plain button at the bar's end, a checkable action that is no tool, stands for it:
        // checked behind the arrow, then a tool started and stopped so the arrow is worked out
        // again, it must leave the arrow muted.
        for ratio in RATIOS {
            let mut built = draw_bar(20);
            let mut harness = Harness::new().with_pixels_per_point(ratio as f32);
            let shown = show_strip(&mut harness, &built, 300.0);
            assert!(
                shown.hidden.iter().any(|name| name == PLAIN),
                "the plain button fitted in 300 px"
            );
            assert!(built.actions.action(PLAIN).is_none());
            built.set_plain_checked(true);
            built.actions.set_active("draw.vertex.delete");
            let shown = show_strip(&mut harness, &built, 300.0);
            assert_eq!(
                pieces_of(&overflow_image(&shown, ratio), palette::accent()),
                2,
                "not lit with Delete Vertex running"
            );
            built.actions.set_active("");
            let shown = show_strip(&mut harness, &built, 300.0);
            assert!(!shown.lit);
            assert_eq!(
                pieces_of(&overflow_image(&shown, ratio), palette::text_muted()),
                2,
                "lit for a button that is no tool"
            );
            built.set_plain_checked(false);
        }
    }

    #[test]
    fn the_arrow_is_lit_for_a_tool_hidden_while_the_toolbar_was_off_the_screen() {
        // A tool running when the window is shown short - or its toolbar shown again - must
        // light the arrow on the first frame that draws the toolbar: the light is worked out
        // from what is hidden and running each frame, never from events that came while the bar
        // was not drawn.
        for ratio in RATIOS {
            let mut built = draw_bar(20);
            let mut harness = Harness::new().with_pixels_per_point(ratio as f32);
            harness.frame(|_ui| {});
            built.actions.set_active("draw.vertex.delete");
            let shown = show_strip(&mut harness, &built, 300.0);
            assert!(shown.overflow, "the Draw toolbar fitted in 300 px");
            assert!(
                shown
                    .hidden
                    .contains(&String::from("toolFamilyButton.Draw.Vertices")),
                "the Vertices family fitted in 300 px"
            );
            assert!(shown.lit);
            assert_eq!(
                pieces_of(&overflow_image(&shown, ratio), palette::accent()),
                2
            );
            built.actions.set_active("");
        }
    }

    #[test]
    fn an_expanded_toolbars_overflow_button_has_the_outline_every_checked_button_has() {
        // The overflow button is checked while its toolbar is expanded, and a checked tool
        // button is outlined in the accent. The theme's rule for the overflow button took its
        // border away to make room for the arrow, and with it the outline: a plain grey pill. It
        // keeps its border; the border's top row is the accent.
        for ratio in RATIOS {
            let built = draw_bar(20);
            let mut harness = Harness::new().with_pixels_per_point(ratio as f32);
            let shown = show_strip(&mut harness, &built, 300.0);
            let control = harness
                .snapshot()
                .control("DrawToolBar", OVERFLOW_BUTTON)
                .cloned()
                .expect("the overflow button is drawn");
            assert!(
                control.checked.is_some(),
                "the overflow button is checkable"
            );
            let at_rest = overflow_image(&shown, ratio);
            let middle = at_rest.width() / 2;
            assert_ne!(
                at_rest.pixel(middle, 0),
                palette::accent(),
                "outlined at rest"
            );
            harness.target("DrawToolBar").press(OVERFLOW_BUTTON);
            let strip = built.strip();
            let mut expanded = None;
            harness
                .run(|ui| {
                    expanded = Some(tool_bar_strip(
                        ui,
                        &strip,
                        300.0,
                        &mut |_ui: &mut Ui, _rect: Rect, _name: &str| false,
                    ));
                })
                .expect("the press is taken");
            let expanded = expanded.expect("the toolbar is drawn");
            assert!(expanded.expanded);
            let image = overflow_image(&expanded, ratio);
            assert_eq!(image.pixel(image.width() / 2, 0), palette::accent());
        }
    }
}
