//! The twins of tests/qt_widgets/tools/test_flyout_button.cpp's TitleBarButtons suite: every
//! button of a panel's and a view's title bar draws its whole icon where its bar's Close draws its
//! own (D's, the plan's section 3.1), and a view's kind switcher draws its arrow beside its icon
//! (wave 4's stragglers, on the workspace). The rest of the file is I's (the tool families, the
//! split buttons, the toolbar signs and overflow).
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
                    .width(32.0)
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
                    // 3, 3: a 20 point button's border and padding, with the 14 point icon
                    // filling what they leave (the C++'s 22 point buttons drew it at 4, 4).
                    if ratio == 1.0 {
                        assert_eq!(
                            icon,
                            IntRect::new(3, 3, 14, 14),
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

    /// The box of the pixels in `picture` that are neither the icon's ink nor `ground`: what a
    /// title bar button draws besides its icon, with nothing under the pointer - its menu's arrow.
    fn sign_box(picture: &Image, ground: Color) -> Option<IntRect> {
        let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for y in 0..picture.height() {
            for x in 0..picture.width() {
                let pixel = picture.pixel(x, y);
                if pixel != INK && pixel != ground {
                    left = left.min(x as i32);
                    top = top.min(y as i32);
                    right = right.max(x as i32);
                    bottom = bottom.max(y as i32);
                }
            }
        }
        (left <= right).then(|| IntRect::new(left, top, right - left + 1, bottom - top + 1))
    }

    fn intersect(a: &IntRect, b: &IntRect) -> bool {
        a.x <= b.right() && b.x <= a.right() && a.y <= b.bottom() && b.y <= a.bottom()
    }

    // Wave 4's stragglers: the case the file comment leaves to the workspace. The workspace is
    // built and chromed as the window builds it (ViewWorkspace::set_chrome) in an 800 x 600 window,
    // and its first view's kind switcher is read from the soft renderer's pixels, the icons lent as
    // above.
    #[test]
    fn a_views_kind_switcher_draws_its_arrow_beside_its_icon_not_on_it() {
        // The first button of every view's title bar opens a menu (Plan, 3D, section, elevation),
        // and its arrow sat on its icon's frame: 2 px over at 100% and 3 at 125%, the owner's
        // display. The design of 2026-11-16 made the bar's buttons 20 points and the switcher 32
        // (20 and the arrow's 12, as the C++'s 34 was 22 and 12): its padding box is 1..30 across
        // and 1..18 down, the 14 px icon fills the contents at 3..16 both ways, and Fusion's 8 px
        // arrow is 32 - 11.5 = 20.5..28.5, over columns 20..28 - three pixels clear of the icon.
        // The C++'s working, by hand at a ratio of 1, from Qt 6.11's qstylesheetstyle.cpp and
        // qfusionstyle.cpp: the button is 34 x 22 (makeTitleBarButton's 22, widened to 34 by the
        // workspace) with a 1 px border, so its padding box is 1..32 across and 1..20 down. The
        // arrow's box is a ::menu-indicator's default, 13 x 13 (defaultSize), set right and
        // centred in it by theme.cpp: 20..32 across, 1 + 20/2 - 13/2 = 5 down
        // (QStyle::alignedRect). Fusion draws its arrow 8 px wide and 8 x 8 / 14 = 4 deep,
        // (13 - 8) / 2 = 2.5 px in (qt_fusion_draw_arrow): 22.5..30.5, antialiased over columns
        // 22..30. The theme pads the button's right by that box's 13 px, so with the 2 px on the
        // left the contents are 3..19 across and 3..18 down, and the 14 px icon is centred at
        // 4..17 both ways - four pixels clear of the arrow. Unpadded, the contents were 3..30 and
        // the icon 10..23.
        use katana_cad::Document;
        use katana_window::host::RecordingHost;
        use katana_window::view_workspace::{Scenery, ViewWorkspace, dock_name};

        for ratio in [1.0_f32, 1.25] {
            let document = Document::new(None);
            let views = ViewWorkspace::new(&document);
            views.set_chrome(icons());
            let mut host = RecordingHost::with_document(document);
            let window = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
            let mut headless = Headless::with_pixels_per_point(ratio);
            headless
                .settle_ui(|ui| {
                    ui.scope_builder(UiBuilder::new().max_rect(window), |ui| {
                        views.ui(ui, &mut host, Scenery::default());
                    });
                })
                .expect("the workspace is drawn");
            let dock = dock_name(views.active_id());
            let frame = headless.snapshot();
            let kind = frame
                .control(&dock, "ViewKindButton")
                .unwrap_or_else(|| panic!("no view kind switcher at {ratio}"));
            assert_eq!(
                (kind.rect.width(), kind.rect.height()),
                (32.0, 20.0),
                "at {ratio}"
            );
            let whole = headless.render();
            // QWidget::grab: the button's own pixels. At 1.25 its edges fall inside pixels it shares
            // with the bar, whose blend is neither ink nor ground, so only the pixels wholly inside
            // it are taken (crop_rect's rounding out would take them).
            let inner = IntRect::new(
                (kind.rect.min.x * ratio).ceil() as i32,
                (kind.rect.min.y * ratio).ceil() as i32,
                ((kind.rect.max.x * ratio).floor() - (kind.rect.min.x * ratio).ceil()) as i32,
                ((kind.rect.max.y * ratio).floor() - (kind.rect.min.y * ratio).ceil()) as i32,
            );
            let picture = whole.copy(inner);
            // Nothing is under the pointer, so the button has no fill: its corner is the bar's
            // ground.
            let ground = picture.pixel(0, 0);
            let icon = icon_box(&picture).unwrap_or_else(|| panic!("no icon drawn at {ratio}"));
            let sign =
                sign_box(&picture, ground).unwrap_or_else(|| panic!("no arrow drawn at {ratio}"));
            if ratio == 1.0 {
                assert_eq!(icon, IntRect::new(3, 3, 14, 14), "icon {}", text(&icon));
                // The Rust arrow is always Fusion's 8 px (dock_chrome's title_bar_button).
                assert_eq!(sign.x, 20, "arrow {}", text(&sign));
                assert_eq!(sign.right(), 28, "arrow {}", text(&sign));
            }
            assert!(
                !intersect(&sign, &icon),
                "arrow {} over icon {} at {ratio}",
                text(&sign),
                text(&icon)
            );
            // Apart, not merely not over: 3 px at a ratio of 1 (the C++'s 4, with its 2 more
            // points of switcher), and at any ratio at least 3 px times it, taken down.
            // The pixels between two inclusive edges, as the C++ counts them.
            let gap = sign.x - icon.right() - 1;
            assert!(
                gap >= whole_device_pixels(3, ratio),
                "arrow {} and icon {} at {ratio}",
                text(&sign),
                text(&icon)
            );
            // The C++ then runs the window's check (menuSignClashes) over the title bars and finds
            // this button clear under its dock's name: that check over the window is the main
            // window's (katana --check-toolbars), not the workspace's, and is twinned with it.
        }
    }
}
