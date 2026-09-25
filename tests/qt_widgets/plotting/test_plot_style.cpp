// The plot style on paper (src/katana_qt/plotting/plot_style and the sheet
// painter's pens, text, fills and images): a sheet painted in colour,
// greyscale and monochrome, and at a line weight scale, read back pixel by
// pixel.
//
// With KATANA_SHEET_PNG set to a directory, the whole sheets painted here are
// also written there as PNGs, to be looked at.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <optional>
#include <string>
#include <tuple>
#include <utility>
#include <vector>

#include <QColor>
#include <QImage>
#include <QPainter>

#include "katana/cad/plot.hpp"
#include "katana/cad/plotting/frame.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/entity/model.hpp"
#include "katana/entity/tables.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "plotting/plot_style.hpp"
#include "sheet_painter.hpp"

using katana::cad::PlotColourMode;
using katana::cad::PlotSettings;
using katana::entity::Color;
using katana::entity::Entity;
using katana::entity::Model;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Segment2;
using katana::qt::SheetPaintCache;
using katana::qt::SheetPaintOptions;
using katana::qt::SheetSource;
namespace plotting = katana::cad::plotting;

namespace {

constexpr double kSheetHeightMm = 297.0; // A3 landscape

Box2 box(double x0, double y0, double x1, double y1) { return Box2(Point2(x0, y0), Point2(x1, y1)); }

PlotSettings styled(PlotColourMode mode, double lineWeightScale = 1.0)
{
    PlotSettings plot;
    plot.colourMode = mode;
    plot.lineWeightScale = lineWeightScale;
    return plot;
}

void addLine(Model& model, Point2 a, Point2 b, std::optional<Color> colour = std::nullopt)
{
    Entity entity;
    entity.geometry = Segment2{a, b};
    entity.layer = "0";
    entity.color = colour;
    ASSERT_TRUE(model.entities.add(std::move(entity)).ok());
}

// An A3 sheet, the built-in frame, one plan viewport at 1 : 1000 centred on
// the origin over 100..300 x 100..250 mm of paper.
plotting::SheetSet planSheet()
{
    plotting::SheetSet set;
    plotting::Sheet sheet;
    sheet.id = "s1";
    sheet.name = "STYLE";
    plotting::Viewport plan;
    plan.id = "vp1";
    plan.kind = plotting::ViewportKind::Plan;
    plan.rect = box(100.0, 100.0, 300.0, 250.0);
    plan.scale = 1000.0;
    plan.centre = Point2(0.0, 0.0);
    sheet.viewports.push_back(plan);
    set.sheets.push_back(sheet);
    return set;
}

// Paints sheet 0 of `set` in `plot`, only the paper box `region`, at `ppmm`
// device pixels a millimetre: the painter's own origin shift, as the editor
// scrolls, so a fine resolution costs only the pixels looked at.
QImage paintedRegion(const plotting::SheetSet& set, const SheetSource& source,
                     const PlotSettings& plot, Box2 region, double ppmm)
{
    QImage image(static_cast<int>(std::ceil(region.width() * ppmm)),
                 static_cast<int>(std::ceil(region.height() * ppmm)),
                 QImage::Format_ARGB32_Premultiplied);
    image.fill(Qt::gray); // the painter's paper covers it
    SheetPaintOptions options;
    options.pixelsPerMillimetre = ppmm;
    options.origin = QPointF(-region.min.x * ppmm, -(kSheetHeightMm - region.max.y) * ppmm);
    options.plot = plot;
    SheetPaintCache cache;
    QPainter painter(&image);
    (void)katana::qt::paintSheet(painter, set, 0, source, options, cache);
    painter.end();
    return image;
}

QImage paintedSheet(const plotting::SheetSet& set, const SheetSource& source,
                    const PlotSettings& plot, const char* tag)
{
    QImage image = paintedRegion(set, source, plot, box(0.0, 0.0, 420.0, kSheetHeightMm), 4.0);
    if (const char* dir = std::getenv("KATANA_SHEET_PNG"); dir != nullptr) {
        const auto* test = ::testing::UnitTest::GetInstance()->current_test_info();
        image.save(QString("%1/%2_%3.png").arg(dir, test->name(), tag));
    }
    return image;
}

// The pixel of paper point `p` on an image painted of `region` at `ppmm`.
QPoint pixelOf(Point2 p, Box2 region, double ppmm)
{
    return QPoint(static_cast<int>(std::floor((p.x - region.min.x) * ppmm)),
                  static_cast<int>(std::floor((region.max.y - p.y) * ppmm)));
}

int chroma(QRgb pixel)
{
    const int high = std::max({qRed(pixel), qGreen(pixel), qBlue(pixel)});
    const int low = std::min({qRed(pixel), qGreen(pixel), qBlue(pixel)});
    return high - low;
}

int lightness(QRgb pixel) { return qRed(pixel) + qGreen(pixel) + qBlue(pixel); }

// The darkest pixel within `radiusMm` of paper point `p`.
QRgb darkestNear(const QImage& image, Point2 p, Box2 region, double ppmm, double radiusMm)
{
    const QPoint a = pixelOf(Point2(p.x - radiusMm, p.y + radiusMm), region, ppmm);
    const QPoint b = pixelOf(Point2(p.x + radiusMm, p.y - radiusMm), region, ppmm);
    QRgb darkest = qRgb(255, 255, 255);
    for (int y = std::max(a.y(), 0); y <= std::min(b.y(), image.height() - 1); ++y) {
        for (int x = std::max(a.x(), 0); x <= std::min(b.x(), image.width() - 1); ++x) {
            const QRgb pixel = image.pixel(x, y);
            if (lightness(pixel) < lightness(darkest)) {
                darkest = pixel;
            }
        }
    }
    return darkest;
}

// The most colourful pixel of the image, and how many have any colour.
struct Colourfulness {
    int most = 0;
    int coloured = 0;
};

Colourfulness colourfulness(const QImage& image, int threshold)
{
    Colourfulness out;
    for (int y = 0; y < image.height(); ++y) {
        const auto* row = reinterpret_cast<const QRgb*>(image.constScanLine(y));
        for (int x = 0; x < image.width(); ++x) {
            const int c = chroma(row[x]);
            out.most = std::max(out.most, c);
            out.coloured += c > threshold ? 1 : 0;
        }
    }
    return out;
}

// Dark pixels (every channel under 128) along the device row through paper
// height `y`, between paper x `x0` and `x1`: a vertical line's width.
int darkAcross(const QImage& image, double y, double x0, double x1, Box2 region, double ppmm)
{
    const int row = pixelOf(Point2(x0, y), region, ppmm).y();
    int count = 0;
    for (int x = pixelOf(Point2(x0, y), region, ppmm).x(); x <= pixelOf(Point2(x1, y), region, ppmm).x();
         ++x) {
        const QRgb pixel = image.pixel(x, row);
        count += qRed(pixel) < 128 && qGreen(pixel) < 128 && qBlue(pixel) < 128 ? 1 : 0;
    }
    return count;
}

// The same down the device column through paper x `x`: a level line's width.
int darkDown(const QImage& image, double x, double y0, double y1, Box2 region, double ppmm)
{
    const int column = pixelOf(Point2(x, y0), region, ppmm).x();
    int count = 0;
    for (int y = pixelOf(Point2(x, y1), region, ppmm).y(); y <= pixelOf(Point2(x, y0), region, ppmm).y();
         ++y) {
        const QRgb pixel = image.pixel(column, y);
        count += qRed(pixel) < 128 && qGreen(pixel) < 128 && qBlue(pixel) < 128 ? 1 : 0;
    }
    return count;
}

} // namespace

