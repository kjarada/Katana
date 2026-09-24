// StylePreview: a style, a library linestyle or a library symbol painted at a plot
// scale on paper or on the screen. The pictures are read back pixel by pixel
// and compared with what was worked out by hand from the definitions.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <memory>
#include <utility>
#include <vector>

#include <QImage>

#include "customisation/style_preview.hpp"
#include "katana/entity/style_library.hpp"

using katana::cad::Document;
using katana::entity::Stroke;
using katana::entity::StrokeOp;
using katana::qt::PreviewGround;
using katana::qt::StylePreview;

namespace {

// The fixture's 'TEST Dashed Kerb' (tests/archive12d/data/customisation/
// test_linestyles.4d), written out here: a paperstyle with a 4 mm period, a
// 1.5 mm dash, a 0.5 mm gap, a 1 mm dash and a 1 mm gap.
katana::entity::LineStyle dashedKerb()
{
    katana::entity::LineStyle kerb;
    kerb.name = "TEST Dashed Kerb";
    kerb.units = katana::entity::StyleUnits::Paper;
    kerb.length = 4.0;
    kerb.strokes = {Stroke{.op = StrokeOp::Move, .point = {0.0, 0.0}},
                    Stroke{.op = StrokeOp::Draw, .point = {1.5, 0.0}},
                    Stroke{.op = StrokeOp::Move, .point = {2.0, 0.0}},
                    Stroke{.op = StrokeOp::Draw, .point = {3.0, 0.0}},
                    Stroke{.op = StrokeOp::Move, .point = {4.0, 0.0}}};
    return kerb;
}

// A symbol drawn round its insertion point, and one drawn wholly away from
// it (a square from (1, 1) to (2, 2)).
katana::entity::LineStyle ring()
{
    katana::entity::LineStyle symbol;
    symbol.name = "TEST Ring";
    symbol.atVertices = true;
    symbol.strokes = {Stroke{.op = StrokeOp::Move, .point = {0.0, 0.0}},
                      Stroke{.op = StrokeOp::Circle, .radius = 0.5}};
    return symbol;
}

katana::entity::LineStyle offsetSquare()
{
    katana::entity::LineStyle symbol;
    symbol.name = "TEST Offset";
    symbol.atVertices = true;
    symbol.strokes = {Stroke{.op = StrokeOp::Move, .point = {1.0, 1.0}},
                      Stroke{.op = StrokeOp::Draw, .point = {2.0, 1.0}},
                      Stroke{.op = StrokeOp::Draw, .point = {2.0, 2.0}},
                      Stroke{.op = StrokeOp::Draw, .point = {1.0, 2.0}},
                      Stroke{.op = StrokeOp::Draw, .point = {1.0, 1.0}}};
    return symbol;
}

struct PreviewFixture {
    Document document;
    PreviewFixture()
    {
        katana::entity::StyleLibrary library;
        EXPECT_TRUE(library.add(dashedKerb()).ok());
        EXPECT_TRUE(library.add(ring()).ok());
        EXPECT_TRUE(library.add(offsetSquare()).ok());
        document.setStyleLibrary(std::move(library));
    }
};

// How much ink column `x` holds on paper: the sum, over rows y0-2 .. y0+2, of
// each pixel's darkness (0 white, 1 black). A 1-pixel line covering a column
// fully sums to 1 whichever rows it straddles.
double columnInk(const QImage& image, int x, int y0)
{
    double ink = 0.0;
    for (int y = y0 - 2; y <= y0 + 2; ++y) {
        ink += 1.0 - image.pixelColor(x, y).lightnessF();
    }
    return ink;
}

struct InkCount {
    int columns = 0; // columns at least half covered
    int runs = 0;    // separate runs of such columns: the dashes seen
};

InkCount countInk(const QImage& image, int from, int to, int y0)
{
    InkCount count;
    bool inRun = false;
    for (int x = from; x < to; ++x) {
        const bool inked = columnInk(image, x, y0) >= 0.5;
        count.columns += inked ? 1 : 0;
        count.runs += inked && !inRun ? 1 : 0;
        inRun = inked;
    }
    return count;
}

} // namespace

