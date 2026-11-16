//! The twin of tests/qt_widgets/test_theme.cpp's Icons suite: every icon paints. The file's Theme
//! suite is C's and D's. Beside it, the icon set's own rules of 2026-11-16 (icons, the module
//! comment): the strokes' weights at each size, and the accent only on an active icon.

mod icons {
    use katana_window::canvas::{Color, Image, RasterCanvas, RectF};
    use katana_window::icons::{
        Icon, IconMode, all_icons, icon_colours, paint_icon, paint_named_icon_at,
    };
    use katana_window::theme::palette;
    use katana_window::tools::flyout_button::{
        ButtonIcon, ButtonPaint, ButtonState, MenuSign, ToolButtonLook,
    };
    use katana_window::tools::tool_icons::{DESIGN_PIXELS, FINE, GRID, STROKE, weight_at};

    use crate::test_canvas::raster;

    #[test]
    fn the_list_holds_every_icon_once_and_each_paints_something() {
        // Declaration order, one of each: the contact sheet and this test see every icon only if
        // the list does.
        let icons = all_icons();
        // The last enumerator, which moves whenever an icon is added at the end.
        assert_eq!(icons.len(), Icon::OnlineData as usize + 1);
        for (index, icon) in icons.iter().enumerate() {
            assert_eq!(*icon as usize, index);
        }
        // At a menu's 16 px, the smallest size it is shown at.
        for &which in icons {
            let mut canvas = RasterCanvas::new(16, 16);
            paint_icon(
                &mut canvas,
                which,
                RectF::new(0.0, 0.0, 16.0, 16.0),
                Color::WHITE,
                Color::rgb(0, 0, 255),
            );
            let image = canvas.into_image();
            let mut ink = 0;
            for y in 0..16 {
                for x in 0..16 {
                    if image.pixel(x, y).a > 64 {
                        ink += 1;
                    }
                }
            }
            // Something to see: the set's smallest mark, Minimise's dash, is 18 pixels at this
            // size; an icon that paints nothing is 0.
            assert!(ink >= 12, "icon {} has {ink}", which as usize);
        }
    }

    #[test]
    fn no_two_icons_of_the_set_paint_the_same_picture() {
        // The third look audit's G: GIS's processing items, Terrain's Analysis and DEM items and
        // View's sizes each shared one icon. Two names painting one picture is that again, so
        // every pair is told apart at a toolbar's 20 px, both tones distinct.
        let painted: Vec<(Icon, Vec<u8>)> = all_icons()
            .iter()
            .map(|&which| {
                let mut canvas = RasterCanvas::new(20, 20);
                paint_icon(
                    &mut canvas,
                    which,
                    RectF::new(0.0, 0.0, 20.0, 20.0),
                    Color::WHITE,
                    Color::rgb(0, 0, 255),
                );
                let image = canvas.into_image();
                let mut bytes = Vec::with_capacity(20 * 20 * 4);
                for y in 0..20 {
                    for x in 0..20 {
                        let pixel = image.pixel(x, y);
                        bytes.extend([pixel.r, pixel.g, pixel.b, pixel.a]);
                    }
                }
                (which, bytes)
            })
            .collect();
        let mut same = Vec::new();
        for (at, (one, picture)) in painted.iter().enumerate() {
            for (other, theirs) in &painted[at + 1..] {
                if picture == theirs {
                    same.push(format!("{} = {}", one.name(), other.name()));
                }
            }
        }
        assert!(painted.len() > 190, "the premise: the whole set ({})", painted.len());
        assert!(same.is_empty(), "icons that paint one picture: {same:?}");
    }

    /// Whether `colour` is the accent's blue rather than one of the text greys: the accent
    /// (#4c9ffe) is 178 bluer than red, the text colours (#d7dde5, #8b97a5) 14 and 26, and an
    /// antialiased pixel keeps its hue (Image::pixel unpremultiplies), so 100 parts them.
    fn is_accent(colour: Color) -> bool {
        colour.a > 128 && i32::from(colour.b) - i32::from(colour.r) > 100
    }

    fn accent_pixels(image: &Image) -> usize {
        let mut count = 0;
        for y in 0..image.height() {
            for x in 0..image.width() {
                if is_accent(image.pixel(x, y)) {
                    count += 1;
                }
            }
        }
        count
    }