TEST(PlotStyle, InkAndFillsFollowTheColourRules)
{
    const PlotSettings colour = styled(PlotColourMode::Colour);
    const PlotSettings grey = styled(PlotColourMode::Greyscale);
    const PlotSettings mono = styled(PlotColourMode::Monochrome);
    // Colour keeps a colour; white prints black in every mode.
    EXPECT_EQ(katana::qt::plotInk(QColor(0, 147, 208), colour), QColor(0, 147, 208));
    EXPECT_EQ(katana::qt::plotInk(QColor(255, 255, 255), colour), QColor(0, 0, 0));
    // Greyscale: the luminance, alpha kept.
    EXPECT_EQ(katana::qt::plotInk(QColor(0, 0, 255, 99), grey), QColor(29, 29, 29, 99));
    EXPECT_EQ(katana::qt::plotFill(QColor(0, 0, 255, 99), grey), QColor(29, 29, 29, 99));
    // Monochrome: every pen black however light; a fill black or white by
    // its luminance against 128.
    EXPECT_EQ(katana::qt::plotInk(QColor(255, 255, 0), mono), QColor(0, 0, 0));
    EXPECT_EQ(katana::qt::plotFill(QColor(200, 0, 30, 18), mono), QColor(0, 0, 0, 18));
    EXPECT_EQ(katana::qt::plotFill(QColor(255, 255, 0), mono), QColor(255, 255, 255));
}