TEST(StylePreview, APaperLinestyleWithGapsIsPaintedWithItsGapsNotSolid)
{
    // Worked by hand, for a 400 x 160 pane at 1:500 on paper:
    //   drawing area 400 x (160 - 24 bar) = 400 x 136; usable 376 x 112 after
    //   12-pixel margins; the sample is w wide and w/4 high, so 376 pixels
    //   wide (4 x 112 = 448 would not fit across). Printed size: 96/25.4 =
    //   3.7795 pixels per plot mm, whatever the scale, for a paperstyle.
    //   The sample's start, model (0, 0), is at the usable area's left edge,
    //   x = 12, and its bottom edge, y = 136/2 + 376/8 = 68 + 47 = 115.
    //   Along the straight run (0.45 x 376 = 169 pixels) each 4 mm period is
    //   15.118 px: a dash over [0, 5.669], a gap to 7.559, a dash to 11.339,
    //   a gap to 15.118. Over the first 4 periods (x from 12 to 72) a column
    //   is at least half covered where its centre lies in a dash:
    //     period 0: 12-17 (6), 20-22 (3)   period 1: 27-32 (6), 35-37 (3)
    //     period 2: 42-47 (6), 50-53 (4)   period 3: 57-62 (6), 65-68 (4)
    //   = 38 columns in 8 separate dashes; solid would be 60 in 1.
    //   Some dash ends fall within 0.06 px of half a column (19.559 and
    //   53.575), so the count is allowed a column either way at each of
    //   those; the 8 dashes are exact - every gap clears a whole column.
    PreviewFixture fixture;
    katana::entity::Style style;
    style.name = "kerb";
    style.linetype = "TEST Dashed Kerb";
    // The default 0.25 mm weight is 0.945 px, painted at the 1 px minimum.
    for (const bool asStyle : {true, false}) {
        StylePreview preview(fixture.document);
        preview.resize(400, 160);
        preview.setScaleDenominator(500);
        preview.setGround(PreviewGround::Paper);
        if (asStyle) {
            preview.setStyle(style);
        } else {
            preview.setLinestyle("TEST Dashed Kerb");
        }
        const QImage image = preview.grab().toImage();
        ASSERT_EQ(image.width(), 400);
        const katana::geometry::Point2 start = preview.view().worldToScreen({0.0, 0.0});
        EXPECT_NEAR(start.x, 12.0, 1e-9);
        EXPECT_NEAR(start.y, 115.0, 1e-9);

        const InkCount count = countInk(image, 12, 72, 115);
        EXPECT_EQ(count.runs, 8) << (asStyle ? "style" : "linestyle");
        EXPECT_NEAR(count.columns, 38, 2) << (asStyle ? "style" : "linestyle");
        EXPECT_TRUE(preview.notice().isEmpty()) << preview.notice().toStdString();
    }
}

TEST(StylePreview, AWhiteStylePrintsBlackOnPaperAndStaysWhiteOnTheScreen)
{
    PreviewFixture fixture;
    katana::entity::Style style;
    style.name = "white";
    style.color = katana::entity::Color{255, 255, 255, 255};
    StylePreview preview(fixture.document);
    preview.resize(400, 160);
    preview.setStyle(style);

    // Paper: decision D7. The run is at y = 115 (worked in the test above).
    preview.setGround(PreviewGround::Paper);
    EXPECT_EQ(preview.inkColour(style.color), QColor(0, 0, 0));
    QImage image = preview.grab().toImage();
    EXPECT_EQ(image.pixelColor(2, 2), QColor(Qt::white)); // the ground
    // A black 1-pixel line covers the column fully: ink 1. White ink on
    // white paper would sum to 0.
    EXPECT_NEAR(columnInk(image, 100, 115), 1.0, 0.05);

    // Screen: the dark ground, and the white line lighter than it.
    preview.setGround(PreviewGround::Screen);
    EXPECT_EQ(preview.inkColour(style.color), QColor(255, 255, 255));
    image = preview.grab().toImage();
    const QColor ground = image.pixelColor(2, 2);
    EXPECT_LT(ground.lightness(), 64);
    int lightest = 0;
    for (int y = 113; y <= 117; ++y) {
        lightest = std::max(lightest, image.pixelColor(100, y).lightness());
    }
    EXPECT_GT(lightest, ground.lightness() + 100);
}

