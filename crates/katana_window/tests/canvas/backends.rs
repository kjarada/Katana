//! The backends against scenes worked by hand (the plan's entry C): the raster canvas's dashed
//! line, filled ring and text in its box; the PDF canvas's text, extractable as text; a picture
//! read back through the image reader; the toolbar's overflow arrow in its pixels; and viewports
//! through the host's clipboard. Ink is compared with a baseline drawn alike.

use std::collections::BTreeMap;

use katana_cad::plotting::Viewport;
use katana_core::ErrorCode;
use katana_window::canvas::{
    Brush, Canvas, Color, FillRule, Font, FontMetrics, FontSize, Image, ImageFormat, Path,
    PdfCanvas, PdfDocument, Pen, PenCap, PenJoin, PenStyle, PointF, RasterCanvas, RectF, TextFlags,
    decode_image, encode_png,
};
use katana_window::host::MemoryClipboard;
use katana_window::plotting::viewport_clipboard::{
    clipboard_holds_viewports, copy_viewports_to_clipboard, viewports_on_clipboard,
};
use katana_window::theme::palette;
use katana_window::theme::style::overflow_arrow;

use crate::{ink, is_ink};

/// A white canvas of `width` by `height`: a page to draw ink on.
fn page(width: u32, height: u32) -> RasterCanvas {
    let mut canvas = RasterCanvas::new(width, height);
    canvas.fill_rect(
        RectF::new(0.0, 0.0, f64::from(width), f64::from(height)),
        &Brush::solid(Color::WHITE),
    );
    canvas
}

/// The runs of ink along row `y`.
fn runs(image: &Image, y: u32) -> usize {
    let mut count = 0;
    let mut inside = false;
    for x in 0..image.width() {
        let inked = is_ink(image.pixel(x, y), Color::WHITE);
        if inked && !inside {
            count += 1;
        }
        inside = inked;
    }
    count
}

/// The ink of `image` outside `area`.
fn ink_outside(image: &Image, area: RectF) -> usize {
    let mut count = 0;
    for y in 0..image.height() {
        for x in 0..image.width() {
            let centre = PointF::new(f64::from(x) + 0.5, f64::from(y) + 0.5);
            if !area.contains(centre) && is_ink(image.pixel(x, y), Color::WHITE) {
                count += 1;
            }
        }
    }
    count
}

mod raster_canvas {
    use super::*;

    fn line(style: PenStyle) -> Image {
        let mut canvas = page(100, 20);
        canvas.set_pen(Pen::with(
            Color::BLACK,
            2.0,
            style,
            PenCap::Flat,
            PenJoin::Bevel,
        ));
        canvas.draw_line(PointF::new(0.0, 10.0), PointF::new(96.0, 10.0));
        canvas.into_image()
    }

    // Worked by hand: Qt's dash is 4 widths on and 2 off, so with a pen 2 wide a dash of 8 px and
    // a gap of 4 - a period of 12, two thirds of it ink. 96 px is eight periods: eight dashes,
    // and two thirds of the ink of the same line drawn solid, which is the baseline. Aliased
    // with flat caps at whole coordinates, both lines cover exactly the pixels whose centres
    // they hold, so the ratio is exact.
    #[test]
    fn a_dashed_line_inks_two_thirds_of_a_solid_one_in_eight_dashes() {
        let solid = line(PenStyle::Solid);
        let dashed = line(PenStyle::Dash);
        let solid_ink = ink(&solid, Color::WHITE);
        let dashed_ink = ink(&dashed, Color::WHITE);
        assert!(solid_ink > 0);
        assert_eq!(3 * dashed_ink, 2 * solid_ink);
        assert_eq!(runs(&dashed, 10), 8);
        assert_eq!(runs(&solid, 10), 1);
    }

    fn disc(rect: RectF) -> Path {
        let mut path = Path::new();
        path.add_ellipse(rect);
        path
    }

    fn filled(path: &Path) -> Image {
        let mut canvas = page(100, 100);
        canvas.fill_path(path, &Brush::solid(Color::BLACK));
        canvas.into_image()
    }

