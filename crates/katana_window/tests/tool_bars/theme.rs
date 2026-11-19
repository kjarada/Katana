//! The twin of tests/qt_widgets/test_theme.cpp's Icons suite: every icon paints. The file's Theme
//! suite is C's and D's. Beside it, the icon set's own rules (icons, the module comment): the
//! strokes' weights at each size (2026-11-16), the accent only on an active icon, and the colours
//! of the disciplines and the function marks (the owner's ask of 2026-11-19).

mod icons {
    use katana_cad::interactive_tool::tool_catalog;
    use katana_window::canvas::{Color, Image, RasterCanvas, RectF};
    use katana_window::icons::{
        DISCIPLINES, Discipline, Icon, IconColours, IconMode, all_icons, icon_colours,
        named_icon_discipline, paint_icon, paint_named_icon_at, tool_discipline,
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
        assert!(
            painted.len() > 190,
            "the premise: the whole set ({})",
            painted.len()
        );
        assert!(same.is_empty(), "icons that paint one picture: {same:?}");
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

    // ---- colour arithmetic, worked out here from the published formulas, not from the palette ----

    /// An sRGB channel as linear light (IEC 61966-2-1, the sRGB standard's transfer function).
    fn linear(channel: u8) -> f64 {
        let c = f64::from(channel) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    /// WCAG 2.1's relative luminance (its "relative luminance" definition).
    fn luminance(colour: Color) -> f64 {
        0.2126 * linear(colour.r) + 0.7152 * linear(colour.g) + 0.0722 * linear(colour.b)
    }

    /// WCAG 2.1's contrast ratio, (L1 + 0.05) / (L2 + 0.05), the lighter over the darker.
    fn contrast(one: Color, other: Color) -> f64 {
        let (a, b) = (luminance(one), luminance(other));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// The colour in OKLab (Bjorn Ottosson, "A perceptual color space for image processing",
    /// 2020: his published matrices from linear sRGB).
    fn oklab(colour: Color) -> [f64; 3] {
        let (r, g, b) = (linear(colour.r), linear(colour.g), linear(colour.b));
        let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
        let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
        let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
        [
            0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
            1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
            0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
        ]
    }

    /// The Euclidean distance of two colours in OKLab: about 0.02 is just noticeable.
    fn distance(one: Color, other: Color) -> f64 {
        let (a, b) = (oklab(one), oklab(other));
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    }

    /// The colour's chroma in OKLab: 0 for a grey. The window's greys are a little blue, the
    /// disabled colour #5f636b 0.014.
    fn chroma(colour: Color) -> f64 {
        let lab = oklab(colour);
        lab[1].hypot(lab[2])
    }

    /// How near a pixel must be to a colour to be counted as it. An antialiased edge keeps its
    /// stroke's colour (Image::pixel unpremultiplies), but the unpremultiplying rounds: at the
    /// alpha 128 counted below, a channel is off by up to 1 in 128, a few thousandths in OKLab;
    /// two strokes that overlap blend, which this keeps out of the count, as it should.
    const SAME: f64 = 0.03;

    /// The pixels of `image`, more than half covered, within SAME of `colour`.
    fn pixels_of(image: &Image, colour: Color) -> usize {
        let mut count = 0;
        for y in 0..image.height() {
            for x in 0..image.width() {
                let pixel = image.pixel(x, y);
                if pixel.a > 128 && distance(pixel, colour) < SAME {
                    count += 1;
                }
            }
        }
        count
    }

    /// The icon `name` at a toolbar's 20 px in `mode`, as the window paints it.
    fn painted(name: &str, mode: IconMode) -> Image {
        let mut canvas = RasterCanvas::new(20, 20);
        assert!(paint_named_icon_at(
            &mut canvas,
            name,
            RectF::new(0.0, 0.0, 20.0, 20.0),
            mode,
            1.0
        ));
        canvas.into_image()
    }

    /// Every name the window paints: the set's icons and the catalogue's tools.
    fn every_name() -> Vec<String> {
        let mut names: Vec<String> = all_icons()
            .iter()
            .map(|which| String::from(which.name()))
            .collect();
        names.extend(tool_catalog().all().into_iter().map(|tool| tool.id.clone()));
        names
    }

    /// The disciplines' hues: every discipline but General.
    fn hues() -> Vec<(Discipline, Color)> {
        DISCIPLINES
            .into_iter()
            .filter_map(|discipline| discipline.hue().map(|hue| (discipline, hue)))
            .collect()
    }

    // ---- the disciplines ----------------------------------------------------------------------

    #[test]
    fn every_icon_and_every_catalogue_tool_has_the_discipline_of_its_menu() {
        // An icon's discipline is a match with no catch-all, so it cannot be missing; what is
        // checked is that the name the window paints by finds it.
        for &which in all_icons() {
            assert_eq!(
                named_icon_discipline(which.name()),
                Some(which.discipline()),
                "{}",
                which.name()
            );
        }
        // A tool's is read from its id's family; it must agree with the catalogue's own
        // category, the menu the tool is on. A category this does not know fails, so a new one is
        // given a discipline on purpose.
        let tools = tool_catalog().all();
        assert!(
            tools.len() > 90,
            "the premise: the whole catalogue ({})",
            tools.len()
        );
        for tool in &tools {
            let expected = match tool.category.as_str() {
                "Draw" | "Modify" => Discipline::Drafting,
                "Annotate" => Discipline::Annotation,
                "Terrain" => Discipline::Terrain,
                "Tools" => Discipline::General,
                other => panic!("{}: a category with no discipline: {other}", tool.id),
            };
            assert_eq!(tool_discipline(&tool.id), expected, "{}", tool.id);
            assert_eq!(
                named_icon_discipline(&tool.id),
                Some(expected),
                "{}",
                tool.id
            );
        }
        // Not a name in the set: no discipline, as paint_named_icon paints nothing for it.
        assert_eq!(named_icon_discipline("NoSuchIcon"), None);
        assert_eq!(named_icon_discipline(""), None);
        // Every discipline with a hue colours some icon, but Utilities: its menu items borrow
        // other disciplines' icons until they have their own (docs/desktop.md, "Not done").
        for (discipline, _) in hues() {
            let used = all_icons()
                .iter()
                .any(|which| which.discipline() == discipline)
                || tools
                    .iter()
                    .any(|tool| tool_discipline(&tool.id) == discipline);
            assert_eq!(used, discipline != Discipline::Utilities, "{discipline:?}");
        }
    }

    #[test]
    fn each_disciplines_hue_is_readable_on_the_chrome_at_three_to_one_and_more() {
        // WCAG 2.1 (1.4.11, non-text contrast): a graphical object needs 3 : 1 against what is
        // next to it. An icon sits on the panel, a raised button and the window frame. The
        // palette's comments give each hue's figure against the panel; the least is Terrain's.
        assert_eq!(hues().len(), 9);
        // The marks too: the create green, and the error red the remove mark is drawn in.
        let marks = [
            (Discipline::General, palette::icon_create()),
            (Discipline::General, palette::error()),
        ];
        for (discipline, hue) in hues().into_iter().chain(marks) {
            for ground in [palette::panel(), palette::raised(), palette::window()] {
                let ratio = contrast(hue, ground);
                assert!(
                    ratio >= 3.0,
                    "{discipline:?} {} on {}: {ratio}",
                    hue.name(),
                    ground.name()
                );
            }
        }
        let least = hues()
            .into_iter()
            .map(|(_, hue)| contrast(hue, palette::panel()))
            .fold(f64::INFINITY, f64::min);
        // Terrain's #c9983f on the panel, by hand: luminance 0.3523 against the panel's 0.0195,
        // (0.3523 + 0.05) / (0.0195 + 0.05) = 5.79, which the palette states as 5.8.
        assert!((least - contrast(palette::discipline_terrain(), palette::panel())).abs() < 1e-12);
        assert!((least - 5.79).abs() < 0.01, "{least}");
        // And none outshines the text colour: the drawing and the words stay the brightest.
        for (discipline, hue) in hues() {
            assert!(
                luminance(hue) < luminance(palette::text()),
                "{discipline:?} is brighter than the text"
            );
        }
    }

    #[test]
    fn the_hues_are_told_apart_from_each_other_and_from_the_accent_the_error_and_the_greys() {
        // 0.08 in OKLab is four times the just-noticeable 0.02 (Ottosson's space is built so
        // that a distance is a perceived difference). The closest pair is Survey and the create
        // green, 0.084 (palette, "the icons' disciplines").
        const APART: f64 = 0.08;
        let mut set = hues();
        set.push((Discipline::General, palette::icon_create()));
        let mut closest = f64::INFINITY;
        for (at, &(one, colour)) in set.iter().enumerate() {
            for &(other, theirs) in &set[at + 1..] {
                let apart = distance(colour, theirs);
                closest = closest.min(apart);
                assert!(apart >= APART, "{one:?} and {other:?}: {apart}");
            }
            // The accent means active, chosen, primary - and is the plan's selection colour;
            // the error red is the remove mark; the selection is a list's chosen row; and the
            // two greys are the quiet tone and General's bright one.
            for (name, fixed) in [
                ("accent", palette::accent()),
                ("error", palette::error()),
                ("selection", palette::selection()),
                ("text", palette::text()),
                ("muted text", palette::text_muted()),
                ("disabled text", palette::text_disabled()),
            ] {
                let apart = distance(colour, fixed);
                closest = closest.min(apart);
                assert!(apart >= APART, "{one:?} and the {name}: {apart}");
            }
        }
        assert!(
            closest < 0.09,
            "the stated closest pair is 0.084: {closest}"
        );
        // The arithmetic itself, on a pair worked by hand: black and white are 1 apart in
        // OKLab, and a grey has no chroma.
        assert!((distance(Color::rgb(0, 0, 0), Color::rgb(255, 255, 255)) - 1.0).abs() < 1e-3);
        assert!(chroma(Color::rgb(128, 128, 128)) < 1e-6);
    }

    #[test]
    fn an_icon_is_painted_in_its_disciplines_hue_at_rest_and_active_and_greyed_when_disabled() {
        for discipline in DISCIPLINES {
            let marks = (palette::icon_create(), palette::error());
            let normal = icon_colours(IconMode::Normal, discipline);
            let active = icon_colours(IconMode::Active, discipline);
            let disabled = icon_colours(IconMode::Disabled, discipline);
            // At rest: the object quiet, the bright part the hue - the text colour for General.
            let hue = discipline.hue().unwrap_or(palette::text());
            assert_eq!((normal.quiet, normal.bright), (palette::text_muted(), hue));
            // Active: the object lit, the hue kept - the button's accent frame says it runs; a
            // General icon, with no hue, takes the accent as before.
            let lit = discipline.hue().unwrap_or(palette::accent());
            assert_eq!((active.quiet, active.bright), (palette::text(), lit));
            for colours in [normal, active] {
                assert_eq!((colours.create, colours.remove), marks, "{discipline:?}");
            }
            // Disabled: nothing coloured, marks included.
            let grey = palette::text_disabled();
            assert_eq!(disabled, IconColours::two(grey, grey));
            assert_eq!((disabled.create, disabled.remove), (grey, grey));
        }
        // On the canvas: GIS's Buffer shows teal at rest and active, Survey's Inverse green,
        // a draw tool's circle cyan, and none of them a pixel of another discipline's hue.
        for (name, discipline) in [
            ("GisBuffer", Discipline::Gis),
            ("SurveyInverse", Discipline::Survey),
            ("draw.circle", Discipline::Drafting),
            ("annotate.text", Discipline::Annotation),
            ("Sheets", Discipline::Plotting),
        ] {
            let hue = discipline.hue().expect("a coloured discipline");
            for mode in [IconMode::Normal, IconMode::Active] {
                let image = painted(name, mode);
                assert!(pixels_of(&image, hue) > 10, "{name} {mode:?}");
                for (other, theirs) in hues() {
                    if other != discipline {
                        assert_eq!(pixels_of(&image, theirs), 0, "{name} {mode:?} {other:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn every_coloured_icon_shows_its_hue_or_a_function_mark_at_rest() {
        // The table is only worth having if the painter uses it: an icon of a coloured
        // discipline whose bright part were drawn quiet would show none of its hue.
        let mut missing = Vec::new();
        for name in every_name() {
            let discipline = named_icon_discipline(&name).expect("a name of the set");
            let Some(hue) = discipline.hue() else {
                continue;
            };
            let image = painted(&name, IconMode::Normal);
            let shown = pixels_of(&image, hue)
                + pixels_of(&image, palette::icon_create())
                + pixels_of(&image, palette::error());
            if shown == 0 {
                missing.push(name);
            }
        }
        assert!(
            missing.is_empty(),
            "coloured icons showing no colour: {missing:?}"
        );
    }

    #[test]
    fn a_disabled_icon_has_no_coloured_pixel() {
        // Every pixel of every icon and tool, disabled, is the disabled grey's chroma: a command
        // that cannot run must not look like a discipline's, nor like a delete. Pixels a quarter
        // covered and more, where the unpremultiplied colour is within a channel step or two.
        let limit = chroma(palette::text_disabled()) + 0.02;
        for name in every_name() {
            let image = painted(&name, IconMode::Disabled);
            let mut inked = 0;
            for y in 0..image.height() {
                for x in 0..image.width() {
                    let pixel = image.pixel(x, y);
                    if pixel.a > 64 {
                        inked += 1;
                        assert!(
                            chroma(pixel) <= limit,
                            "{name} at {x},{y}: {}",
                            pixel.name()
                        );
                    }
                }
            }
            assert!(inked > 0, "{name} painted nothing");
        }
        // The check can fail: the same icon at rest has coloured pixels.
        let rest = painted("GisBuffer", IconMode::Normal);
        assert!(pixels_of(&rest, palette::discipline_gis()) > 0);
    }

    #[test]
    fn an_active_icon_differs_from_the_same_icon_at_rest() {
        // A running tool's icon must not look as it does at rest: the object is lit (text
        // colour for muted), and General's bright part takes the accent.
        let mut same = Vec::new();
        for name in every_name() {
            if painted(&name, IconMode::Normal) == painted(&name, IconMode::Active) {
                same.push(name);
            }
        }
        assert!(
            same.is_empty(),
            "icons that do not change when active: {same:?}"
        );
    }

    #[test]
    fn function_marks_are_the_create_green_and_the_error_red_whatever_the_discipline() {
        let green = palette::icon_create();
        let red = palette::error();
        // Created: the plus of New (General), of a new layer (Format), of an inserted vertex
        // (Drafting), of an added surface point (Terrain); a check passed: GIS's tick.
        for name in [
            "New",
            "LayerNew",
            "LayerNewChild",
            "draw.vertex.insert",
            "terrain.edit.addpoint",
            "GisCheck",
        ] {
            let image = painted(name, IconMode::Normal);
            assert!(pixels_of(&image, green) > 3, "{name} has no create mark");
            assert_eq!(pixels_of(&image, red), 0, "{name}");
        }
        // Deleted: Erase's bin, the erase tool's, a deleted vertex, deleted surface points and
        // breaklines, thinned vertices and a long triangle struck out.
        for name in [
            "Erase",
            "modify.erase",
            "draw.vertex.delete",
            "terrain.edit.deletepoint",
            "SurfaceDeletePoints",
            "SurfaceDeleteBreakline",
            "SurfaceThin",
            "SurfaceDeleteLong",
        ] {
            let image = painted(name, IconMode::Normal);
            assert!(pixels_of(&image, red) > 3, "{name} has no remove mark");
            assert_eq!(pixels_of(&image, green), 0, "{name}");
        }
        // Sparingly: an ordinary draw tool and an ordinary GIS command carry neither.
        for name in ["draw.line", "GisBuffer", "SurveyTraverse", "Open"] {
            let image = painted(name, IconMode::Normal);
            assert_eq!(
                pixels_of(&image, green) + pixels_of(&image, red),
                0,
                "{name}"
            );
        }
    }

    // ---- the accent ---------------------------------------------------------------------------

    /// Whether `colour` is the accent's blue: within SAME of it in OKLab, more than half covered.
    /// The disciplines' hues are all at least 0.08 from it (the test above), so none is counted.
    fn is_accent(colour: Color) -> bool {
        colour.a > 128 && distance(colour, palette::accent()) < SAME
    }

    fn accent_pixels(image: &Image) -> usize {
        pixels_of(image, palette::accent())
    }

    #[test]
    fn the_accent_is_in_no_icon_at_rest_and_is_the_bright_tone_of_an_active_general_one() {
        // is_accent tells the accent from every colour an icon is painted in, or the rest of
        // this test would prove nothing.
        assert!(is_accent(palette::accent()));
        for other in [
            palette::text(),
            palette::text_muted(),
            palette::text_disabled(),
        ] {
            assert!(!is_accent(other));
        }
        for (_, hue) in hues() {
            assert!(!is_accent(hue), "{}", hue.name());
        }
        // Every icon of the set and every catalogue tool, at rest: not one pixel of the accent.
        for name in every_name() {
            assert_eq!(
                accent_pixels(&painted(&name, IconMode::Normal)),
                0,
                "{name}"
            );
        }
        // Active, a General icon's bright part is the accent: Select's pointer is all bright.
        assert!(accent_pixels(&painted("Select", IconMode::Active)) > 20);
        // A coloured one keeps its hue: Circle's circle stays the drafting cyan.
        assert_eq!(accent_pixels(&painted("draw.circle", IconMode::Active)), 0);
    }

    #[test]
    fn a_checked_tool_buttons_icon_is_drawn_active_and_an_unchecked_ones_at_rest() {
        let look = ToolButtonLook::TOOL;
        let (width, height) = look.button_size(20);
        let button = |icon: &str, checked: bool| ButtonPaint {
            width,
            height,
            look,
            icon: ButtonIcon::Named(String::from(icon)),
            icon_size: 20,
            state: ButtonState {
                checked,
                ..ButtonState::default()
            },
            sign: MenuSign::None,
            ground: Color::TRANSPARENT,
        };
        // Inside the icon's square only: the checked frame is the accent, and lies outside it.
        let in_icon = |image: &Image, paint: &ButtonPaint, colour: Color| {
            let icon = paint.icon_rect();
            let mut count = 0;
            for y in icon.y..=icon.bottom() {
                for x in icon.x..=icon.right() {
                    let pixel = image.pixel(x as u32, y as u32);
                    if distance(pixel, colour) < SAME {
                        count += 1;
                    }
                }
            }
            count
        };
        // A General tool: its bright part the text colour at rest, the accent while it runs.
        let rest = button("Select", false);
        let rest_image = rest.render(&raster(), 1.0);
        assert_eq!(in_icon(&rest_image, &rest, palette::accent()), 0);
        let running = button("Select", true);
        assert!(in_icon(&running.render(&raster(), 1.0), &running, palette::accent()) > 20);
        // A drafting tool: the cyan at rest and running; running, its object lit and the
        // accent frame round it - the frame is the sign, not the icon.
        let cyan = palette::discipline_drafting();
        let rest = button("draw.circle", false);
        let rest_image = rest.render(&raster(), 1.0);
        let running = button("draw.circle", true);
        let running_image = running.render(&raster(), 1.0);
        assert!(in_icon(&rest_image, &rest, cyan) > 20);
        assert!(in_icon(&running_image, &running, cyan) > 20);
        assert_eq!(in_icon(&running_image, &running, palette::accent()), 0);
        assert_ne!(rest_image, running_image);
        let frame = |image: &Image| {
            let mut count = 0;
            for y in 0..image.height() {
                for x in 0..image.width() {
                    if is_accent(image.pixel(x, y)) {
                        count += 1;
                    }
                }
            }
            count
        };
        assert_eq!(frame(&rest_image), 0);
        assert!(
            frame(&running_image) > 20,
            "the running tool has no accent frame"
        );
    }
}