TEST(PlotStyle, ImagesPrintAsTheGreyOfTheirLuminanceWithTheirAlpha)
{
    QImage image(3, 1, QImage::Format_ARGB32);
    image.setPixel(0, 0, qRgba(255, 0, 0, 255));
    image.setPixel(1, 0, qRgba(0, 0, 255, 0)); // transparent ground
    image.setPixel(2, 0, qRgba(10, 200, 30, 128));
    EXPECT_EQ(katana::qt::plotImage(image, styled(PlotColourMode::Colour)), image);
    for (const PlotColourMode mode : {PlotColourMode::Greyscale, PlotColourMode::Monochrome}) {
        const QImage grey = katana::qt::plotImage(image, styled(mode));
        EXPECT_EQ(grey.pixel(0, 0), qRgba(76, 76, 76, 255));
        EXPECT_EQ(qAlpha(grey.pixel(1, 0)), 0);
        EXPECT_EQ(grey.pixel(2, 0), qRgba(124, 124, 124, 128)); // 2.99 + 117.4 + 3.42
    }
    EXPECT_TRUE(katana::qt::plotImage(QImage(), styled(PlotColourMode::Greyscale)).isNull());
}

TEST(PlotStyle, AGreyscaleRasterIsOneChannelOfTheLuminance)
{
    QImage image(2, 1, QImage::Format_RGB32);
    image.setPixel(0, 0, qRgb(255, 0, 0));
    image.setPixel(1, 0, qRgb(255, 255, 255));
    image.setDotsPerMeterX(11811);
    image.setDotsPerMeterY(11811);
    const QImage grey = katana::qt::greyscaleRaster(image);
    ASSERT_EQ(grey.format(), QImage::Format_Grayscale8);
    EXPECT_EQ(grey.constScanLine(0)[0], 76);
    EXPECT_EQ(grey.constScanLine(0)[1], 255);
    EXPECT_EQ(grey.dotsPerMeterX(), 11811);
}

