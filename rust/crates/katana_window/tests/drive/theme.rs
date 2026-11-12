//! The twins of tests/qt_widgets/test_theme.cpp that are D's (the plan's section 3.1): a view
//! bar's Link button, as the chrome makes it, framed in the accent when checked and dashed while
//! it waits for a second view. The rest of the file is C's (the theme applied to egui) and I's
//! (the icons).
//!
//! The button is drawn by the soft renderer as the headless window draws it, and read as the C++
//! read Qt's grab: along the middle of its top edge, clear of its rounded corners, how many pixels
//! are the accent's blue - blue at least twice red and not near black, a ratio rather than a
//! distance to the accent, so a pixel the frame only partly covers still counts.

use egui::{Pos2, Rect, Vec2};
use katana_window::canvas::Image;
use katana_window::dock_chrome::{ChromeIcons, TitleBarButton, title_bar_button};
use katana_window::drive::headless::Headless;

mod theme {
    use super::*;

    /// blueOnTopEdge: the accent's pixels along the top edge, five in from either end.
    fn blue_on_top_edge(image: &Image) -> usize {
        let mut count = 0;
        for x in 5..image.width().saturating_sub(5) {
            let pixel = image.pixel(x, 0);
            count += usize::from(u32::from(pixel.b) > 2 * u32::from(pixel.r) && pixel.b > 40);
        }
        count
    }

    /// linkButtonShot: a view bar's Link button as the chrome makes it, under the theme, drawn at
    /// its 22 points and grabbed.
    fn link_button_shot(checked: bool, waiting: bool) -> Image {
        let button = TitleBarButton::new("ViewLinkButton", "ViewLinked", "Linked", "tip")
            .checkable(checked)
            .waiting(waiting);
        let at = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::splat(22.0));
        let mut icons = ChromeIcons::none();
        let mut headless = Headless::new();
        headless
            .settle_ui(|ui| {
                title_bar_button(ui, at, &button, &mut icons);
            })
            .expect("the button is drawn");
        headless.picture_of(at)
    }

    #[test]
    fn a_checked_title_bar_button_is_framed_in_the_accent_and_an_unchecked_one_is_not() {
        // A view's Link, checked, wears the accent frame a checked toolbar button wears.
        let checked = link_button_shot(true, false);
        let unchecked = link_button_shot(false, false);
        assert!(checked.width() >= 22);
        let straight = checked.width() as usize - 10;
        assert_eq!(blue_on_top_edge(&checked), straight, "a solid accent edge");
        assert_eq!(blue_on_top_edge(&unchecked), 0);
    }

    #[test]
    fn a_link_waiting_for_a_second_view_is_framed_dashed_not_solid() {
        // A link of one moves nothing until a second view joins; its frame is dashed, so the
        // first of the two clicks does not look like the last. Some of the straight edge is the
        // accent (the dashes) and some is not (the gaps between them).
        let waiting = link_button_shot(true, true);
        let straight = waiting.width() as usize - 10;
        let dashes = blue_on_top_edge(&waiting);
        assert!(dashes > 0, "the dashes");
        assert!(dashes < straight, "and the gaps");
    }
}