    // Worked by hand: a disc of radius 40 round (50, 50) with one of radius 20 inside it, both
    // turning the same way. Odd-even, the inner one is a hole: the ring inks what the outer disc
    // inks less what the inner disc inks, pixel for pixel (aliased, a pixel is in when its centre
    // is), and the centre is clear while (20, 50), between the circles, is ink. Winding, the
    // circles add up and the hole is filled: the outer disc's ink.
    #[test]
    fn a_filled_rings_hole_is_clear_by_the_odd_even_rule_and_filled_by_winding() {
        let outer_rect = RectF::new(10.0, 10.0, 80.0, 80.0);
        let inner_rect = RectF::new(30.0, 30.0, 40.0, 40.0);
        let mut ring = disc(outer_rect);
        ring.add_ellipse(inner_rect);
        ring.fill_rule = FillRule::OddEven;
        let odd_even = filled(&ring);
        ring.fill_rule = FillRule::Winding;
        let winding = filled(&ring);
        let outer = ink(&filled(&disc(outer_rect)), Color::WHITE);
        let inner = ink(&filled(&disc(inner_rect)), Color::WHITE);
        assert!(inner > 0 && outer > inner);

        assert!(!is_ink(odd_even.pixel(50, 50), Color::WHITE), "the hole");
        assert!(is_ink(odd_even.pixel(20, 50), Color::WHITE), "the ring");
        assert_eq!(ink(&odd_even, Color::WHITE), outer - inner);
        assert!(is_ink(winding.pixel(50, 50), Color::WHITE), "no hole");
        assert_eq!(ink(&winding, Color::WHITE), outer);
    }

    // A word centred in a box: its ink is inside the rectangle drawText answers (a pixel's
    // antialiasing aside), that rectangle is inside the box and centred in it, and the word inks
    // as much as the same word drawn at a point - the baseline - within a tenth.
    #[test]
    fn a_texts_ink_is_inside_the_box_it_is_laid_out_in() {
        let font = Font::sized(FontSize::Pixels(20));
        let mut canvas = page(200, 60);
        canvas.set_pen(Pen::new(Color::BLACK));
        canvas.set_font(font.clone());
        let rect = RectF::new(20.0, 10.0, 160.0, 40.0);
        let bounds = canvas.draw_text_in(rect, TextFlags::CENTER, "Katana");
        let image = canvas.into_image();
        let total = ink(&image, Color::WHITE);
        assert!(total > 0, "the word inks");
        assert_eq!(
            ink_outside(&image, bounds.adjusted(-1.0, -1.0, 1.0, 1.0)),
            0
        );
        assert!(rect.contains_rect(&bounds), "{bounds:?} in {rect:?}");
        assert!((bounds.center().x - rect.center().x).abs() < 1e-9);
        assert!((bounds.center().y - rect.center().y).abs() < 1e-9);

        let mut baseline = page(200, 60);
        baseline.set_pen(Pen::new(Color::BLACK));
        baseline.set_font(font);
        baseline.draw_text(PointF::new(20.0, 40.0), "Katana");
        let reference = ink(&baseline.into_image(), Color::WHITE);
        assert!(
            total * 10 >= reference * 9 && total * 10 <= reference * 11,
            "{total} against {reference}"
        );
    }

    // What is clipped away is not painted: a square filled across a clip of its left half inks
    // half of what it inks unclipped.
    #[test]
    fn a_clip_keeps_ink_inside_it() {
        let square = RectF::new(10.0, 10.0, 40.0, 40.0);
        let mut whole = page(60, 60);
        whole.fill_rect(square, &Brush::solid(Color::BLACK));
        let mut clipped = page(60, 60);
        clipped.set_clip_rect(RectF::new(0.0, 0.0, 30.0, 60.0), false);
        clipped.fill_rect(square, &Brush::solid(Color::BLACK));
        assert!(clipped.has_clipping());
        assert_eq!(
            2 * ink(&clipped.into_image(), Color::WHITE),
            ink(&whole.into_image(), Color::WHITE)
        );
    }

    // An image handed to a painter is painted in place and keeps its format: a grey one comes
    // back grey, its painted pixel Qt's grey of the colour (qGray of pure red, (11 x 255) / 32,
    // is 87), the rest as it was.
    #[test]
    fn an_image_painted_in_place_keeps_its_format() {
        let mut grey = Image::new_grey(4, 4);
        grey.fill(Color::WHITE);
        RasterCanvas::paint_image(&mut grey, &mut |canvas: &mut dyn Canvas| {
            canvas.fill_rect(
                RectF::new(1.0, 1.0, 1.0, 1.0),
                &Brush::solid(Color::rgb(255, 0, 0)),
            );
        });
        assert_eq!(grey.format(), ImageFormat::Grey8);
        assert_eq!(grey.pixel(1, 1), Color::rgb(87, 87, 87));
        assert_eq!(grey.pixel(0, 0), Color::WHITE);

        let mut colour = Image::new(4, 4);
        RasterCanvas::paint_image(&mut colour, &mut |canvas: &mut dyn Canvas| {
            canvas.fill_rect(
                RectF::new(1.0, 1.0, 1.0, 1.0),
                &Brush::solid(Color::rgb(255, 0, 0)),
            );
        });
        assert_eq!(colour.format(), ImageFormat::Rgba8Premultiplied);
        assert_eq!(colour.pixel(1, 1), Color::rgb(255, 0, 0));
        assert_eq!(colour.pixel(0, 0), Color::TRANSPARENT);
    }
}

