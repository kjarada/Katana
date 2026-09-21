// The sheet arithmetic behind a plot (include/katana/cad/plot.hpp).
//
// Every number here follows from two definitions - 25.4 mm to the inch, and
// ISO 216 paper sizes - and from the meaning of "1 : N", so the expectations
// are worked by hand and none is a value the code once produced.

#include <gtest/gtest.h>

#include "katana/cad/plot.hpp"

using namespace katana::cad;
using katana::core::ErrorCode;
using katana::geometry::Box2;
using katana::geometry::Point2;

namespace {

PlotSettings a4Landscape300()
{
    PlotSettings settings;
    settings.paper = PaperSize::A4;
    settings.landscape = true;
    settings.marginMm = 10.0;
    settings.dpi = 300.0;
    settings.scaleDenominator = 1000.0;
    settings.center = Point2(500.0, 250.0);
    return settings;
}

} // namespace

TEST(Plot, PaperSizesAreISO216)
{
    // ISO 216:2007 Table 1, and landscape merely swaps the two.
    EXPECT_EQ(paperDimensions(PaperSize::A0, false).widthMm, 841.0);
    EXPECT_EQ(paperDimensions(PaperSize::A0, false).heightMm, 1189.0);
    EXPECT_EQ(paperDimensions(PaperSize::A4, false).widthMm, 210.0);
    EXPECT_EQ(paperDimensions(PaperSize::A4, false).heightMm, 297.0);
    EXPECT_EQ(paperDimensions(PaperSize::A3, true).widthMm, 420.0);
    EXPECT_EQ(paperDimensions(PaperSize::A3, true).heightMm, 297.0);
    EXPECT_EQ(paperDimensions(PaperSize::A1, false).widthMm, 594.0);
    EXPECT_EQ(paperDimensions(PaperSize::A2, false).heightMm, 594.0);
}

TEST(Plot, AMillimetreOfPaperIsDpiOver25Point4Pixels)
{
    // 300 / 25.4 = 11.811023622... pixels per millimetre, so a 0.25 mm line
    // - the thinnest weight a pen plotter drew - is 2.9527559 device pixels.
    EXPECT_NEAR(millimetresToPixels(1.0, 300.0), 11.811023622047244, 1e-12);
    EXPECT_NEAR(millimetresToPixels(0.25, 300.0), 2.952755905511811, 1e-12);
    EXPECT_NEAR(millimetresToPixels(25.4, 72.0), 72.0, 1e-12);
}

TEST(Plot, TheSheetIsTheWholePaperAtTheScaleAsked)
{
    // A4 landscape at 300 dpi is 297 x 11.811 = 3507.874 by 2480.315 device
    // pixels. At 1 : 1000 one metre is one millimetre on the paper, so the
    // view scale is exactly the pixels per millimetre; at 1 : 500 it is
    // double that.
    const auto sheet = sheetFor(a4Landscape300());
    ASSERT_TRUE(sheet.ok()) << sheet.error().describe();
    EXPECT_NEAR(sheet->widthPixels, 3507.874015748031, 1e-9);
    EXPECT_NEAR(sheet->heightPixels, 2480.3149606299214, 1e-9);
    EXPECT_NEAR(sheet->pixelsPerMillimetre, 11.811023622047244, 1e-12);
    EXPECT_NEAR(sheet->view.scale, 11.811023622047244, 1e-12);
    EXPECT_EQ(sheet->view.center, Point2(500.0, 250.0));
    EXPECT_NEAR(sheet->view.widthPixels, sheet->widthPixels, 1e-9);
    EXPECT_NEAR(sheet->view.heightPixels, sheet->heightPixels, 1e-9);

    PlotSettings half = a4Landscape300();
    half.scaleDenominator = 500.0;
    EXPECT_NEAR(sheetFor(half)->view.scale, 23.622047244094489, 1e-12);

    // The sheet centre sees the model centre: the whole paper spans 297 mm,
    // which at 1 : 1000 is 297 m, so the visible width is 297 m about x = 500.
    const Box2 visible = sheet->view.visibleWorldBounds();
    EXPECT_NEAR(visible.max.x - visible.min.x, 297.0, 1e-9);
    EXPECT_NEAR(0.5 * (visible.min.x + visible.max.x), 500.0, 1e-9);
}

TEST(Plot, FittingPicksTheFirstStandardScaleTheDrawingFitsAt)
{
    // A4 landscape with 10 mm margins leaves 277 x 190 mm. A 500 x 200 m
    // drawing needs N >= 500 000 / 277 = 1805.05 across and 200 000 / 190 =
    // 1052.6 down, so the first standard scale that fits is 1 : 2000 - not
    // 1 : 1805, which no scale rule carries.
    const PlotSettings settings = a4Landscape300();
    const auto fit = fitScale(Box2(Point2(0, 0), Point2(500, 200)), settings);
    ASSERT_TRUE(fit.ok()) << fit.error().describe();
    EXPECT_EQ(*fit, 2000.0);

    // 1000 x 50 m needs 3610 across: 1 : 5000.
    EXPECT_EQ(*fitScale(Box2(Point2(0, 0), Point2(1000, 50)), settings), 5000.0);
    // A 10 km square: across it needs 36 101, but DOWN it needs 10 000 000 /
    // 190 = 52 632, beyond the top of the ladder, so the exact denominator is
    // returned. (A first draft checked only the width and expected 1 : 50 000;
    // the height is the binding dimension on a landscape sheet.)
    EXPECT_NEAR(*fitScale(Box2(Point2(0, 0), Point2(10000, 10000)), settings),
                10000.0 * 1000.0 / 190.0, 1e-9);
    // 9 km square: 9 000 000 / 190 = 47 368 fits 1 : 50 000, the top of the ladder.
    EXPECT_EQ(*fitScale(Box2(Point2(0, 0), Point2(9000, 9000)), settings), 50000.0);
    // Exactly 277 m across fits 1 : 1000 exactly, and "fits" means >=.
    EXPECT_EQ(*fitScale(Box2(Point2(0, 0), Point2(277, 100)), settings), 1000.0);
    // 100 km is beyond the ladder: the exact denominator, so the plot still
    // fits, even though no scale bar can name it.
    EXPECT_NEAR(*fitScale(Box2(Point2(0, 0), Point2(100000, 1000)), settings),
                100000.0 * 1000.0 / 277.0, 1e-9);
}

TEST(Plot, RefusesASheetThatCannotBePlotted)
{
    PlotSettings settings = a4Landscape300();
    settings.dpi = 0.0;
    EXPECT_EQ(sheetFor(settings).error().code, ErrorCode::InvalidArgument);

    settings = a4Landscape300();
    settings.scaleDenominator = -1000.0;
    EXPECT_EQ(sheetFor(settings).error().code, ErrorCode::InvalidArgument);

    // 105 mm margins all round on 210 mm of A4 height leave nothing.
    settings = a4Landscape300();
    settings.marginMm = 105.0;
    EXPECT_EQ(sheetFor(settings).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(fitScale(Box2(Point2(0, 0), Point2(10, 10)), settings).error().code,
              ErrorCode::InvalidArgument);

    // An empty extent has nothing to fit.
    EXPECT_EQ(fitScale(Box2(Point2(5, 5), Point2(5, 5)), a4Landscape300()).error().code,
              ErrorCode::InvalidArgument);
}