TEST(PlotStyle, GreyscaleAndMonochromeLeaveNoColourAnywhereOnTheSheet)
{
    // Colour from every source the sheet painter has: the frame's coloured
    // legend lines, glyphs and texts; a plan's red and green lines; the
    // logo; a cross section's coloured ground.
    std::vector<katana::geometry::Point3> vertices{
        {0.0, 0.0, 0.0}, {100.0, 0.0, 50.0}, {100.0, 100.0, 50.0}, {0.0, 100.0, 0.0}};
    std::vector<katana::terrain::TinTriangle> triangles{{0, 1, 2}, {0, 2, 3}};
    auto ground = katana::terrain::TinSurface::create(std::move(vertices), std::move(triangles));
    ASSERT_TRUE(ground.ok());
    Model model;
    addLine(model, Point2(-80.0, 0.0), Point2(80.0, 0.0), Color{255, 0, 0, 255});
    addLine(model, Point2(0.0, -60.0), Point2(0.0, 60.0), Color{0, 160, 40, 255});
    katana::entity::Alignment road;
    road.name = "ROAD";
    road.horizontal.pis = {{Point2(50.0, 10.0)}, {Point2(50.0, 90.0)}};
    ASSERT_TRUE(model.alignments.add(road).ok());

    plotting::SheetSet set = planSheet();
    plotting::Viewport across;
    across.id = "vp2";
    across.kind = plotting::ViewportKind::CrossSections;
    across.rect = box(305.0, 100.0, 405.0, 250.0);
    across.scale = 250.0;
    across.source.alignment = "ROAD";
    across.source.stations = {40.0};
    across.source.sectionHalfWidth = 20.0;
    across.autoCentre = true;
    set.sheets[0].viewports.push_back(across);

    SheetSource source;
    source.plan.model = &model;
    source.surfaces.push_back({"GROUND", &*ground});
    source.logo = QImage(400, 100, QImage::Format_ARGB32);
    source.logo.fill(QColor(220, 0, 0));

    const QImage colour = paintedSheet(set, source, styled(PlotColourMode::Colour), "colour");
    // The colour is there to lose: thousands of strongly coloured pixels.
    EXPECT_GT(colourfulness(colour, 60).coloured, 5000);

    for (const PlotColourMode mode : {PlotColourMode::Greyscale, PlotColourMode::Monochrome}) {
        const QImage sheet = paintedSheet(set, source, styled(mode),
                                          mode == PlotColourMode::Greyscale ? "grey" : "mono");
        const Colourfulness found = colourfulness(sheet, 2);
        EXPECT_LE(found.most, 2) << katana::cad::toString(mode);
        EXPECT_EQ(found.coloured, 0) << katana::cad::toString(mode);
    }
}

TEST(PlotStyle, MonochromePrintsEveryFrameLineBlackAndGreyscaleAsItsLuminance)
{
    Model model;
    plotting::SheetSet set = planSheet();
    set.sheets[0].viewports.clear();
    SheetSource source;
    source.plan.model = &model;
    const plotting::Frame& frame = *plotting::builtInFrame();
    // The title block, where the frame's coloured legend lines are, at 10
    // pixels a millimetre: a 0.18 mm line is 1.8 pixels, so one pixel of it
    // is nearly all ink.
    const Box2 region = box(20.0, 12.0, 412.0, 36.0);
    constexpr double kPpmm = 10.0;
    const QImage colour = paintedRegion(set, source, styled(PlotColourMode::Colour), region, kPpmm);
    const QImage grey = paintedRegion(set, source, styled(PlotColourMode::Greyscale), region, kPpmm);
    const QImage mono = paintedRegion(set, source, styled(PlotColourMode::Monochrome), region, kPpmm);

    int coloured = 0;
    int checked = 0;
    for (const plotting::FramePolyline& line : frame.polylines) {
        // Solid lines of 0.15 mm and more: a dashed one's middle may fall in
        // a gap, and the 0.13 mm legend grid is under a pixel and a half.
        if (!line.plots || line.points.size() < 2 || line.weightMm < 0.15 || line.dashMm > 0.0) {
            continue;
        }
        const Point2 middle((line.points[0].x + line.points[1].x) / 2.0,
                            (line.points[0].y + line.points[1].y) / 2.0);
        if (!region.contains(middle)) {
            continue;
        }
        ++checked;
        // Monochrome: black, whatever the line's colour.
        const QRgb black = darkestNear(mono, middle, region, kPpmm, 0.3);
        EXPECT_LE(std::max({qRed(black), qGreen(black), qBlue(black)}), 40)
            << "a line at " << middle.x << ", " << middle.y;
        EXPECT_LE(chroma(black), 2);
        const Color& ink = line.colour;
        if (ink.r == ink.g && ink.g == ink.b) {
            continue;
        }
        ++coloured;
        // Colour: the line's own colour.
        const QRgb own = darkestNear(colour, middle, region, kPpmm, 0.3);
        EXPECT_GT(chroma(own), 60) << "a line at " << middle.x << ", " << middle.y;
        // Greyscale: a grey as light as the colour (its luminance, lightened
        // a little where the line does not cover a whole pixel).
        const QRgb shade = darkestNear(grey, middle, region, kPpmm, 0.3);
        const int expected = katana::cad::luminance(ink);
        EXPECT_LE(chroma(shade), 2);
        EXPECT_GE(qRed(shade), expected - 3) << "a line at " << middle.x << ", " << middle.y;
        EXPECT_LE(qRed(shade), expected + 40) << "a line at " << middle.x << ", " << middle.y;
    }
    // The legend's blue, gold, yellow, orange and brown lines, and the rules.
    EXPECT_GE(coloured, 12);
    EXPECT_GE(checked, 20);
}

