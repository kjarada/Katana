//! The twin of tests/qt_widgets/tools/test_flyout_button.cpp's case that is D's (the plan's
//! section 3.1): every button of a panel's and a view's title bar draws its whole icon where its
//! bar's Close draws its own. The rest of the file is I's (the tool families, the split buttons,
//! the toolbar signs and overflow) and WS's (the view kind switcher's arrow, which needs the
//! workspace).
//!
//! The C++ built the workspace and the chrome as the main window does; the workspace is wave 3's,
//! so here the view's dock is installed as the workspace installs it (ViewWorkspace::installChrome):
//! its kind switcher where the icon was, 34 points wide with its menu, and its Layers and Zoom
//! Extents tools. The icons are the window's (I's) and are lent: each is painted here as a solid
//! square of one colour nothing else on a bar has, so where a button drew its icon is read from
//! the soft renderer's pixels, as the C++ read where a button drew its icon from its rendering.
//! Twinned at a ratio of 1 and again at 1.25, as qt_toolbar_signs_at_125_percent reran it.

use egui::{Rect, UiBuilder, pos2, vec2};
use katana_window::canvas::{Color, Image, IntRect};
use katana_window::dock_chrome::{
    ALWAYS, ChromeIcons, DockChrome, DockRole, DockTab, TitleBarButton,
};
use katana_window::drive::ControlKind;
use katana_window::drive::headless::Headless;
use katana_window::drive::soft_renderer::crop_rect;

mod title_bar_buttons {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct Tab(String);

    impl DockTab for Tab {
        fn dock_name(&self) -> &str {
            &self.0
        }
    }

    /// The colour every icon is painted in.
    const INK: Color = Color::rgb(255, 0, 255);

    fn icons() -> ChromeIcons {
        ChromeIcons::new(|_name, pixels| {
            let mut image = Image::new(pixels, pixels);
            image.fill(INK);
            Some(image)
        })
    }

    /// The box of the icon's pixels in `picture`; None when it drew none.
    fn icon_box(picture: &Image) -> Option<IntRect> {
        let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for y in 0..picture.height() {
            for x in 0..picture.width() {
                if picture.pixel(x, y) == INK {
                    left = left.min(x as i32);
                    top = top.min(y as i32);
                    right = right.max(x as i32);
                    bottom = bottom.max(y as i32);
                }
            }
        }
        (left <= right).then(|| IntRect::new(left, top, right - left + 1, bottom - top + 1))
    }

    /// Whole device pixels in `points` at `ratio`, taken down (the C++'s wholeDevicePixels).
    fn whole_device_pixels(points: i32, ratio: f32) -> i32 {
        (points as f32 * ratio + 1e-6).floor() as i32
    }

    fn text(area: &IntRect) -> String {
        format!(
            "x {}..{} y {}..{}",
            area.x,
            area.right(),
            area.y,
            area.bottom()
        )
    }

    #[test]
    fn every_title_bar_button_draws_its_whole_icon_where_close_draws_its_own() {
        for ratio in [1.0_f32, 1.25] {
            let mut chrome: DockChrome<Tab> = DockChrome::new(icons());
            chrome
                .install("LayersDock", "Layers", DockRole::Panel)
                .set_title("Layers");
            let view = chrome.install("View1", "PlanView", DockRole::View);
            view.set_title("Plan 1");
            view.set_leading(
                TitleBarButton::new("ViewKindButton", "PlanView", "Plan", "What this view shows")
                    .width(34.0)
                    .with_menu(),
            );
            view.add_tool(
                TitleBarButton::new("ViewLayersButton", "Layers", "Layers", "The view's layers"),
                ALWAYS,
            );
            view.add_tool(
                TitleBarButton::new(
                    "ViewZoomExtentsButton",
                    "ZoomExtents",
                    "Zoom Extents",
                    "Frame the drawing",
                ),
                ALWAYS,
            );
            let docks = [
                (
                    "LayersDock",
                    Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 200.0)),
                ),
                (
                    "View1",
                    Rect::from_min_size(pos2(0.0, 220.0), vec2(660.0, 300.0)),
                ),
            ];
            let mut headless = Headless::with_pixels_per_point(ratio);
            headless
                .settle_ui(|ui| {
                    for (name, rect) in docks {
                        ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                            chrome.dock_ui(ui, name, |_| {});
                        });
                    }
                })
                .expect("the docks are drawn");
            let frame = headless.snapshot();
            let whole = headless.render();
            let picture =
                |rect: Rect| whole.copy(crop_rect(rect, ratio, whole.width(), whole.height()));

            let mut measured = Vec::new();
            let mut without_menu = 0;
            for (dock, _) in docks {
                let close = frame
                    .control(dock, "DockCloseButton")
                    .unwrap_or_else(|| panic!("{dock} has no Close"));
                let close_icon = icon_box(&picture(close.rect))
                    .unwrap_or_else(|| panic!("{dock}'s Close drew no icon"));
                // Whole: never fewer device pixels than 14 points times the ratio, taken down.
                assert!(
                    close_icon.width >= whole_device_pixels(14, ratio),
                    "{dock} at {ratio}"
                );
                assert!(
                    close_icon.height >= whole_device_pixels(14, ratio),
                    "{dock} at {ratio}"
                );
                for button in frame.in_target(dock).filter(|control| {
                    matches!(control.kind, ControlKind::Button | ControlKind::Toggle)
                }) {
                    let place = format!("{dock} > {}", button.name);
                    measured.push(button.name.clone());
                    if button.name != "ViewKindButton" {
                        without_menu += 1;
                    }
                    let icon = icon_box(&picture(button.rect))
                        .unwrap_or_else(|| panic!("{place} drew no icon"));
                    assert_eq!(
                        icon,
                        close_icon,
                        "{place}: icon {}, Close's {} (device pixels at a ratio of {ratio})",
                        text(&icon),
                        text(&close_icon)
                    );
                    if ratio == 1.0 {
                        assert_eq!(
                            icon,
                            IntRect::new(4, 4, 14, 14),
                            "{place}: icon {}",
                            text(&icon)
                        );
                    }
                }
            }
            // Every kind of title bar button was measured, and among them the case that broke:
            // buttons with no menu.
            for name in [
                "DockMinimiseButton",
                "DockFloatButton",
                "DockMaximiseButton",
                "DockCloseButton",
                "ViewLayersButton",
                "ViewZoomExtentsButton",
                "ViewKindButton",
            ] {
                assert!(
                    measured.iter().any(|each| each == name),
                    "{name} not measured at {ratio}"
                );
            }
            assert!(without_menu > 0);
        }
    }
}
