//! The twin of tests/qt_widgets/test_theme.cpp's Icons suite: every icon paints. The file's Theme
//! suite is C's and D's.

mod icons {
    use katana_window::canvas::{Color, RectF};
    use katana_window::icons::{Icon, all_icons, paint_icon};

    use katana_window::canvas::RasterCanvas;

    #[test]
    fn the_list_holds_every_icon_once_and_each_paints_something() {
        // Declaration order, one of each: the contact sheet and this test see every icon only if
        // the list does.
        let icons = all_icons();
        // The last enumerator, which moves whenever an icon is added at the end.
        assert_eq!(icons.len(), Icon::Download as usize + 1);
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
}
