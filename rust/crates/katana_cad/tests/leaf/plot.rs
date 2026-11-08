// The sheet arithmetic behind a plot (tests/cad/test_plot.cpp).
//
// Every number here follows from two definitions - 25.4 mm to the inch, and
// ISO 216 paper sizes - and from the meaning of "1 : N", so the expectations
// are worked by hand and none is a value the code once produced.

use crate::near;
use katana_cad::plot::{
    fit_scale, millimetres_to_pixels, paper_colour, paper_dimensions, sheet_for,
};
use katana_cad::{PaperSize, PlotSettings};
use katana_core::ErrorCode;
use katana_entity::Color;
use katana_geometry::{Box2, Point2};

fn a4_landscape300() -> PlotSettings {
    PlotSettings {
        paper: PaperSize::A4,
        landscape: true,
        margin_mm: 10.0,
        dpi: 300.0,
        scale_denominator: 1000.0,
        center: Point2::new(500.0, 250.0),
        ..PlotSettings::default()
    }
}

fn extent(x0: f64, y0: f64, x1: f64, y1: f64) -> Box2 {
    Box2::new(Point2::new(x0, y0), Point2::new(x1, y1))
}

fn fit(bounds: Box2, settings: &PlotSettings) -> f64 {
    fit_scale(&bounds, settings).unwrap_or_else(|error| panic!("{}", error.describe()))
}

mod plot {
    use super::*;

    #[test]
    fn paper_sizes_are_iso216() {
        // ISO 216:2007 Table 1, and landscape merely swaps the two.
        assert_eq!(paper_dimensions(PaperSize::A0, false).width_mm, 841.0);
        assert_eq!(paper_dimensions(PaperSize::A0, false).height_mm, 1189.0);
        assert_eq!(paper_dimensions(PaperSize::A4, false).width_mm, 210.0);
        assert_eq!(paper_dimensions(PaperSize::A4, false).height_mm, 297.0);
        assert_eq!(paper_dimensions(PaperSize::A3, true).width_mm, 420.0);
        assert_eq!(paper_dimensions(PaperSize::A3, true).height_mm, 297.0);
        assert_eq!(paper_dimensions(PaperSize::A1, false).width_mm, 594.0);
        assert_eq!(paper_dimensions(PaperSize::A2, false).height_mm, 594.0);
    }

    #[test]
    fn a_millimetre_of_paper_is_dpi_over25_point4_pixels() {
        // 300 / 25.4 = 11.811023622... pixels per millimetre, so a 0.25 mm
        // line - the thinnest weight a pen plotter drew - is 2.9527559 device
        // pixels.
        near(millimetres_to_pixels(1.0, 300.0), 11.811023622047244, 1e-12);
        near(millimetres_to_pixels(0.25, 300.0), 2.952755905511811, 1e-12);
        near(millimetres_to_pixels(25.4, 72.0), 72.0, 1e-12);
    }

    #[test]
    fn the_sheet_is_the_whole_paper_at_the_scale_asked() {
        // A4 landscape at 300 dpi is 297 x 11.811 = 3507.874 by 2480.315
        // device pixels. At 1 : 1000 one metre is one millimetre on the
        // paper, so the view scale is exactly the pixels per millimetre; at
        // 1 : 500 it is double that.
        let sheet =
            sheet_for(&a4_landscape300()).unwrap_or_else(|error| panic!("{}", error.describe()));
        near(sheet.width_pixels, 3507.874015748031, 1e-9);
        near(sheet.height_pixels, 2480.3149606299214, 1e-9);
        near(sheet.pixels_per_millimetre, 11.811023622047244, 1e-12);
        near(sheet.view.scale, 11.811023622047244, 1e-12);
        assert_eq!(sheet.view.center, Point2::new(500.0, 250.0));
        near(sheet.view.width_pixels, sheet.width_pixels, 1e-9);
        near(sheet.view.height_pixels, sheet.height_pixels, 1e-9);

        let half = PlotSettings {
            scale_denominator: 500.0,
            ..a4_landscape300()
        };
        near(
            sheet_for(&half).expect("a sheet at 1 : 500").view.scale,
            23.622047244094489,
            1e-12,
        );

        // The sheet centre sees the model centre: the whole paper spans
        // 297 mm, which at 1 : 1000 is 297 m, so the visible width is 297 m
        // about x = 500.
        let visible = sheet.view.visible_world_bounds();
        near(visible.max.x - visible.min.x, 297.0, 1e-9);
        near(0.5 * (visible.min.x + visible.max.x), 500.0, 1e-9);
    }

