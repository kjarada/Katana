//! The twins of tests/qt_widgets/customisation/test_shared_style_preview.cpp: a style, a library
//! linestyle or a library symbol painted at a plot scale on paper or on the screen. The pictures
//! are read back pixel by pixel and compared with what was worked out by hand from the
//! definitions.
//!
//! The C++'s `resize` and `grab` are one call here: the pane is grabbed at the size given, a pixel
//! a point.

use katana_cad::Document;
use katana_entity::{Color as EntityColor, LineStyle, Stroke, StrokeOp, Style, StyleLibrary};
use katana_entity::StyleUnits;
use katana_geometry::Point2;
use katana_window::canvas::{Color, Image};
use katana_window::customisation::{DefinitionAs, PreviewGround, StylePreview};

fn stroke(op: StrokeOp, x: f64, y: f64) -> Stroke {
    Stroke {
        op,
        point: Point2::new(x, y),
        ..Stroke::default()
    }
}

/// The fixture's 'TEST Dashed Kerb', written out here: a paper linestyle with a 4 mm period, a
/// 1.5 mm dash, a 0.5 mm gap, a 1 mm dash and a 1 mm gap.
fn dashed_kerb() -> LineStyle {
    LineStyle {
        name: String::from("TEST Dashed Kerb"),
        units: StyleUnits::Paper,
        length: 4.0,
        strokes: vec![
            stroke(StrokeOp::Move, 0.0, 0.0),
            stroke(StrokeOp::Draw, 1.5, 0.0),
            stroke(StrokeOp::Move, 2.0, 0.0),
            stroke(StrokeOp::Draw, 3.0, 0.0),
            stroke(StrokeOp::Move, 4.0, 0.0),
        ],
        ..LineStyle::default()
    }
}

/// A symbol drawn round its insertion point.
fn ring() -> LineStyle {
    LineStyle {
        name: String::from("TEST Ring"),
        at_vertices: true,
        strokes: vec![
            stroke(StrokeOp::Move, 0.0, 0.0),
            Stroke {
                op: StrokeOp::Circle,
                radius: 0.5,
                ..Stroke::default()
            },
        ],
        ..LineStyle::default()
    }
}

/// A symbol drawn wholly away from its insertion point (a square from (1, 1) to (2, 2)).
fn offset_square() -> LineStyle {
    LineStyle {
        name: String::from("TEST Offset"),
        at_vertices: true,
        strokes: vec![
            stroke(StrokeOp::Move, 1.0, 1.0),
            stroke(StrokeOp::Draw, 2.0, 1.0),
            stroke(StrokeOp::Draw, 2.0, 2.0),
            stroke(StrokeOp::Draw, 1.0, 2.0),
            stroke(StrokeOp::Draw, 1.0, 1.0),
        ],
        ..LineStyle::default()
    }
}

fn fixture() -> Document {
    let mut library = StyleLibrary::new();
    library.add(dashed_kerb()).expect("the kerb is added");
    library.add(ring()).expect("the ring is added");
    library.add(offset_square()).expect("the square is added");
    let mut document = Document::new(None);
    document.set_style_library(library);
    document
}

/// QColor::lightnessF: the HSL lightness, 0 black to 1 white.
fn lightness_f(colour: Color) -> f64 {
    let most = colour.r.max(colour.g).max(colour.b);
    let least = colour.r.min(colour.g).min(colour.b);
    (f64::from(most) + f64::from(least)) / 2.0 / 255.0
}

/// QColor::lightness: the same, 0 to 255.
fn lightness(colour: Color) -> i32 {
    (lightness_f(colour) * 255.0).round() as i32
}

/// How much ink column `x` holds on paper: the sum, over rows y0-2 .. y0+2, of each pixel's
/// darkness (0 white, 1 black). A 1-pixel line covering a column fully sums to 1 whichever rows it
/// straddles.
fn column_ink(image: &Image, x: u32, y0: u32) -> f64 {
    (y0 - 2..=y0 + 2)
        .map(|y| 1.0 - lightness_f(image.pixel(x, y)))
        .sum()
}

#[derive(Debug, Default, PartialEq, Eq)]
struct InkCount {
    /// Columns at least half covered.
    columns: i32,
    /// Separate runs of such columns: the dashes seen.
    runs: i32,
}