/// The text of a PDF written uncompressed, as a reader extracts it: each string shown between BT
/// and ET, its two-byte codes mapped through the ToUnicode table to characters.
fn extracted_text(pdf: &[u8]) -> String {
    let source = String::from_utf8_lossy(pdf);
    // The ToUnicode tables: "<code> <UTF-16 hex>" lines between beginbfchar and endbfchar.
    let mut unicode: BTreeMap<u16, String> = BTreeMap::new();
    let mut rest = source.as_ref();
    while let Some(start) = rest.find("beginbfchar") {
        let after = &rest[start + "beginbfchar".len()..];
        let end = after.find("endbfchar").unwrap_or(after.len());
        for line in after[..end].lines() {
            let fields: Vec<&str> = line
                .split(['<', '>'])
                .map(str::trim)
                .filter(|field| !field.is_empty())
                .collect();
            if let [code, text] = fields[..]
                && let Ok(code) = u16::from_str_radix(code, 16)
            {
                let units: Vec<u16> = text
                    .as_bytes()
                    .chunks(4)
                    .filter_map(|quad| {
                        u16::from_str_radix(std::str::from_utf8(quad).ok()?, 16).ok()
                    })
                    .collect();
                unicode.insert(code, String::from_utf16_lossy(&units));
            }
        }
        rest = &after[end..];
    }
    // The strings shown inside text objects; BT and ET are whole tokens.
    let token = |at: usize, word: &[u8]| {
        pdf[at..].starts_with(word)
            && (at == 0 || pdf[at - 1].is_ascii_whitespace())
            && pdf.get(at + word.len()).is_none_or(u8::is_ascii_whitespace)
    };
    let mut out = String::new();
    let mut at = 0;
    while at < pdf.len() {
        if !token(at, b"BT") {
            at += 1;
            continue;
        }
        at += 2;
        while at < pdf.len() && !token(at, b"ET") {
            let bytes = match pdf[at] {
                b'(' => literal_string(pdf, &mut at),
                b'<' => hex_string(pdf, &mut at),
                _ => {
                    at += 1;
                    continue;
                }
            };
            for pair in bytes.chunks(2) {
                if let [high, low] = pair
                    && let Some(text) = unicode.get(&u16::from_be_bytes([*high, *low]))
                {
                    out.push_str(text);
                }
            }
        }
    }
    out
}

/// A PDF literal string from its opening parenthesis: its bytes, escapes undone, and `at` past it.
fn literal_string(pdf: &[u8], at: &mut usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut depth = 0;
    *at += 1;
    while *at < pdf.len() {
        let byte = pdf[*at];
        *at += 1;
        match byte {
            b'\\' => {
                let Some(&next) = pdf.get(*at) else {
                    break;
                };
                *at += 1;
                match next {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0c),
                    b'0'..=b'7' => {
                        let mut value = u32::from(next - b'0');
                        for _ in 0..2 {
                            match pdf.get(*at) {
                                Some(&digit @ b'0'..=b'7') => {
                                    value = value * 8 + u32::from(digit - b'0');
                                    *at += 1;
                                }
                                _ => break,
                            }
                        }
                        out.push(value as u8);
                    }
                    other => out.push(other),
                }
            }
            b'(' => {
                depth += 1;
                out.push(byte);
            }
            b')' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                out.push(byte);
            }
            _ => out.push(byte),
        }
    }
    out
}

/// A PDF hex string from its opening angle bracket: its bytes, and `at` past it.
fn hex_string(pdf: &[u8], at: &mut usize) -> Vec<u8> {
    *at += 1;
    let mut digits = Vec::new();
    while *at < pdf.len() && pdf[*at] != b'>' {
        if pdf[*at].is_ascii_hexdigit() {
            digits.push(pdf[*at]);
        }
        *at += 1;
    }
    *at += 1;
    digits
        .chunks(2)
        .filter_map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect()
}

mod pdf_canvas {
    use super::*;

    // A line of text on a PDF page is written as text: a reader maps the codes it shows through
    // the font's ToUnicode table back to the words drawn.
    #[test]
    fn a_pdf_pages_text_is_extractable() {
        let mut document = PdfDocument::uncompressed();
        let mut page = PdfCanvas::new(595.0, 842.0, 72.0);
        page.set_font(Font::sized(FontSize::Points(12.0)));
        page.draw_text(PointF::new(72.0, 100.0), "Katana plot");
        document.add_page(page).expect("the page is added");
        let pdf = document.finish().expect("the PDF is written");
        assert!(pdf.starts_with(b"%PDF-"));
        let text = extracted_text(&pdf);
        assert!(text.contains("Katana plot"), "extracted {text:?}");
    }