    #[test]
    fn fitting_picks_the_first_standard_scale_the_drawing_fits_at() {
        // A4 landscape with 10 mm margins leaves 277 x 190 mm. A 500 x 200 m
        // drawing needs N >= 500 000 / 277 = 1805.05 across and 200 000 / 190
        // = 1052.6 down, so the first standard scale that fits is 1 : 2000 -
        // not 1 : 1805, which no scale rule carries.
        let settings = a4_landscape300();
        assert_eq!(fit(extent(0.0, 0.0, 500.0, 200.0), &settings), 2000.0);

        // 1000 x 50 m needs 3610 across: 1 : 5000.
        assert_eq!(fit(extent(0.0, 0.0, 1000.0, 50.0), &settings), 5000.0);
        // A 10 km square: across it needs 36 101, but DOWN it needs
        // 10 000 000 / 190 = 52 632, beyond the top of the ladder, so the
        // exact denominator is returned. (A first draft checked only the
        // width and expected 1 : 50 000; the height is the binding dimension
        // on a landscape sheet.)
        near(
            fit(extent(0.0, 0.0, 10000.0, 10000.0), &settings),
            10000.0 * 1000.0 / 190.0,
            1e-9,
        );
        // 9 km square: 9 000 000 / 190 = 47 368 fits 1 : 50 000, the top of
        // the ladder.
        assert_eq!(fit(extent(0.0, 0.0, 9000.0, 9000.0), &settings), 50000.0);
        // Exactly 277 m across fits 1 : 1000 exactly, and "fits" means >=.
        assert_eq!(fit(extent(0.0, 0.0, 277.0, 100.0), &settings), 1000.0);
        // 100 km is beyond the ladder: the exact denominator, so the plot
        // still fits, even though no scale bar can name it.
        near(
            fit(extent(0.0, 0.0, 100000.0, 1000.0), &settings),
            100000.0 * 1000.0 / 277.0,
            1e-9,
        );
    }

    #[test]
    fn refuses_a_sheet_that_cannot_be_plotted() {
        let mut settings = a4_landscape300();
        settings.dpi = 0.0;
        assert_eq!(
            sheet_for(&settings).expect_err("no resolution").code,
            ErrorCode::InvalidArgument
        );

        settings = a4_landscape300();
        settings.scale_denominator = -1000.0;
        assert_eq!(
            sheet_for(&settings).expect_err("no scale").code,
            ErrorCode::InvalidArgument
        );

        // 105 mm margins all round on 210 mm of A4 height leave nothing.
        settings = a4_landscape300();
        settings.margin_mm = 105.0;
        assert_eq!(
            sheet_for(&settings).expect_err("no printable area").code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            fit_scale(&extent(0.0, 0.0, 10.0, 10.0), &settings)
                .expect_err("no printable area")
                .code,
            ErrorCode::InvalidArgument
        );

        // An empty extent has nothing to fit.
        assert_eq!(
            fit_scale(&extent(5.0, 5.0, 5.0, 5.0), &a4_landscape300())
                .expect_err("nothing to fit")
                .code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn a_line_with_no_height_fits_on_its_length_alone() {
        // Audit CAD-15. LINE 0,0 500,0 has width 500 m and height 0. On A4
        // landscape with 10 mm margins the printable area is 277 x 190 mm, so
        // the width needs 500 000 / 277 = 1805.1 and the height asks nothing:
        // the first standard scale at or above 1805.1 is 1 : 2000. It used to
        // be refused as 'no extent to fit'.
        let settings = a4_landscape300();
        assert_eq!(fit(extent(0.0, 0.0, 500.0, 0.0), &settings), 2000.0);
        // The same line running north: 500 000 / 190 = 2631.6 down, so
        // 1 : 5000.
        assert_eq!(fit(extent(7.0, 0.0, 7.0, 500.0), &settings), 5000.0);
    }

    #[test]
    fn white_and_near_white_print_black_on_paper_while_every_other_colour_keeps_its_own() {
        // Decision D7: white on white paper is invisible, and white is the
        // colour of a new layer and of 130 of a reference mapfile's 457 rules.
        // "Near" is every channel at 230 or more, so (230, 230, 230) prints
        // black and (229, 255, 255) - a tint, not a white - keeps its colour.
        // Alpha is the pen's own and is kept.
        let settings = PlotSettings::default(); // the rule is on by default
        assert!(settings.white_to_black);
        let colour = Color::new;
        assert_eq!(
            paper_colour(colour(255, 255, 255, 255), &settings),
            colour(0, 0, 0, 255)
        );
        assert_eq!(
            paper_colour(colour(230, 230, 230, 128), &settings),
            colour(0, 0, 0, 128)
        );
        assert_eq!(
            paper_colour(colour(229, 255, 255, 255), &settings),
            colour(229, 255, 255, 255)
        );
        assert_eq!(
            paper_colour(colour(255, 0, 0, 255), &settings),
            colour(255, 0, 0, 255)
        );
        assert_eq!(
            paper_colour(colour(0, 0, 0, 255), &settings),
            colour(0, 0, 0, 255)
        );
    }

    #[test]
    fn the_white_to_black_rule_can_be_turned_off_for_a_plot_that_wants_its_white() {
        let settings = PlotSettings {
            white_to_black: false,
            ..PlotSettings::default()
        };
        assert_eq!(
            paper_colour(Color::new(255, 255, 255, 255), &settings),
            Color::new(255, 255, 255, 255)
        );
    }
}