fn count_ink(image: &Image, from: u32, to: u32, y0: u32) -> InkCount {
    let mut count = InkCount::default();
    let mut in_run = false;
    for x in from..to {
        let inked = column_ink(image, x, y0) >= 0.5;
        count.columns += i32::from(inked);
        count.runs += i32::from(inked && !in_run);
        in_run = inked;
    }
    count
}

fn near(value: f64, expected: f64, tolerance: f64) -> bool {
    (value - expected).abs() <= tolerance
}

mod style_preview {
    use super::*;

    #[test]
    fn a_paper_linestyle_with_gaps_is_painted_with_its_gaps_not_solid() {
        // Worked by hand, for a 400 x 160 pane at 1:500 on paper:
        //   drawing area 400 x (160 - 24 bar) = 400 x 136; usable 376 x 112 after 12-pixel
        //   margins; the sample is w wide and w/4 high, so 376 pixels wide (4 x 112 = 448 would
        //   not fit across). Printed size: 96/25.4 = 3.7795 pixels per plot mm, whatever the
        //   scale, for a paper linestyle. The sample's start, model (0, 0), is at the usable
        //   area's left edge, x = 12, and its bottom edge, y = 136/2 + 376/8 = 68 + 47 = 115.
        //   Along the straight run (0.45 x 376 = 169 pixels) each 4 mm period is 15.118 px: a dash
        //   over [0, 5.669], a gap to 7.559, a dash to 11.339, a gap to 15.118. Over the first 4
        //   periods (x from 12 to 72) a column is at least half covered where its centre lies in a
        //   dash:
        //     period 0: 12-17 (6), 20-22 (3)   period 1: 27-32 (6), 35-37 (3)
        //     period 2: 42-47 (6), 50-53 (4)   period 3: 57-62 (6), 65-68 (4)
        //   = 38 columns in 8 separate dashes; solid would be 60 in 1. Some dash ends fall within
        //   0.06 px of half a column (19.559 and 53.575), so the count is allowed a column either
        //   way at each of those; the 8 dashes are exact - every gap clears a whole column.
        let document = fixture();
        let style = Style {
            name: String::from("kerb"),
            linetype: String::from("TEST Dashed Kerb"),
            ..Style::default()
        };
        // The default 0.25 mm weight is 0.945 px, painted at the 1 px minimum.
        for as_style in [true, false] {
            let what = if as_style { "style" } else { "linestyle" };
            let mut preview = StylePreview::new(&document);
            preview.set_scale_denominator(500);
            preview.set_ground(PreviewGround::Paper);
            if as_style {
                preview.set_style(&style);
            } else {
                preview.set_linestyle("TEST Dashed Kerb");
            }
            let image = preview.grab(&document, 400, 160, 1.0);
            assert_eq!(image.width(), 400);
            let start = preview.view().world_to_screen(Point2::new(0.0, 0.0));
            assert!(near(start.x, 12.0, 1e-9), "{}", start.x);
            assert!(near(start.y, 115.0, 1e-9), "{}", start.y);

            let count = count_ink(&image, 12, 72, 115);
            assert_eq!(count.runs, 8, "{what}");
            assert!((count.columns - 38).abs() <= 2, "{what}: {}", count.columns);
            assert!(preview.notice().is_empty(), "{}", preview.notice());
        }
    }

    #[test]
    fn a_white_style_prints_black_on_paper_and_stays_white_on_the_screen() {
        let document = fixture();
        let style = Style {
            name: String::from("white"),
            color: Some(EntityColor::new(255, 255, 255, 255)),
            ..Style::default()
        };
        let mut preview = StylePreview::new(&document);
        preview.set_style(&style);

        // Paper: decision D7. The run is at y = 115 (worked in the test above).
        preview.set_ground(PreviewGround::Paper);
        assert_eq!(preview.ink_colour(style.color), Color::rgb(0, 0, 0));
        let image = preview.grab(&document, 400, 160, 1.0);
        assert_eq!(image.pixel(2, 2), Color::WHITE); // the ground
        // A black 1-pixel line covers the column fully: ink 1. White ink on white paper would sum
        // to 0.
        assert!(near(column_ink(&image, 100, 115), 1.0, 0.05));

        // Screen: the dark ground, and the white line lighter than it.
        preview.set_ground(PreviewGround::Screen);
        assert_eq!(preview.ink_colour(style.color), Color::rgb(255, 255, 255));
        let image = preview.grab(&document, 400, 160, 1.0);
        let ground = image.pixel(2, 2);
        assert!(lightness(ground) < 64);
        let lightest = (113..=117)
            .map(|y| lightness(image.pixel(100, y)))
            .max()
            .unwrap_or(0);
        assert!(lightest > lightness(ground) + 100);
    }