TEST(PlotStyle, APlanOnASheetFollowsTheStyle)
{
    // A red line across the plan at y 175 on the paper.
    Model model;
    addLine(model, Point2(-80.0, 0.0), Point2(80.0, 0.0), Color{255, 0, 0, 255});
    const plotting::SheetSet set = planSheet();
    SheetSource source;
    source.plan.model = &model;
    const Box2 region = box(190.0, 170.0, 210.0, 180.0);
    constexpr double kPpmm = 10.0;
    const Point2 onLine(200.0, 175.0);

    const QRgb red = darkestNear(
        paintedRegion(set, source, styled(PlotColourMode::Colour), region, kPpmm), onLine, region, kPpmm, 0.5);
    EXPECT_GT(qRed(red), 200);
    EXPECT_LT(qGreen(red), 60);

    const QRgb grey = darkestNear(
        paintedRegion(set, source, styled(PlotColourMode::Greyscale), region, kPpmm), onLine, region, kPpmm, 0.5);
    EXPECT_LE(chroma(grey), 2);
    EXPECT_NEAR(qRed(grey), 76, 12); // red's luminance

    const QRgb black = darkestNear(
        paintedRegion(set, source, styled(PlotColourMode::Monochrome), region, kPpmm), onLine, region, kPpmm, 0.5);
    EXPECT_LE(std::max({qRed(black), qGreen(black), qBlue(black)}), 20);
}

TEST(PlotStyle, APlansSolidFillIsGreyedOnceAndInMonochromeALightOneDropsOut)
{
    // Two 40 m squares hatched solid, a light yellow one left of the centre
    // and a dark blue one right of it: at 1 : 1000 their middles are paper
    // (160, 175) and (240, 175).
    Model model;
    katana::entity::HatchPattern solid;
    solid.name = "fill";
    solid.solid = true;
    ASSERT_TRUE(model.hatchPatterns.add(solid));
    const Color light{255, 230, 0, 255}; // luminance 211
    const Color dark{0, 0, 200, 255};    // luminance 23
    for (const auto& [name, colour, x0] :
         {std::tuple{"light", light, -60.0}, std::tuple{"dark", dark, 20.0}}) {
        katana::entity::Layer layer;
        layer.name = name;
        layer.color = colour;
        layer.hatchPattern = "fill";
        ASSERT_TRUE(model.layers.add(layer));
        Entity square;
        square.geometry = katana::geometry::Polyline2{
            {Point2(x0, -20.0), Point2(x0 + 40.0, -20.0), Point2(x0 + 40.0, 20.0), Point2(x0, 20.0)},
            true};
        square.layer = name;
        ASSERT_TRUE(model.entities.add(std::move(square)).ok());
    }
    const plotting::SheetSet set = planSheet();
    SheetSource source;
    source.plan.model = &model;
    const Box2 region = box(130.0, 150.0, 270.0, 200.0);
    constexpr double kPpmm = 10.0; // the outline's 0.25 mm is 2.5 pixels
    const auto middleOf = [&](const QImage& image, double x) {
        return image.pixel(pixelOf(Point2(x, 175.0), region, kPpmm));
    };

    // Greyscale: each fill the grey of its own luminance - not darkened by a
    // second pass.
    const QImage grey = paintedRegion(set, source, styled(PlotColourMode::Greyscale), region, kPpmm);
    EXPECT_EQ(middleOf(grey, 160.0), qRgb(211, 211, 211));
    EXPECT_EQ(middleOf(grey, 240.0), qRgb(23, 23, 23));

    // Monochrome: the light fill drops out to the paper's white, leaving its
    // black outline; the dark one prints solid black.
    const QImage mono = paintedRegion(set, source, styled(PlotColourMode::Monochrome), region, kPpmm);
    EXPECT_EQ(middleOf(mono, 160.0), qRgb(255, 255, 255));
    EXPECT_EQ(middleOf(mono, 240.0), qRgb(0, 0, 0));
    const QRgb edge = darkestNear(mono, Point2(140.0, 175.0), region, kPpmm, 0.5);
    EXPECT_LE(std::max({qRed(edge), qGreen(edge), qBlue(edge)}), 40);
}