    #[test]
    fn strokes_are_one_and_a_half_pixels_at_every_size_up_to_a_toolbars_and_scale_above_it() {
        // STROKE is 1.5 px at the 20 px design size: 1.5 x 24 / 20 = 1.8 units; FINE is 1 px,
        // 1 x 24 / 20 = 1.2.
        assert!((STROKE * DESIGN_PIXELS / GRID - 1.5).abs() < 1e-12);
        assert!((FINE * DESIGN_PIXELS / GRID - 1.0).abs() < 1e-12);
        // A menu's 16 px: the weights thickened by 20 / 16, so STROKE is still 1.5 px there.
        assert!((weight_at(16.0) - 1.25).abs() < 1e-12);
        assert!((STROKE * weight_at(16.0) * 16.0 / GRID - 1.5).abs() < 1e-12);
        // A title bar's 14 px: 20 / 14.
        assert!((STROKE * weight_at(14.0) * 14.0 / GRID - 1.5).abs() < 1e-12);
        // At and above the design size the drawing scales whole.
        assert_eq!(weight_at(20.0), 1.0);
        assert_eq!(weight_at(28.0), 1.0);
        assert_eq!(weight_at(96.0), 1.0);
        // Below 12 px the weights stop thickening: at 8 px they are what they are at 12.
        assert_eq!(weight_at(8.0), weight_at(12.0));
        assert!((weight_at(12.0) - 20.0 / 12.0).abs() < 1e-12);
        // Nothing to size by: drawn as designed.
        assert_eq!(weight_at(0.0), 1.0);
        assert_eq!(weight_at(-4.0), 1.0);
        assert_eq!(weight_at(f64::NAN), 1.0);
        assert_eq!(weight_at(f64::INFINITY), 1.0);
    }

    #[test]
    fn the_accent_is_in_no_icon_at_rest_and_is_the_bright_tone_of_an_active_one() {
        let (quiet, bright) = icon_colours(IconMode::Normal);
        assert_eq!((quiet, bright), (palette::text_muted(), palette::text()));
        let (quiet, bright) = icon_colours(IconMode::Active);
        assert_eq!((quiet, bright), (palette::text(), palette::accent()));
        let (quiet, bright) = icon_colours(IconMode::Disabled);
        assert_eq!(
            (quiet, bright),
            (palette::text_disabled(), palette::text_disabled())
        );
        // The colours themselves are told apart by is_accent as its comment says, or the rest
        // of this test would prove nothing.
        assert!(is_accent(palette::accent()));
        assert!(!is_accent(palette::text()));
        assert!(!is_accent(palette::text_muted()));
        assert!(!is_accent(palette::text_disabled()));
        let paint = |name: &str, mode: IconMode| {
            let mut canvas = RasterCanvas::new(20, 20);
            assert!(paint_named_icon_at(
                &mut canvas,
                name,
                RectF::new(0.0, 0.0, 20.0, 20.0),
                mode,
                1.0
            ));
            canvas.into_image()
        };
        // Every icon of the set and a catalogue tool, at rest: not one pixel of the accent.
        for &which in all_icons() {
            assert_eq!(
                accent_pixels(&paint(which.name(), IconMode::Normal)),
                0,
                "{}",
                which.name()
            );
        }
        assert_eq!(accent_pixels(&paint("draw.circle", IconMode::Normal)), 0);
        // Active, the bright part is the accent: Select's pointer is all bright, and Circle's
        // circle is the curve the tool draws.
        assert!(accent_pixels(&paint("Select", IconMode::Active)) > 20);
        assert!(accent_pixels(&paint("draw.circle", IconMode::Active)) > 20);
    }

    #[test]
    fn a_checked_tool_buttons_icon_is_drawn_active_and_an_unchecked_ones_at_rest() {
        let look = ToolButtonLook::TOOL;
        let (width, height) = look.button_size(20);
        let button = |checked: bool| ButtonPaint {
            width,
            height,
            look,
            icon: ButtonIcon::Named(String::from("draw.circle")),
            icon_size: 20,
            state: ButtonState {
                checked,
                ..ButtonState::default()
            },
            sign: MenuSign::None,
            ground: Color::TRANSPARENT,
        };
        // Inside the icon's square only: the checked frame is the accent too, and lies outside it.
        let in_icon = |image: &Image, paint: &ButtonPaint| {
            let icon = paint.icon_rect();
            let mut count = 0;
            for y in icon.y..=icon.bottom() {
                for x in icon.x..=icon.right() {
                    if is_accent(image.pixel(x as u32, y as u32)) {
                        count += 1;
                    }
                }
            }
            count
        };
        let rest = button(false);
        assert_eq!(in_icon(&rest.render(&raster(), 1.0), &rest), 0);
        let running = button(true);
        assert!(in_icon(&running.render(&raster(), 1.0), &running) > 20);
    }
}