    #[test]
    fn the_crosshair_sits_on_the_insertion_point_at_the_panes_centre() {
        // Worked by hand: a 200 x 150 pane leaves a 200 x 126 drawing area above the 24-pixel
        // scale bar. The view is centred on the insertion point (the model origin), so it is
        // painted at (200/2, 126/2) = (100, 63). The ring's radius 0.5 m is fitted into 176 x 102
        // usable pixels (12 px margins): 102 / (2 x 0.5) = 102 px per metre, a 51-pixel radius,
        // well clear of the crosshair's 10-pixel arms.
        let document = fixture();
        let mut preview = StylePreview::new(&document);
        preview.set_ground(PreviewGround::Paper);
        preview.set_symbol("TEST Ring", 0.0);
        let image = preview.grab(&document, 200, 150, 1.0);

        let insertion = preview.insertion_point().expect("a symbol has one");
        assert!(near(insertion.x, 100.0, 1e-9));
        assert!(near(insertion.y, 63.0, 1e-9));
        assert!(near(preview.view().scale, 102.0, 1e-9));
        let mark = StylePreview::insertion_mark_colour();
        assert_eq!(image.pixel(100, 63), mark);
        assert_eq!(image.pixel(106, 63), mark);
        assert_eq!(image.pixel(94, 63), mark);
        assert_eq!(image.pixel(100, 57), mark);
        assert_eq!(image.pixel(100, 69), mark);
        // The arms end at 10 pixels.
        assert_ne!(image.pixel(100 + 13, 63), mark);
        assert!(!preview.origin_outside_extent());
    }

    #[test]
    fn a_symbol_drawn_away_from_its_insertion_point_is_flagged() {
        let document = fixture();
        let mut preview = StylePreview::new(&document);
        preview.set_symbol("TEST Offset", 0.0);
        let _ = preview.grab(&document, 200, 150, 1.0);
        // The square spans (1, 1) to (2, 2): the origin is outside it.
        assert!(preview.origin_outside_extent());

        // An undefined name is drawn as its built-in fallback, and said so.
        preview.set_symbol("Grated Pit", 0.0);
        let _ = preview.grab(&document, 200, 150, 1.0);
        assert!(preview.notice().contains("manhole"), "{}", preview.notice());
    }

    #[test]
    fn a_symbol_at_vertices_asked_for_as_a_linestyle_is_drawn_plain_and_the_notice_says_what_it_is()
     {
        // TEST Ring is in the library - as a symbol, drawn at vertices. Asked for as a LINESTYLE
        // it is a plain line, and the notice names what it is by the phrase the Style Manager's
        // "Drawn as" has for the same state (the table "The words a rule is shown by" in
        // docs/customisation.md).
        let document = fixture();
        let mut preview = StylePreview::new(&document);
        preview.set_linestyle("TEST Ring");
        let _ = preview.grab(&document, 400, 160, 1.0);
        assert_eq!(
            preview.notice(),
            "\"TEST Ring\" is an `at vertices` symbol, not a linestyle: a line naming it is drawn \
             plain."
        );

        // A name nothing defines is the other notice, and says nothing of symbols.
        preview.set_linestyle("TEST Nothing");
        let _ = preview.grab(&document, 400, 160, 1.0);
        assert!(
            preview.notice().to_lowercase().contains("not defined"),
            "{}",
            preview.notice()
        );
        assert!(!preview.notice().contains("symbol"), "{}", preview.notice());
    }

    #[test]
    fn the_scale_bar_is_a_round_number_of_ground_metres_for_the_chosen_scale() {
        // Worked by hand for a 400-pixel-wide pane: the bar is the longest 1-2-5 length of at most
        // a quarter of the width, 100 px, at 3.7795 px per plot mm and N/1000 m per plot mm:
        //   1:100  37.795 px/m -> 100 px = 2.65 m -> 2 m
        //   1:500   7.559 px/m -> 13.2 m -> 10 m
        //   1:1000  3.780 px/m -> 26.5 m -> 20 m
        let document = fixture();
        let style = Style {
            name: String::from("plain"),
            ..Style::default()
        };
        let mut preview = StylePreview::new(&document);
        preview.set_style(&style);
        for (denominator, metres) in [(100, 2.0), (500, 10.0), (1000, 20.0)] {
            preview.set_scale_denominator(denominator);
            assert_eq!(preview.paper_scale(), f64::from(denominator) / 1000.0);
            let _ = preview.grab(&document, 400, 160, 1.0);
            assert_eq!(preview.scale_bar_metres(), metres, "1:{denominator}");
        }
    }