    // A page is as large as asked, in points, whatever its resolution: the device's pixels at
    // 1200 dots an inch are 1200 / 72 of a point each.
    #[test]
    fn a_pdf_pages_device_is_its_points_at_its_resolution() {
        let page = PdfCanvas::new(72.0, 144.0, 1200.0);
        let device = page.device_size();
        assert_eq!(device.width, 1200.0);
        assert_eq!(device.height, 2400.0);
        let nothing = Font::sized(FontSize::Points(0.0));
        assert_eq!(FontMetrics::height(page.metrics(), &nothing), 0.0);
    }
}

mod image_files {
    use super::*;

    // A picture written as PNG reads back as the same pixels through the image reader, which
    // finds the format from the bytes; bytes that are no picture are refused.
    #[test]
    fn a_picture_reads_back_and_bytes_that_are_none_are_refused() {
        let mut picture = Image::new(3, 2);
        picture.fill(Color::WHITE);
        picture.set_pixel(1, 0, Color::rgb(30, 35, 41));
        picture.set_pixel(2, 1, palette::accent());
        let bytes = encode_png(&picture).expect("the picture is encoded");
        let read = decode_image(&bytes).expect("the picture is read");
        assert_eq!((read.width(), read.height()), (3, 2));
        assert_eq!(read.pixel(1, 0), Color::rgb(30, 35, 41));
        assert_eq!(read.pixel(2, 1), palette::accent());
        assert_eq!(read.pixel(0, 0), Color::WHITE);
        assert_eq!(
            decode_image(b"not a picture").expect_err("no picture").code,
            ErrorCode::FileImportFailure
        );
    }
}

mod overflow_arrow {
    use super::*;

    // Worked from theme.cpp's own numbers: in the overflow button's 10 px of room at 100 %, the
    // arrow is two chevrons a pixel thick, 7 across and 4 deep each, 8 rows in all, centred - so
    // its ink spans columns 1 to 7 and rows 1 to 8 of a 10 x 10 image, in the muted text colour,
    // and in the accent while a tool runs behind it.
    #[test]
    fn the_overflow_arrow_is_two_chevrons_in_whole_pixels() {
        let arrow = overflow_arrow(10, 10, true, false, false);
        let mut columns = Vec::new();
        let mut rows = Vec::new();
        for y in 0..10 {
            for x in 0..10 {
                let pixel = arrow.pixel(x, y);
                if pixel.a > 0 {
                    assert_eq!(pixel, palette::text_muted(), "({x}, {y}) is whole ink");
                    columns.push(x);
                    rows.push(y);
                }
            }
        }
        assert_eq!(columns.iter().min(), Some(&1));
        assert_eq!(columns.iter().max(), Some(&7));
        assert_eq!(rows.iter().min(), Some(&1));
        assert_eq!(rows.iter().max(), Some(&8));
        let lit = overflow_arrow(10, 10, true, false, true);
        assert_eq!(lit.pixel(1, 1), palette::accent());
        // No room for a chevron that reads as one: nothing.
        let cramped = overflow_arrow(4, 4, true, false, false);
        assert!((0..4).all(|y| (0..4).all(|x| cramped.pixel(x, y).a == 0)));
    }
}

mod viewport_clipboard {
    use super::*;

    // Viewports copied travel through the host's clipboard as the sheets' own JSON and come back
    // as they were; text that is not copied viewports is nothing to paste.
    #[test]
    fn viewports_travel_through_the_hosts_clipboard() {
        let viewports = vec![
            Viewport {
                id: String::from("vp1"),
                scale: 500.0,
                ..Viewport::default()
            },
            Viewport {
                id: String::from("vp2"),
                scale: 1000.0,
                ..Viewport::default()
            },
        ];
        let mut clipboard = MemoryClipboard::new();
        assert!(!clipboard_holds_viewports(&mut clipboard));
        assert_eq!(
            viewports_on_clipboard(&mut clipboard)
                .expect_err("an empty clipboard")
                .code,
            ErrorCode::NotFound
        );
        copy_viewports_to_clipboard(&mut clipboard, &viewports).expect("the views are copied");
        assert!(clipboard_holds_viewports(&mut clipboard));
        assert_eq!(
            viewports_on_clipboard(&mut clipboard).expect("the views are pasted"),
            viewports
        );
        katana_window::host::Clipboard::set_text(&mut clipboard, "hello").expect("text is set");
        assert_eq!(
            viewports_on_clipboard(&mut clipboard)
                .expect_err("text is nothing to paste")
                .code,
            ErrorCode::NotFound
        );
    }
}