TEST(PlotStyle, TheLogoPrintsGreyInGreyscaleAndMonochrome)
{
    Model model;
    plotting::SheetSet set = planSheet();
    SheetSource source;
    source.plan.model = &model;
    source.logo = QImage(400, 100, QImage::Format_ARGB32);
    source.logo.fill(QColor(220, 0, 0));
    const Point2 middle = plotting::builtInFrame()->cell("logo")->rect.center();
    const Box2 region = box(middle.x - 5.0, middle.y - 2.0, middle.x + 5.0, middle.y + 2.0);
    for (const PlotColourMode mode : {PlotColourMode::Greyscale, PlotColourMode::Monochrome}) {
        const QImage image = paintedRegion(set, source, styled(mode), region, 4.0);
        const QRgb pixel = image.pixel(image.width() / 2, image.height() / 2);
        // 220 red is a luminance of 66: grey, not thresholded to black.
        EXPECT_NEAR(qRed(pixel), 66, 2) << katana::cad::toString(mode);
        EXPECT_LE(chroma(pixel), 1);
    }
}

TEST(PlotStyle, TheLineWeightScaleThickensTheFrameAndThePlan)
{
    Model model;
    addLine(model, Point2(-80.0, 0.0), Point2(80.0, 0.0)); // layer 0: 0.25 mm
    const plotting::SheetSet set = planSheet();
    SheetSource source;
    source.plan.model = &model;
    constexpr double kPpmm = 10.0;
    // The border's left rule (0.53 mm at x 21.825) and the plan's line (at y
    // 175 on the paper).
    const Box2 border = box(18.0, 150.0, 26.0, 152.0);
    const Box2 plan = box(195.0, 170.0, 205.0, 180.0);
    const auto widths = [&](double scale) {
        const PlotSettings plot = styled(PlotColourMode::Colour, scale);
        const QImage left = paintedRegion(set, source, plot, border, kPpmm);
        const QImage line = paintedRegion(set, source, plot, plan, kPpmm);
        return std::pair{darkAcross(left, 151.0, 18.5, 25.5, border, kPpmm),
                         darkDown(line, 200.0, 170.5, 179.5, plan, kPpmm)};
    };
    const auto [rule, line] = widths(1.0);
    EXPECT_NEAR(rule, 5, 1);  // 0.53 mm
    EXPECT_NEAR(line, 2.5, 1); // 0.25 mm
    const auto [ruleBold, lineBold] = widths(2.0);
    EXPECT_NEAR(ruleBold, 11, 1);
    EXPECT_NEAR(lineBold, 5, 1);
    const auto [ruleFine, lineFine] = widths(0.5);
    EXPECT_NEAR(ruleFine, 2.5, 1);
    EXPECT_LE(lineFine, 2);
    EXPECT_GE(lineFine, 1);
}