    #[test]
    fn a_preview_whose_document_has_gone_paints_only_its_ground() {
        let document = Document::new(None);
        let mut preview = StylePreview::new(&document);
        let style = Style {
            name: String::from("plain"),
            ..Style::default()
        };
        preview.set_style(&style);
        drop(document);
        // A stranger stands where the Document was: the preview is lent it and must not read it.
        let stranger = fixture();
        let image = preview.grab(&stranger, 200, 100, 1.0);
        assert_eq!(image.pixel(100, 50), Color::WHITE);
        assert_eq!(preview.scale_bar_metres(), 0.0);
    }

    #[test]
    fn an_unsaved_definition_is_painted_as_one_in_the_library_is_without_being_put_there() {
        // The drawing has NO library. The dashed kerb and the ring are handed to the pane as the
        // definition editor hands over what is typed, and are painted exactly where the tests
        // above worked out by hand that a library definition is: the kerb's first four periods as
        // 8 dashes over about 38 columns from x = 12 on the row y = 115, the ring fitted at 102
        // pixels a metre round a crosshair at (100, 63).
        let document = Document::new(None);
        let generation = document.library_generation();
        {
            let mut preview = StylePreview::new(&document);
            preview.set_scale_denominator(500);
            preview.set_definition(&dashed_kerb(), DefinitionAs::Linestyle);
            let image = preview.grab(&document, 400, 160, 1.0);
            let start = preview.view().world_to_screen(Point2::new(0.0, 0.0));
            assert!(near(start.x, 12.0, 1e-9));
            assert!(near(start.y, 115.0, 1e-9));
            let count = count_ink(&image, 12, 72, 115);
            assert_eq!(count.runs, 8);
            assert!((count.columns - 38).abs() <= 2, "{}", count.columns);
            assert!(preview.notice().is_empty(), "{}", preview.notice());
        }
        {
            let mut preview = StylePreview::new(&document);
            preview.set_definition(&ring(), DefinitionAs::Symbol);
            let image = preview.grab(&document, 200, 150, 1.0);
            let insertion = preview.insertion_point().expect("a symbol has one");
            assert!(near(insertion.x, 100.0, 1e-9));
            assert!(near(insertion.y, 63.0, 1e-9));
            assert!(near(preview.view().scale, 102.0, 1e-9));
            assert_eq!(image.pixel(100, 63), StylePreview::insertion_mark_colour());
            assert!(preview.notice().is_empty(), "{}", preview.notice());
        }
        // Nothing reached the Document: its library is as empty and as old as it was.
        assert!(document.style_library().empty());
        assert_eq!(document.library_generation(), generation);
    }

    #[test]
    fn an_unsaved_definition_edited_under_one_name_is_painted_as_it_now_is() {
        // The same name at every keystroke: a cache of flattened definitions keyed on names would
        // paint the first for ever. The ring's radius 0.5 fits at 102 px a metre (worked above);
        // at radius 1 it is 102 / (2 x 1) = 51.
        let document = fixture();
        let mut preview = StylePreview::new(&document);
        let mut edited = ring();
        preview.set_definition(&edited, DefinitionAs::Symbol);
        let _ = preview.grab(&document, 200, 150, 1.0);
        assert!(near(preview.view().scale, 102.0, 1e-9));
        edited.strokes.last_mut().expect("the circle").radius = 1.0;
        preview.set_definition(&edited, DefinitionAs::Symbol);
        let _ = preview.grab(&document, 200, 150, 1.0);
        assert!(near(preview.view().scale, 51.0, 1e-9));

        // Asked for by NAME again, it is the library's definition - radius 0.5 - that is painted,
        // not the edit that was never saved.
        preview.set_symbol("TEST Ring", 0.0);
        let _ = preview.grab(&document, 200, 150, 1.0);
        assert!(near(preview.view().scale, 102.0, 1e-9));

        // A definition no library would take - no name - shows nothing at all.
        preview.set_definition(&LineStyle::default(), DefinitionAs::Symbol);
        let _ = preview.grab(&document, 200, 150, 1.0);
        assert!(preview.insertion_point().is_none());
        assert_eq!(preview.scale_bar_metres(), 0.0);
    }
}