TEST(StylePreview, TheCrosshairSitsOnTheInsertionPointAtThePanesCentre)
{
    // Worked by hand: a 200 x 150 pane leaves a 200 x 126 drawing area above
    // the 24-pixel scale bar. The view is centred on the insertion point
    // (the model origin), so it is painted at (200/2, 126/2) = (100, 63).
    // The ring's radius 0.5 m is fitted into 176 x 102 usable pixels (12 px
    // margins): 102 / (2 x 0.5) = 102 px per metre, a 51-pixel radius, well
    // clear of the crosshair's 10-pixel arms.
    PreviewFixture fixture;
    StylePreview preview(fixture.document);
    preview.resize(200, 150);
    preview.setGround(PreviewGround::Paper);
    preview.setSymbol("TEST Ring");
    const QImage image = preview.grab().toImage();

    ASSERT_TRUE(preview.insertionPoint().has_value());
    EXPECT_NEAR(preview.insertionPoint()->x(), 100.0, 1e-9);
    EXPECT_NEAR(preview.insertionPoint()->y(), 63.0, 1e-9);
    EXPECT_NEAR(preview.view().scale, 102.0, 1e-9);
    const QColor mark = StylePreview::insertionMarkColour();
    EXPECT_EQ(image.pixelColor(100, 63), mark);
    EXPECT_EQ(image.pixelColor(106, 63), mark);
    EXPECT_EQ(image.pixelColor(94, 63), mark);
    EXPECT_EQ(image.pixelColor(100, 57), mark);
    EXPECT_EQ(image.pixelColor(100, 69), mark);
    // The arms end at 10 pixels.
    EXPECT_NE(image.pixelColor(100 + 13, 63), mark);
    EXPECT_FALSE(preview.originOutsideExtent());
}

TEST(StylePreview, ASymbolDrawnAwayFromItsInsertionPointIsFlagged)
{
    PreviewFixture fixture;
    StylePreview preview(fixture.document);
    preview.resize(200, 150);
    preview.setSymbol("TEST Offset");
    (void)preview.grab();
    // The square spans (1, 1) to (2, 2): the origin is outside it.
    EXPECT_TRUE(preview.originOutsideExtent());

    // An undefined name is drawn as its built-in fallback, and said so.
    preview.setSymbol("Grated Pit");
    (void)preview.grab();
    EXPECT_TRUE(preview.notice().contains(QStringLiteral("manhole")))
        << preview.notice().toStdString();
}

TEST(StylePreview, TheScaleBarIsARoundNumberOfGroundMetresForTheChosenScale)
{
    // Worked by hand for a 400-pixel-wide pane: the bar is the longest 1-2-5
    // length of at most a quarter of the width, 100 px, at 3.7795 px per plot
    // mm and N/1000 m per plot mm:
    //   1:100  37.795 px/m -> 100 px = 2.65 m -> 2 m
    //   1:500   7.559 px/m -> 13.2 m -> 10 m
    //   1:1000  3.780 px/m -> 26.5 m -> 20 m
    PreviewFixture fixture;
    katana::entity::Style style;
    style.name = "plain";
    StylePreview preview(fixture.document);
    preview.resize(400, 160);
    preview.setStyle(style);
    const std::vector<std::pair<int, double>> expected = {{100, 2.0}, {500, 10.0}, {1000, 20.0}};
    for (const auto& [denominator, metres] : expected) {
        preview.setScaleDenominator(denominator);
        EXPECT_DOUBLE_EQ(preview.paperScale(), denominator / 1000.0);
        (void)preview.grab();
        EXPECT_DOUBLE_EQ(preview.scaleBarMetres(), metres) << "1:" << denominator;
    }
}

TEST(StylePreview, APreviewWhoseDocumentHasGonePaintsOnlyItsGround)
{
    auto document = std::make_unique<Document>();
    StylePreview preview(*document);
    preview.resize(200, 100);
    katana::entity::Style style;
    style.name = "plain";
    preview.setStyle(style);
    document.reset();
    const QImage image = preview.grab().toImage();
    EXPECT_EQ(image.pixelColor(100, 50), QColor(Qt::white));
    EXPECT_EQ(preview.scaleBarMetres(), 0.0);
}
