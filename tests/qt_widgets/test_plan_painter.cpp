// The plan painter (src/katana_qt/plan_painter): what it draws through a
// rotated frame, on paper and on screen, and what its screen-speed measures
// leave unchanged. Painted into QImages and read back pixel by pixel, on the
// offscreen platform the widget tests run on.

#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <functional>
#include <limits>
#include <string>
#include <thread>
#include <vector>

#include <QFile>
#include <QImage>
#include <QMouseEvent>
#include <QTemporaryDir>
#include <QWheelEvent>
#include <QPainter>

#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/plot.hpp"
#include "katana/cad/selection.hpp"
#include "katana/cad/view_set.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/mesh.hpp"
#include "katana/render/framebuffer.hpp"
#include "plan_painter.hpp"
#include "widget_harness.hpp"

using katana::cad::PlotSettings;
using katana::entity::Entity;
using katana::entity::Model;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::interop::RasterOverlay;
using katana::interop::ReferenceData;
using katana::qt::PlanFrame;
using katana::qt::PlanMedium;
using katana::qt::PlanPaintCache;
using katana::qt::PlanPaintOptions;
using katana::qt::PlanPaintStats;
using katana::qt::PlanSource;
using katana::qt::paintPlan;

namespace {

const QRgb kBlack = qRgb(0, 0, 0);
const QRgb kWhite = qRgb(255, 255, 255);

Entity entityOf(katana::entity::Geometry geometry, std::string layer = "0")
{
    Entity entity;
    entity.geometry = std::move(geometry);
    entity.layer = std::move(layer);
    return entity;
}

void add(Model& model, Entity entity)
{
    ASSERT_TRUE(model.entities.add(std::move(entity)).ok());
}

// A frame `width` x `height` pixels at `scale` pixels a unit about `centre`.
PlanFrame frameOf(double width, double height, double scale, Point2 centre, double rotation = 0.0)
{
    PlanFrame frame;
    frame.transform.resize(width, height);
    frame.transform.scale = scale;
    frame.transform.center = centre;
    frame.rotation = rotation;
    return frame;
}

// Paints `source` through `frame` onto a `fill`ed image of the frame's size.
QImage paintedSource(const PlanSource& source, const PlanFrame& frame,
                     const PlanPaintOptions& options, QRgb fill = kBlack,
                     PlanPaintStats* stats = nullptr, PlanPaintCache* cache = nullptr)
{
    QImage image(static_cast<int>(frame.transform.widthPixels),
                 static_cast<int>(frame.transform.heightPixels), QImage::Format_ARGB32_Premultiplied);
    image.fill(fill);
    PlanPaintCache local;
    QPainter painter(&image);
    const PlanPaintStats result =
        paintPlan(painter, source, frame, options, cache != nullptr ? *cache : local);
    painter.end();
    if (stats != nullptr) {
        *stats = result;
    }
    return image;
}

// Paints `model` through `frame` onto a `fill`ed image of the frame's size.
QImage painted(const Model& model, const PlanFrame& frame, const PlanPaintOptions& options,
               QRgb fill = kBlack, PlanPaintStats* stats = nullptr, PlanPaintCache* cache = nullptr)
{
    PlanSource source;
    source.model = &model;
    return paintedSource(source, frame, options, fill, stats, cache);
}

// A `width` x `height` raster whose pixel (x, y) is `colour(x, y)`, placed by
// `geotransform`.
RasterOverlay rasterOf(int width, int height, const std::array<double, 6>& geotransform,
                       const std::function<QRgb(int, int)>& colour)
{
    RasterOverlay raster;
    raster.name = "ortho";
    raster.width = width;
    raster.height = height;
    raster.rgba.resize(static_cast<std::size_t>(width) * static_cast<std::size_t>(height) * 4);
    std::size_t at = 0;
    for (int y = 0; y < height; ++y) {
        for (int x = 0; x < width; ++x) {
            const QRgb c = colour(x, y);
            raster.rgba[at++] = static_cast<std::uint8_t>(qRed(c));
            raster.rgba[at++] = static_cast<std::uint8_t>(qGreen(c));
            raster.rgba[at++] = static_cast<std::uint8_t>(qBlue(c));
            raster.rgba[at++] = static_cast<std::uint8_t>(qAlpha(c));
        }
    }
    raster.geotransform = geotransform;
    raster.hasGeotransform = true;
    return raster;
}

// The geotransform of pixels `size` model units square, the image turned
// `angle` counter-clockwise about its top-left corner at `origin`: a step
// along a row moves size (cos, sin), a step down a column size (sin, -cos).
std::array<double, 6> turnedGeotransform(Point2 origin, double size, double angle)
{
    const double c = std::cos(angle);
    const double s = std::sin(angle);
    return {origin.x, size * c, size * s, origin.y, size * s, -size * c};
}

// Where the red channel of row `y` first falls through half, going right, in
// pixel-centre units interpolated between the two pixels either side; NaN
// when it does not.
double redFallAlongRow(const QImage& image, int y)
{
    for (int x = 0; x + 1 < image.width(); ++x) {
        const double a = qRed(image.pixel(x, y));
        const double b = qRed(image.pixel(x + 1, y));
        if (a >= 127.5 && b < 127.5) {
            return x + (a - 127.5) / (a - b);
        }
    }
    return std::numeric_limits<double>::quiet_NaN();
}

// The same for the green channel down column `x`.
double greenFallDownColumn(const QImage& image, int x)
{
    for (int y = 0; y + 1 < image.height(); ++y) {
        const double a = qGreen(image.pixel(x, y));
        const double b = qGreen(image.pixel(x, y + 1));
        if (a >= 127.5 && b < 127.5) {
            return y + (a - 127.5) / (a - b);
        }
    }
    return std::numeric_limits<double>::quiet_NaN();
}

// Whether `a` and `b` are within `tolerance` on every channel.
bool near(QRgb a, QRgb b, int tolerance = 2)
{
    return std::abs(qRed(a) - qRed(b)) <= tolerance && std::abs(qGreen(a) - qGreen(b)) <= tolerance &&
           std::abs(qBlue(a) - qBlue(b)) <= tolerance;
}

// Whether any pixel within `radius` of (x, y) is not `background`.
bool inkNear(const QImage& image, int x, int y, QRgb background, int radius = 1)
{
    for (int dy = -radius; dy <= radius; ++dy) {
        for (int dx = -radius; dx <= radius; ++dx) {
            const int px = x + dx;
            const int py = y + dy;
            if (px >= 0 && py >= 0 && px < image.width() && py < image.height() &&
                image.pixel(px, py) != background) {
                return true;
            }
        }
    }
    return false;
}

// How many pixels of row `y` are not `background`.
int inkInRow(const QImage& image, int y, QRgb background)
{
    int count = 0;
    for (int x = 0; x < image.width(); ++x) {
        count += image.pixel(x, y) != background ? 1 : 0;
    }
    return count;
}

// The whole image's darkness on white paper: the sum of 255 - grey.
long long ink(const QImage& image)
{
    long long total = 0;
    for (int y = 0; y < image.height(); ++y) {
        for (int x = 0; x < image.width(); ++x) {
            total += 255 - qGray(image.pixel(x, y));
        }
    }
    return total;
}

// How many separate runs of ink lie along rows `y - 1` to `y + 1`, read
// column by column: a column is ink when any of the three rows is.
int runsAlong(const QImage& image, int y, QRgb background)
{
    int runs = 0;
    bool inRun = false;
    for (int x = 0; x < image.width(); ++x) {
        bool inked = false;
        for (int row = y - 1; row <= y + 1; ++row) {
            inked = inked || image.pixel(x, row) != background;
        }
        runs += inked && !inRun ? 1 : 0;
        inRun = inked;
    }
    return runs;
}

// How many rows from `y` downwards in column `x` are ink without a break.
int inkRowsDownFrom(const QImage& image, int x, int y, QRgb background)
{
    int rows = 0;
    while (y + rows < image.height() && image.pixel(x, y + rows) != background) {
        ++rows;
    }
    return rows;
}

PlanPaintOptions paperOptions(const PlotSettings& settings, double pixelsPerMillimetre)
{
    PlanPaintOptions options;
    options.medium = PlanMedium::Paper;
    options.pixelsPerMillimetre = pixelsPerMillimetre;
    options.plot = &settings;
    return options;
}

} // namespace

// ---- rotated frames ---------------------------------------------------------------------

TEST(PlanPainter, ARotatedFrameCullsByTheBoxAroundTheTurnedRectangle)
{
    // A 200 x 100 frame at 1 px a unit about the origin, turned 30 degrees.
    // The turned rectangle's corners reach, about the centre,
    //   across: 100 cos 30 + 50 sin 30 = 86.603 + 25 = 111.603
    //   up:     100 sin 30 + 50 cos 30 = 50 + 43.301 = 93.301
    // so the box it shows is (-111.603, -93.301)-(111.603, 93.301), where the
    // unturned frame shows (-100, -50)-(100, 50).
    const double turn = katana::math::kPi / 6.0;
    const Box2 box = katana::qt::visibleBox(frameOf(200.0, 100.0, 1.0, Point2(0.0, 0.0), turn));
    const double across = 100.0 * std::cos(turn) + 50.0 * std::sin(turn);
    const double up = 100.0 * std::sin(turn) + 50.0 * std::cos(turn);
    EXPECT_NEAR(across, 111.603, 1e-3);
    EXPECT_NEAR(up, 93.301, 1e-3);
    EXPECT_NEAR(box.min.x, -across, 1e-9);
    EXPECT_NEAR(box.max.x, across, 1e-9);
    EXPECT_NEAR(box.min.y, -up, 1e-9);
    EXPECT_NEAR(box.max.y, up, 1e-9);
}

TEST(PlanPainter, AnEntityInTheCornerOfATurnedFrameIsDrawnThereAndNotCulled)
{
    // The same 200 x 100 frame turned a quarter turn counter-clockwise. The
    // drawing's north (model +y, up the screen) now points left, and its east
    // points up: a model offset (e, n) from the centre lands at the device
    // offset (-n, -e), y down. The segment (45, 95)-(48, 95) therefore lands
    // at x = 100 - 95 = 5, from y = 50 - 45 = 5 up to 50 - 48 = 2: in the
    // top-left corner. Unturned, y = 95 is far above the frame's y <= 50 and
    // the segment is culled.
    Model model;
    add(model, entityOf(Segment2{Point2(45.0, 95.0), Point2(48.0, 95.0)}));
    PlanPaintOptions options;
    PlanPaintStats turned;
    const QImage image = painted(
        model, frameOf(200.0, 100.0, 1.0, Point2(0.0, 0.0), katana::math::kPi / 2.0), options,
        kBlack, &turned);
    EXPECT_EQ(turned.entitiesDrawn, 1u);
    EXPECT_TRUE(inkNear(image, 5, 3, kBlack));
    // Nowhere near where the unturned frame would have put it (off the top).
    EXPECT_FALSE(inkNear(image, 145, 3, kBlack, 3));

    PlanPaintStats straight;
    (void)painted(model, frameOf(200.0, 100.0, 1.0, Point2(0.0, 0.0)), options, kBlack, &straight);
    EXPECT_EQ(straight.entitiesDrawn, 0u);
}

TEST(PlanPainter, AnEntityAtTheCornerOfAFrameTurnedThirtyDegreesIsDrawnAtThatCorner)
{
    // Device pixel (8, 8)'s centre, (8.5, 8.5), is the offset (-91.5, -41.5)
    // from the centre of the 200 x 100 frame, y down: (-91.5, 41.5) y up. The
    // drawing is turned 30 degrees counter-clockwise, so the model offset
    // there is that turned back 30 degrees:
    //   e = cos30 (-91.5) + sin30 (41.5) = -79.241 + 20.750 = -58.491
    //   n = -sin30 (-91.5) + cos30 (41.5) = 45.750 + 35.940 = 81.690
    // n = 81.69 is above the unturned frame's y <= 50, so only the turned
    // box keeps it. A 2-unit segment centred there is drawn through (8, 8).
    const double c = std::cos(katana::math::kPi / 6.0);
    const double s = std::sin(katana::math::kPi / 6.0);
    const Point2 at(c * -91.5 + s * 41.5, -s * -91.5 + c * 41.5);
    ASSERT_NEAR(at.x, -58.491, 1e-3);
    ASSERT_NEAR(at.y, 81.690, 1e-3);
    Model model;
    add(model, entityOf(Segment2{Point2(at.x - 1.0, at.y), Point2(at.x + 1.0, at.y)}));
    PlanPaintStats stats;
    const QImage image = painted(
        model, frameOf(200.0, 100.0, 1.0, Point2(0.0, 0.0), katana::math::kPi / 6.0),
        PlanPaintOptions{}, kBlack, &stats);
    EXPECT_EQ(stats.entitiesDrawn, 1u);
    EXPECT_TRUE(inkNear(image, 8, 8, kBlack));
}

// ---- paper -----------------------------------------------------------------------------

TEST(PlanPainter, APlotsFrameIsThePrintableAreaClippedAndCentredOnThePaper)
{
    // A4 landscape at 254 dpi is 10 device pixels a millimetre: the paper is
    // 2970 x 2100, the 10 mm margins 100 px, the printable area 2770 x 1900.
    PlotSettings settings;
    settings.paper = katana::cad::PaperSize::A4;
    settings.landscape = true;
    settings.dpi = 254.0;
    settings.marginMm = 10.0;
    settings.scaleDenominator = 1000.0;
    settings.center = Point2(500.0, 300.0);
    const auto frame = katana::qt::sheetFrame(settings);
    ASSERT_TRUE(frame.ok()) << frame.error().describe();
    EXPECT_TRUE(frame->clip);
    EXPECT_NEAR(frame->origin.x(), 100.0, 1e-9);
    EXPECT_NEAR(frame->origin.y(), 100.0, 1e-9);
    EXPECT_NEAR(frame->transform.widthPixels, 2770.0, 1e-9);
    EXPECT_NEAR(frame->transform.heightPixels, 1900.0, 1e-9);
    // The sheet's centre point is the paper's centre, (1485, 1050).
    const Point2 centre = frame->transform.worldToScreen(settings.center);
    EXPECT_NEAR(centre.x + frame->origin.x(), 1485.0, 1e-9);
    EXPECT_NEAR(centre.y + frame->origin.y(), 1050.0, 1e-9);
}

TEST(PlanPainter, AtAFixedScaleTheDrawingStopsAtTheMarginOfThePaper)
{
    // A4 landscape at 101.6 dpi, 4 px a millimetre: 1188 x 840 px of paper,
    // 40 px margins. At 1 : 1000 a metre is a millimetre, 4 px. The line
    // from x = -500 to 500 m through the centre is 4000 px long, far wider
    // than the paper: it must stop 40 px from each edge.
    PlotSettings settings;
    settings.paper = katana::cad::PaperSize::A4;
    settings.landscape = true;
    settings.dpi = 101.6;
    settings.marginMm = 10.0;
    settings.scaleDenominator = 1000.0;
    settings.center = Point2(0.0, 0.0);
    Model model;
    add(model, entityOf(Segment2{Point2(-500.0, 0.0), Point2(500.0, 0.0)}));
    const auto frame = katana::qt::sheetFrame(settings);
    ASSERT_TRUE(frame.ok());
    QImage paper(1188, 840, QImage::Format_ARGB32_Premultiplied);
    paper.fill(kWhite);
    PlanSource source;
    source.model = &model;
    PlanPaintCache cache;
    QPainter painter(&paper);
    (void)paintPlan(painter, source, *frame, paperOptions(settings, 4.0), cache);
    painter.end();
    EXPECT_TRUE(inkNear(paper, 594, 420, kWhite)); // the middle of the sheet
    EXPECT_TRUE(inkNear(paper, 45, 420, kWhite));  // just inside the margin
    EXPECT_FALSE(inkNear(paper, 20, 420, kWhite, 2));   // in the left margin
    EXPECT_FALSE(inkNear(paper, 1168, 420, kWhite, 2)); // in the right margin
}

TEST(PlanPainter, APointCrossOnPaperIsTwoMillimetresAcrossAtEveryResolution)
{
    // A plain point's cross is kPointMarkerPaperMillimetres = 1 mm either
    // side on paper: 8 px across at 4 px/mm and 16 px at 8 px/mm (plus up to
    // a pixel of antialiasing at each end). It was 4 px either side whatever
    // the resolution, 8 px at both.
    PlotSettings settings;
    Model model;
    add(model, entityOf(katana::entity::PointGeometry{Point2(0.0, 0.0)}));
    const auto crossWidth = [&](double pixelsPerMillimetre) {
        const QImage image = painted(model, frameOf(100.0, 100.0, 1.0, Point2(0.0, 0.0)),
                                     paperOptions(settings, pixelsPerMillimetre), kWhite);
        return inkInRow(image, 50, kWhite);
    };
    const int coarse = crossWidth(4.0);
    const int fine = crossWidth(8.0);
    EXPECT_GE(coarse, 8);
    EXPECT_LE(coarse, 10);
    EXPECT_GE(fine, 16);
    EXPECT_LE(fine, 18);
}

TEST(PlanPainter, ASolidHatchIsOpaqueOnPaperAndTranslucentOnScreen)
{
    // A 40 x 40 square on a layer hatched solid, in red. On paper its middle
    // is the layer's red itself; on screen, the red at alpha 90 over black:
    // 255 * 90 / 255 = 90, so (90, 0, 0) give or take the rounding.
    Model model;
    katana::entity::HatchPattern solid;
    solid.name = "fill";
    solid.solid = true;
    ASSERT_TRUE(model.hatchPatterns.add(solid));
    katana::entity::Layer layer;
    layer.name = "lots";
    layer.color = katana::entity::Color{255, 0, 0, 255};
    layer.hatchPattern = "fill";
    ASSERT_TRUE(model.layers.add(layer));
    add(model, entityOf(Polyline2{{Point2(-20, -20), Point2(20, -20), Point2(20, 20), Point2(-20, 20)},
                                  true},
                        "lots"));
    const PlanFrame frame = frameOf(100.0, 100.0, 1.0, Point2(0.0, 0.0));
    PlotSettings settings;
    const QImage paper = painted(model, frame, paperOptions(settings, 4.0), kWhite);
    EXPECT_EQ(paper.pixel(50, 50), qRgb(255, 0, 0));
    const QImage screen = painted(model, frame, PlanPaintOptions{}, kBlack);
    EXPECT_NEAR(qRed(screen.pixel(50, 50)), 90, 1);
    EXPECT_EQ(qGreen(screen.pixel(50, 50)), 0);
}

TEST(PlanPainter, HatchLinesOnPaperAreThirteenHundredthsOfAMillimetreWideAtEveryResolution)
{
    // An 80 x 80 square about the origin, hatched with horizontal lines 20
    // units apart, in black on white paper at 1 px a unit. Down the middle
    // column, rows 20 to 79 are exactly three spacings, so they hold exactly
    // three lines' worth of ink wherever the family's offset puts them (a
    // line the window cuts is made up by its neighbour's missing share), and
    // well clear of the boundary at rows 10 and 90 and its 0.25 mm pen. The
    // column's darkness over 255 is a line's width in pixels, three times:
    // kHatchLinePaperMillimetres = 0.13 mm is 0.13 * 20 = 2.6 px at 20 px/mm
    // and 5.2 px at 40 px/mm. A hairline is one device pixel at both - 0.05
    // mm on a 508 dpi plot and half that at 1016.
    Model model;
    katana::entity::HatchPattern lines;
    lines.name = "rule";
    lines.families = {katana::entity::HatchLineFamily{0.0, 20.0, 5.0}};
    ASSERT_TRUE(model.hatchPatterns.add(lines));
    katana::entity::Layer layer;
    layer.name = "ruled";
    layer.color = katana::entity::Color{0, 0, 0, 255};
    layer.hatchPattern = "rule";
    ASSERT_TRUE(model.layers.add(layer));
    add(model, entityOf(Polyline2{{Point2(-40, -40), Point2(40, -40), Point2(40, 40), Point2(-40, 40)},
                                  true},
                        "ruled"));
    PlotSettings settings;
    const auto lineWidth = [&](double pixelsPerMillimetre) {
        const QImage image = painted(model, frameOf(100.0, 100.0, 1.0, Point2(0.0, 0.0)),
                                     paperOptions(settings, pixelsPerMillimetre), kWhite);
        double darkness = 0.0;
        for (int y = 20; y < 80; ++y) {
            darkness += (255.0 - qGray(image.pixel(50, y))) / 255.0;
        }
        return darkness / 3.0;
    };
    EXPECT_NEAR(lineWidth(20.0), 2.6, 0.3);
    EXPECT_NEAR(lineWidth(40.0), 5.2, 0.3);
}

TEST(PlanPainter, TheAlignmentOverlayOnPaperIsMillimetresOfPaperAtEveryResolution)
{
    // A straight alignment from (-60, 0) to (60, 0) plotted twice: at 4 px/mm
    // through a frame of 1 px a unit, and at 8 px/mm through one of 2 px a
    // unit - the same sheet at twice the resolution. With k = 1 and 2 the
    // centreline lies along row 50k and the key stations are the ends,
    // 0+000 at column 20k and 0+120 at 140k.
    //
    // The end tick is kAlignmentTickPaperMillimetres = 1.5 mm either side of
    // the centreline: 6 px below it at 4 px/mm and 12 px at 8 px/mm, plus
    // half the 0.25 mm tick pen's square cap (0.5 and 1 px) and a pixel of
    // antialiasing - rows 50k down to 56 or 57 at k = 1, and to 112 or 113
    // at k = 2. Nothing else is below the line there: the station's label is
    // above it, and the alignment's name is at the start. A tick of 6 screen
    // pixels is the same 7 rows at both.
    //
    // Everything else scales the same way - the 0.5 mm centreline, the
    // 2.5 mm labels and the gap between them - so at twice the resolution
    // the overlay is the same picture twice the size: four times the ink,
    // within an eighth for the antialiased edges and the glyphs' rasterising
    // at two sizes. Sized in screen pixels, only the centreline's length
    // doubled: about twice the ink.
    Model model;
    katana::entity::Alignment road;
    road.name = "A";
    road.horizontal.pis = {katana::geometry::AlignmentPI{Point2(-60.0, 0.0)},
                           katana::geometry::AlignmentPI{Point2(60.0, 0.0)}};
    ASSERT_TRUE(model.alignments.add(road));
    PlotSettings settings;
    const auto sheet = [&](double k) {
        return painted(model, frameOf(160.0 * k, 100.0 * k, k, Point2(0.0, 0.0)),
                       paperOptions(settings, 4.0 * k), kWhite);
    };
    const QImage coarse = sheet(1.0);
    const QImage fine = sheet(2.0);
    const int coarseTick = inkRowsDownFrom(coarse, 140, 50, kWhite);
    const int fineTick = inkRowsDownFrom(fine, 280, 100, kWhite);
    EXPECT_GE(coarseTick, 6);
    EXPECT_LE(coarseTick, 8);
    EXPECT_GE(fineTick, 12);
    EXPECT_LE(fineTick, 14);
    const double ratio = static_cast<double>(ink(fine)) / static_cast<double>(ink(coarse));
    EXPECT_GT(ratio, 3.5);
    EXPECT_LT(ratio, 4.5);
}

TEST(PlanPainter, TextOnPaperIsSetAtItsFractionalHeightFromOneCachedFont)
{
    // At 1 px a unit, a text 30.0 units tall is 30.0 px and one 30.9 tall is
    // 30.9 px. A whole-pixel font set both at 30 px, the same ink; set at its
    // exact height the taller one's ink grows with its area, by about
    // (30.9 / 30)^2 = 1.061. Asked for more than 3% so the test is not about
    // the font's rounding. Every size is one font on paper.
    PlotSettings settings;
    PlanPaintCache cache;
    const auto inkOf = [&](double height) {
        Model model;
        add(model, entityOf(katana::entity::TextGeometry{Point2(-90.0, -10.0), "HEIGHT", height, 0.0}));
        return ink(painted(model, frameOf(200.0, 100.0, 1.0, Point2(0.0, 0.0)),
                           paperOptions(settings, 4.0), kWhite, nullptr, &cache));
    };
    const long long whole = inkOf(30.0);
    const long long fractional = inkOf(30.9);
    ASSERT_GT(whole, 0);
    EXPECT_GT(static_cast<double>(fractional), 1.03 * static_cast<double>(whole));
    EXPECT_EQ(cache.fontCount(), 1u);
}

TEST(PlanPainter, APdfIsGivenTheNearestWholeResolutionAndTheScaleToTheExactOne)
{
    // Audit QT-27. 300.9 dpi: the writer at 301, the painter scaled by
    // 301 / 300.9 so the sheet's pixels of 1/300.9 inch land exactly.
    const auto odd = katana::qt::pdfResolutionFor(300.9);
    EXPECT_EQ(odd.resolution, 301);
    EXPECT_DOUBLE_EQ(odd.scale, 301.0 / 300.9);
    const auto even = katana::qt::pdfResolutionFor(300.0);
    EXPECT_EQ(even.resolution, 300);
    EXPECT_EQ(even.scale, 1.0);
    // Never a resolution of nothing.
    EXPECT_EQ(katana::qt::pdfResolutionFor(0.4).resolution, 1);
}

// ---- imagery on paper ------------------------------------------------------------------

TEST(PlanPainter, ARasterOnPaperLandsWhereTheScreenDrawsTheWholeImage)
{
    // A 256 x 256 image of 0.25 m pixels, red in its left half and green in
    // its top half: an edge through its middle in each channel. Through a
    // 240 x 160 frame at 6.4 px a metre an image pixel is 1.6 device pixels.
    // On paper at 8 px/mm with a cap of 63.5 dpi (2.5 px/mm, 0.3125 of the
    // device's) it is half a capped pixel, so the crop is averaged down by
    // two; with no cap it is embedded as it is. Either way the red edge must
    // cross each row, and the green edge each column, where the screen -
    // which draws the whole image through the one composed transform - puts
    // it, to within a device pixel.
    //
    // Straight; the image turned 25 degrees by its geotransform; the frame
    // turned 0.35 rad (20 degrees); and both, 25 degrees and 0.2 rad (36.5
    // degrees in all - under 45, so the red edge still crosses every row
    // tested and the green every column). The view's centre is 1.5 m east
    // and 1 m south of the image's middle, so neither edge runs through the
    // frame's centre, about which it turns.
    PlotSettings settings;
    struct Turn {
        double image = 0.0;
        double frame = 0.0;
    };
    const double degrees25 = 25.0 * katana::math::kPi / 180.0;
    for (const Turn turn : {Turn{0.0, 0.0}, Turn{degrees25, 0.0}, Turn{0.0, 0.35},
                            Turn{degrees25, 0.2}}) {
        const auto g = turnedGeotransform(Point2(1000.0, 2000.0), 0.25, turn.image);
        ReferenceData reference;
        reference.add(rasterOf(256, 256, g, [](int x, int y) {
            return qRgb(x < 128 ? 255 : 0, y < 128 ? 255 : 0, 60);
        }));
        const Point2 middle(g[0] + 128.0 * (g[1] + g[2]), g[3] + 128.0 * (g[4] + g[5]));
        const PlanFrame frame =
            frameOf(240.0, 160.0, 6.4, Point2(middle.x + 1.5, middle.y - 1.0), turn.frame);
        PlanSource source;
        source.reference = &reference;
        PlanPaintStats screenStats;
        const QImage screen = paintedSource(source, frame, PlanPaintOptions{}, kBlack, &screenStats);
        EXPECT_EQ(screenStats.rastersDrawn, 1u);
        for (const double cap : {63.5, 0.0}) {
            PlanPaintOptions paper = paperOptions(settings, 8.0);
            paper.rasterDpiCap = cap;
            PlanPaintStats stats;
            const QImage printed = paintedSource(source, frame, paper, kBlack, &stats);
            EXPECT_EQ(stats.rastersDrawn, 1u);
            EXPECT_EQ(stats.rasterCropsMade, 1u);
            double worst = 0.0;
            for (int y = 20; y <= 140; y += 5) {
                const double expected = redFallAlongRow(screen, y);
                const double actual = redFallAlongRow(printed, y);
                ASSERT_FALSE(std::isnan(expected)) << "row " << y;
                ASSERT_FALSE(std::isnan(actual)) << "row " << y;
                worst = std::max(worst, std::abs(expected - actual));
            }
            for (int x = 60; x <= 180; x += 5) {
                const double expected = greenFallDownColumn(screen, x);
                const double actual = greenFallDownColumn(printed, x);
                ASSERT_FALSE(std::isnan(expected)) << "column " << x;
                ASSERT_FALSE(std::isnan(actual)) << "column " << x;
                worst = std::max(worst, std::abs(expected - actual));
            }
            EXPECT_LE(worst, 1.0) << "image turned " << turn.image << ", frame turned " << turn.frame
                                  << ", cap " << cap;
        }
    }
}

TEST(PlanPainter, ARasterOnPaperIsCroppedToTheViewportAndEmbeddedNoFinerThanTheCap)
{
    // A 2000 x 2000 image of 5 cm pixels, 100 m square, under a 200 x 100
    // frame at 10 px a metre about its middle - 1 : 1000 at 10 px/mm (254
    // dpi). The frame shows 20 x 10 m of it, 400 x 200 of its pixels, each
    // half a device pixel; with a pixel of margin all round the crop is
    // 402 x 202. The whole image at its own resolution would be 4 000 000
    // pixels.
    //   No dpi cap: averaged down to the device's own resolution, one pixel
    //     a device pixel: 402 / 2 x 202 / 2 = 201 x 101, 20 301.
    //   100 dpi: 3.937 px/mm, 0.3937 of the device's, so a pixel is 0.19685
    //     of a capped one: 402 x 0.19685 = 79 by 202 x 0.19685 = 40, 3 160.
    //   100 dpi and a pixel cap of 1 000: at most 1 000.
    ReferenceData reference;
    reference.add(rasterOf(2000, 2000, {0.0, 0.05, 0.0, 100.0, 0.0, -0.05},
                           [](int, int) { return qRgb(30, 140, 90); }));
    PlanSource source;
    source.reference = &reference;
    const PlanFrame frame = frameOf(200.0, 100.0, 10.0, Point2(50.0, 50.0));
    PlotSettings settings;
    const auto embedded = [&](double dpiCap, std::size_t pixelCap) {
        PlanPaintOptions paper = paperOptions(settings, 10.0);
        paper.rasterDpiCap = dpiCap;
        paper.rasterPixelCap = pixelCap;
        PlanPaintStats stats;
        const QImage printed = paintedSource(source, frame, paper, kWhite, &stats);
        EXPECT_EQ(stats.rastersDrawn, 1u);
        // The image's colour, all over the viewport, at every resolution.
        for (const QPoint at : {QPoint(2, 2), QPoint(100, 50), QPoint(197, 97)}) {
            EXPECT_TRUE(near(printed.pixel(at), qRgb(30, 140, 90)))
                << at.x() << "," << at.y() << " cap " << dpiCap << "/" << pixelCap;
        }
        return static_cast<double>(stats.rasterPixelsEmbedded);
    };
    EXPECT_NEAR(embedded(0.0, 16'000'000), 20'301.0, 400.0);
    EXPECT_NEAR(embedded(100.0, 16'000'000), 3'160.0, 150.0);
    const double capped = embedded(100.0, 1'000);
    EXPECT_LE(capped, 1'000.0);
    EXPECT_GT(capped, 900.0);
}

TEST(PlanPainter, AFineRasterIsAveragedDownOnPaperAndNotPointSampled)
{
    // A 400 x 400 one-pixel black and white checkerboard of 1 cm pixels,
    // exactly filling a 100 x 100 frame at 25 px a metre: four image pixels
    // to a device pixel each way, and the crop - its margin clamped to the
    // image - the whole image. On paper with the dpi cap off it is averaged
    // down to the device's resolution, 100 x 100, four to one, and every box
    // of 4 x 4 checks is half white: 127.5, mid grey, everywhere, and drawn
    // 1 : 1. Sampled at the nearest pixel it would print a moire of black
    // and white.
    ReferenceData reference;
    reference.add(rasterOf(400, 400, {0.0, 0.01, 0.0, 4.0, 0.0, -0.01}, [](int x, int y) {
        return (x + y) % 2 == 0 ? qRgb(255, 255, 255) : qRgb(0, 0, 0);
    }));
    PlanSource source;
    source.reference = &reference;
    PlotSettings settings;
    PlanPaintOptions paper = paperOptions(settings, 10.0);
    paper.rasterDpiCap = 0.0;
    PlanPaintStats stats;
    const QImage printed = paintedSource(source, frameOf(100.0, 100.0, 25.0, Point2(2.0, 2.0)),
                                         paper, kWhite, &stats);
    EXPECT_EQ(stats.rasterPixelsEmbedded, 100u * 100u);
    int far = 0;
    for (int y = 0; y < 100; ++y) {
        for (int x = 0; x < 100; ++x) {
            far += std::abs(qGray(printed.pixel(x, y)) - 128) > 2 ? 1 : 0;
        }
    }
    EXPECT_EQ(far, 0);
}

TEST(PlanPainter, AHiddenRasterOrOneOutsideTheViewportDoesNotPrint)
{
    ReferenceData reference;
    const katana::interop::ReferenceId id = reference.add(
        rasterOf(100, 100, {0.0, 1.0, 0.0, 100.0, 0.0, -1.0}, [](int, int) { return qRgb(220, 0, 0); }));
    PlanSource source;
    source.reference = &reference;
    PlotSettings settings;
    const PlanPaintOptions paper = paperOptions(settings, 4.0);
    PlanFrame frame = frameOf(100.0, 100.0, 2.0, Point2(50.0, 50.0));

    PlanPaintStats stats;
    EXPECT_TRUE(near(paintedSource(source, frame, paper, kWhite, &stats).pixel(50, 50), qRgb(220, 0, 0)));
    EXPECT_EQ(stats.rastersDrawn, 1u);

    // Hidden in this view only.
    const std::set<std::uint64_t> hidden{id};
    frame.hiddenReferences = &hidden;
    EXPECT_EQ(paintedSource(source, frame, paper, kWhite, &stats).pixel(50, 50), kWhite);
    EXPECT_EQ(stats.rastersDrawn, 0u);
    EXPECT_EQ(stats.rasterPixelsEmbedded, 0u);
    frame.hiddenReferences = nullptr;

    // Hidden in the Reference Data panel.
    reference.findRaster(id)->visible = false;
    EXPECT_EQ(paintedSource(source, frame, paper, kWhite, &stats).pixel(50, 50), kWhite);
    EXPECT_EQ(stats.rastersDrawn, 0u);
    reference.findRaster(id)->visible = true;

    // Shown, but a kilometre from the viewport: nothing of it is embedded.
    frame = frameOf(100.0, 100.0, 2.0, Point2(1050.0, 50.0));
    EXPECT_EQ(paintedSource(source, frame, paper, kWhite, &stats).pixel(50, 50), kWhite);
    EXPECT_EQ(stats.rastersDrawn, 0u);
    EXPECT_EQ(stats.rasterPixelsEmbedded, 0u);
}

TEST(PlanPainter, APaperCropIsResampledOnceAndTheCropsKeptAreBounded)
{
    // The sheet editor paints paper, and repaints for a pan of the editor
    // or a second viewport over the same image: the same crop at the same
    // size comes from the cache, and is drawn exactly as when it was made.
    // A crop elsewhere is made afresh; however many are made, no more than
    // kMaximumRasterCrops are kept.
    ReferenceData reference;
    reference.add(rasterOf(400, 400, {0.0, 0.25, 0.0, 100.0, 0.0, -0.25},
                           [](int x, int y) { return qRgb(x % 256, y % 256, 90); }));
    PlanSource source;
    source.reference = &reference;
    PlotSettings settings;
    PlanPaintOptions paper = paperOptions(settings, 8.0);
    paper.rasterDpiCap = 100.0;
    PlanPaintCache cache;
    const PlanFrame frame = frameOf(100.0, 80.0, 4.0, Point2(50.0, 50.0));
    PlanPaintStats stats;
    const QImage first = paintedSource(source, frame, paper, kWhite, &stats, &cache);
    EXPECT_EQ(stats.rasterCropsMade, 1u);
    const QImage again = paintedSource(source, frame, paper, kWhite, &stats, &cache);
    EXPECT_EQ(stats.rasterCropsMade, 0u);
    EXPECT_EQ(stats.rastersDrawn, 1u);
    EXPECT_TRUE(first == again);
    EXPECT_EQ(cache.rasterCropCount(), 1u);

    for (int i = 0; i < 12; ++i) {
        (void)paintedSource(source, frameOf(100.0, 80.0, 4.0, Point2(20.0 + 5.0 * i, 40.0)), paper,
                            kWhite, &stats, &cache);
        EXPECT_EQ(stats.rasterCropsMade, 1u) << i;
        EXPECT_LE(cache.rasterCropCount(), PlanPaintCache::kMaximumRasterCrops);
    }
    EXPECT_EQ(cache.rasterCropCount(), PlanPaintCache::kMaximumRasterCrops);
    cache.invalidateReferences();
    EXPECT_EQ(cache.rasterCropCount(), 0u);
}

TEST(PlanPainter, APointCloudOnPaperFillsTheTurnedViewportAtTheCappedResolution)
{
    // Red points every half metre over (-60, -60)-(60, 60), under a 200 x
    // 100 frame at 2 px a metre turned 30 degrees, on paper at 8 px/mm with
    // imagery capped at 100 dpi: 0.4921 of the device's resolution, so the
    // splat's image is 0.9843 px a metre. It covers the box around the
    // turned viewport - half-widths of 111.6 and 93.3 px, 55.8 and 46.65 m -
    // where that box meets the cloud: 111.6 x 93.3 m, which with a pixel of
    // margin either side is ceil(109.8 + 2) x ceil(91.8 + 2) = 112 x 94
    // pixels. Every corner of the device is inside the turned viewport and
    // must be red: the screen's splat, sized to the unturned view, leaves
    // those corners empty. With a pixel cap of 2 000 it is at most that.
    //
    // Uncapped, the image is no finer than kCloudPointPaperMillimetres =
    // 0.25 mm a pixel, 4 px/mm, so that a point is the same size on the page
    // at every resolution: the same sheet at 8 px/mm (a frame of 2 px a
    // metre) and at 16 (4 px a metre) splats into the same image, 1 px a
    // metre, ceil(111.6 + 2) x ceil(93.3 + 2) = 114 x 96.
    katana::interop::PointCloudLayer cloud;
    cloud.colorMode = katana::interop::PointColorMode::SourceColor;
    for (int i = 0; i <= 240; ++i) {
        for (int j = 0; j <= 240; ++j) {
            katana::pointcloud::PointCloudPoint point;
            point.x = -60.0 + 0.5 * i;
            point.y = -60.0 + 0.5 * j;
            point.red = 220;
            point.hasColor = true;
            cloud.points.push_back(point);
        }
    }
    cloud.bounds.minX = -60.0;
    cloud.bounds.minY = -60.0;
    cloud.bounds.maxX = 60.0;
    cloud.bounds.maxY = 60.0;
    cloud.bounds.minZ = 0.0;
    cloud.bounds.maxZ = 0.0;
    cloud.sourcePointCount = cloud.points.size();
    ReferenceData reference;
    reference.add(std::move(cloud));
    PlanSource source;
    source.reference = &reference;
    PlotSettings settings;
    PlanPaintOptions paper = paperOptions(settings, 8.0);
    paper.rasterDpiCap = 100.0;
    const PlanFrame frame = frameOf(200.0, 100.0, 2.0, Point2(0.0, 0.0), katana::math::kPi / 6.0);
    PlanPaintStats stats;
    const QImage printed = paintedSource(source, frame, paper, kWhite, &stats);
    EXPECT_GT(stats.cloudPointsDrawn, 0u);
    EXPECT_NEAR(static_cast<double>(stats.cloudPixelsEmbedded), 112.0 * 94.0, 112.0 * 3.0);
    for (const QPoint at : {QPoint(1, 1), QPoint(198, 1), QPoint(1, 98), QPoint(198, 98),
                            QPoint(100, 50)}) {
        EXPECT_TRUE(near(printed.pixel(at), qRgb(220, 0, 0))) << at.x() << "," << at.y();
    }

    paper.rasterPixelCap = 2'000;
    const QImage coarse = paintedSource(source, frame, paper, kWhite, &stats);
    EXPECT_LE(stats.cloudPixelsEmbedded, 2'000u);
    EXPECT_GT(stats.cloudPixelsEmbedded, 1'500u);
    EXPECT_TRUE(near(coarse.pixel(100, 50), qRgb(220, 0, 0)));

    const auto uncapped = [&](double k) {
        PlanPaintOptions sheet = paperOptions(settings, 8.0 * k);
        sheet.rasterDpiCap = 0.0;
        PlanPaintStats at;
        (void)paintedSource(source,
                            frameOf(200.0 * k, 100.0 * k, 2.0 * k, Point2(0.0, 0.0), katana::math::kPi / 6.0),
                            sheet, kWhite, &at);
        return at.cloudPixelsEmbedded;
    };
    EXPECT_EQ(uncapped(1.0), 114u * 96u);
    EXPECT_EQ(uncapped(2.0), 114u * 96u);
}

TEST(PlanPainter, AMeshFootprintOnPaperIsOutlinedInPaperMillimetresAtEveryResolution)
{
    // A black mesh 60 units square about the origin, plotted twice: at
    // 10 px/mm through a frame of 1 px a unit, and at 20 px/mm through one
    // of 2 px a unit - the same sheet at twice the resolution. Its outline is
    // kMeshOutlinePaperMillimetres = 0.13 mm, 1.3 and 2.6 px, half of it
    // outside the square, dashed 2 mm on and 1 mm off; inside is the faint
    // fill. The ink OUTSIDE the square is the outline's outer half alone, so
    // at twice the resolution, twice as long and twice as wide, it is four
    // times as much. A screen's one-pixel pen would be twice as much.
    katana::geometry::TriangleMesh mesh;
    mesh.vertices = {{-30.0, -30.0, 0.0}, {30.0, -30.0, 0.0}, {30.0, 30.0, 0.0}, {-30.0, 30.0, 0.0}};
    mesh.faces = {{0, 1, 2}, {0, 2, 3}};
    std::vector<katana::cad::SceneMesh> meshes(1);
    meshes[0].name = "slab";
    meshes[0].mesh = &mesh;
    meshes[0].flatColor = katana::render::rgba(0, 0, 0);
    PlanSource source;
    source.meshes = &meshes;
    PlotSettings settings;
    const auto outsideInk = [&](double k) {
        const QImage image = paintedSource(source, frameOf(100.0 * k, 100.0 * k, k, Point2(0.0, 0.0)),
                                           paperOptions(settings, 10.0 * k), kWhite);
        const int lo = static_cast<int>(20.0 * k);
        const int hi = static_cast<int>(80.0 * k);
        double total = 0.0;
        for (int y = 0; y < image.height(); ++y) {
            for (int x = 0; x < image.width(); ++x) {
                if (x < lo || x >= hi || y < lo || y >= hi) {
                    total += (255.0 - qGray(image.pixel(x, y))) / 255.0;
                }
            }
        }
        return total;
    };
    const double coarse = outsideInk(1.0);
    const double fine = outsideInk(2.0);
    ASSERT_GT(coarse, 10.0);
    EXPECT_GT(fine / coarse, 3.4);
    EXPECT_LT(fine / coarse, 4.6);
}

TEST(PlanPainter, ASinglePagePlotEmbedsTheRasterItShowsAndNotAHiddenOne)
{
    // File > Plot to PDF: the imagery prints, as one embedded image, and a
    // raster hidden in the view does not.
    ReferenceData reference;
    const katana::interop::ReferenceId id = reference.add(rasterOf(
        200, 200, {0.0, 1.0, 0.0, 200.0, 0.0, -1.0}, [](int x, int y) { return qRgb(x, y, 128); }));
    PlanSource source;
    source.reference = &reference;
    PlotSettings settings;
    settings.paper = katana::cad::PaperSize::A4;
    settings.dpi = 150.0;
    settings.scaleDenominator = 2000.0;
    settings.center = Point2(100.0, 100.0);
    QTemporaryDir dir;
    ASSERT_TRUE(dir.isValid());
    const auto plotted = [&](const std::set<std::uint64_t>& hidden) {
        const QString path = dir.filePath("plot.pdf");
        PlanPaintCache cache;
        const auto status = katana::qt::plotPlanToPdf(path, settings, source,
                                                      katana::cad::LayerOverrides{}, hidden, cache);
        EXPECT_TRUE(status.ok());
        QFile file(path);
        EXPECT_TRUE(file.open(QIODevice::ReadOnly));
        return file.readAll();
    };
    // An image XObject; every page's /ProcSet names /ImageB and /ImageC
    // whether it has one or not.
    EXPECT_TRUE(plotted({}).contains("/Subtype /Image"));
    EXPECT_FALSE(plotted({id}).contains("/Subtype /Image"));
}

// ---- screen speed, without a change of look --------------------------------------------

TEST(PlanPainter, ALineClippedToTheViewPaintsWhatTheWholeLinePaintsToWithinAntialiasing)
{
    // A zig-zag of 400 vertices from x = -2000 to 2000 through a 200 x 100
    // view about (3, 1) at 1 px a unit: all but a twentieth of it is off
    // screen. Where it crosses the view (i = 190 to 210) its even vertices
    // are at y = -80 + 0.37 i, about -10 to -2, inside, and its odd ones at
    // about 150, far above: it leaves the view and comes back ten times.
    // Clipped with the pen's margin it must paint what the whole line
    // paints, at the 1.5 px hairline and the thin pixel both.
    Model model;
    Polyline2 zigzag;
    for (int i = 0; i < 400; ++i) {
        zigzag.vertices.emplace_back(-2000.0 + 10.0 * i, (i % 2 == 0 ? -80.0 : 80.0) + 0.37 * i);
    }
    add(model, entityOf(zigzag));
    const PlanFrame frame = frameOf(200.0, 100.0, 1.0, Point2(3.0, 1.0));
    for (const bool thin : {false, true}) {
        PlanPaintOptions whole;
        whole.clipLines = false;
        whole.thinLines = thin;
        PlanPaintOptions clipped = whole;
        clipped.clipLines = true;
        PlanPaintStats stats;
        const QImage expected = painted(model, frame, whole);
        const QImage actual = painted(model, frame, clipped, kBlack, &stats);
        EXPECT_EQ(stats.linesClipped, 1u);
        int differing = 0;
        int largest = 0;
        for (int y = 0; y < expected.height(); ++y) {
            for (int x = 0; x < expected.width(); ++x) {
                const int difference = std::abs(qGray(expected.pixel(x, y)) - qGray(actual.pixel(x, y)));
                differing += difference > 0 ? 1 : 0;
                largest = std::max(largest, difference);
            }
        }
        // Measured: 1 257 pixels a level or three apart for the 1.5 px pen,
        // 94 up to eight levels apart along the view's edge for the thin
        // one. Qt's rasterisers are not local to the segment: the same
        // segment drawn in a shorter polyline comes out a few levels of
        // antialiasing different, which no eye sees and a bit-for-bit
        // comparison does. Anything more - a gap, a missing run, a line
        // drawn where it was not - is dozens of levels.
        EXPECT_LE(largest, 8) << "thin " << thin << ", " << differing << " pixels differ";
    }
}

TEST(PlanPainter, EachLayerAndStyleIsResolvedOncePerPaintHoweverManyEntitiesShareIt)
{
    // 100 lines on layer "a" and 50 on "b", no styles or colours of their
    // own: two displays to resolve, not 150.
    Model model;
    for (const char* name : {"a", "b"}) {
        katana::entity::Layer layer;
        layer.name = name;
        ASSERT_TRUE(model.layers.add(layer));
    }
    for (int i = 0; i < 150; ++i) {
        add(model, entityOf(Segment2{Point2(-40.0, i * 0.5 - 37.0), Point2(40.0, i * 0.5 - 37.0)},
                            i < 100 ? "a" : "b"));
    }
    PlanPaintStats stats;
    (void)painted(model, frameOf(100.0, 100.0, 1.0, Point2(0.0, 0.0)), PlanPaintOptions{}, kBlack,
                  &stats);
    EXPECT_EQ(stats.entitiesDrawn, 150u);
    EXPECT_EQ(stats.displaysResolved, 2u);
}

TEST(PlanPainter, ThinScreenLinesAreOnePixelAndTheHairlineIsWider)
{
    // The line y = -0.5 in a 100-pixel-high view about the origin at 1 px a
    // unit lies along y = 50.5, the middle of pixel row 50. A cosmetic pixel
    // pen covers row 50 alone; the 1.5 px hairline covers 49.75 to 51.25, a
    // quarter of rows 49 and 51 as well.
    Model model;
    add(model, entityOf(Segment2{Point2(-40.0, -0.5), Point2(40.0, -0.5)}));
    const PlanFrame frame = frameOf(100.0, 100.0, 1.0, Point2(0.0, 0.0));
    PlanPaintOptions thin;
    thin.thinLines = true;
    const QImage one = painted(model, frame, thin);
    EXPECT_NE(one.pixel(50, 50), kBlack);
    EXPECT_EQ(one.pixel(50, 49), kBlack);
    EXPECT_EQ(one.pixel(50, 51), kBlack);
    const QImage hairline = painted(model, frame, PlanPaintOptions{});
    EXPECT_NE(hairline.pixel(50, 49), kBlack);
    EXPECT_NE(hairline.pixel(50, 51), kBlack);
}

TEST(PlanPainter, SymbolsAreStampedFromSpritesOnScreenAndStrokedOnPaper)
{
    // Forty points in a style drawing the built-in "circle" 6 units wide, at
    // 2 px a unit (12 px across): on screen each stamp is a sprite, and the
    // stamps share at most one sprite per quarter-pixel phase (4 x 4); drawn
    // from the sprites the points look as stroked ones do, within an eighth
    // of a pixel's antialiasing. On paper every stamp is stroked.
    Model model;
    katana::entity::Style style;
    style.name = "manhole";
    style.symbol = "circle";
    style.symbolSize = 6.0;
    ASSERT_TRUE(model.styles.add(style));
    for (int i = 0; i < 40; ++i) {
        Entity point = entityOf(katana::entity::PointGeometry{Point2(-45.0 + 2.3 * i, 3.0 + 0.1 * i)});
        point.style = "manhole";
        add(model, point);
    }
    const PlanFrame frame = frameOf(240.0, 120.0, 2.0, Point2(0.0, 0.0));
    PlanPaintOptions stroked;
    PlanPaintOptions sprites;
    sprites.symbolSprites = true;
    PlanPaintCache cache;
    PlanPaintStats spriteStats;
    const QImage fromSprites = painted(model, frame, sprites, kBlack, &spriteStats, &cache);
    EXPECT_EQ(spriteStats.symbolsStamped, 40u);
    EXPECT_EQ(spriteStats.spritesDrawn, 40u);
    EXPECT_LE(cache.spriteCount(), 16u);
    const QImage vector = painted(model, frame, stroked);
    // Every stamp in the same place: nowhere does one image have ink where
    // the other has none by more than faint antialiasing.
    int strongDifferences = 0;
    for (int y = 0; y < vector.height(); ++y) {
        for (int x = 0; x < vector.width(); ++x) {
            if (std::abs(qGray(vector.pixel(x, y)) - qGray(fromSprites.pixel(x, y))) > 96) {
                ++strongDifferences;
            }
        }
    }
    EXPECT_EQ(strongDifferences, 0);

    PlotSettings settings;
    PlanPaintOptions paper = paperOptions(settings, 4.0);
    paper.symbolSprites = true; // asked for, and not given: a plot stays vector
    PlanPaintStats paperStats;
    (void)painted(model, frame, paper, kWhite, &paperStats);
    EXPECT_EQ(paperStats.symbolsStamped, 40u);
    EXPECT_EQ(paperStats.spritesDrawn, 0u);
}

// ---- the plan view's kept drawing -------------------------------------------------------

TEST(PlanView, AMouseMoveOverAnUnchangedDrawingDoesNotPaintTheDrawingAgain)
{
    katana::cad::Document document;
    ASSERT_TRUE(
        document.execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(100.0, 50.0)))
            .ok());
    katana::cad::ViewSet views;
    katana::cad::ViewState& state = views.add(katana::cad::ViewKind::Plan);
    katana::qt::ViewportWidget view(document, state);
    view.resize(400, 300);
    QString stats;
    view.onFrameStats = [&stats](const QString& text) { stats = text; };
    katana::qt::test::paint(view);
    const std::size_t first = view.drawingPaintCount();
    ASSERT_GE(first, 1u);
    EXPECT_TRUE(stats.startsWith("Plan  1 drawn")) << stats.toStdString();

    // The cursor moves: only the overlay is painted.
    const QPointF at(120.0, 80.0);
    QMouseEvent move(QEvent::MouseMove, at, view.mapToGlobal(at), Qt::NoButton, Qt::NoButton,
                     Qt::NoModifier);
    QCoreApplication::sendEvent(&view, &move);
    katana::qt::test::paint(view);
    EXPECT_EQ(view.drawingPaintCount(), first);
    EXPECT_TRUE(stats.contains("kept")) << stats.toStdString();

    // A command changes the drawing; a pan changes the view; a layer hidden
    // in this view changes what it shows: each paints the drawing again.
    ASSERT_TRUE(
        document.execute(katana::commands::createLine(Point2(0.0, 50.0), Point2(100.0, 0.0)))
            .ok());
    katana::qt::test::paint(view);
    EXPECT_EQ(view.drawingPaintCount(), first + 1);
    // Zoomed as the user zooms, with the wheel: a view the user has moved
    // keeps its own centre and scale from then on.
    const QPointF middle(200.0, 150.0);
    QWheelEvent wheel(middle, view.mapToGlobal(middle), QPoint(), QPoint(0, 120), Qt::NoButton,
                      Qt::NoModifier, Qt::NoScrollPhase, false);
    QCoreApplication::sendEvent(&view, &wheel);
    katana::qt::test::paint(view);
    EXPECT_EQ(view.drawingPaintCount(), first + 2);
    state.layers.hide("0");
    katana::qt::test::paint(view);
    EXPECT_EQ(view.drawingPaintCount(), first + 3);
    EXPECT_EQ(view.lastDrawnEntityCount(), 0u);
}

TEST(PlanView, ASelectionChangedWithoutANotificationIsStillDrawn)
{
    // The selection is part of what the kept drawing shows: a caller that
    // sets it and only repaints must see it, notification or not.
    katana::cad::Document document;
    ASSERT_TRUE(
        document.execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(100.0, 50.0)))
            .ok());
    katana::cad::ViewSet views;
    katana::cad::ViewState& state = views.add(katana::cad::ViewKind::Plan);
    katana::qt::ViewportWidget view(document, state);
    view.resize(400, 300);
    katana::qt::test::paint(view);
    const std::size_t first = view.drawingPaintCount();
    document.selection().set(document.lastCreatedEntities());
    katana::qt::test::paint(view);
    EXPECT_EQ(view.drawingPaintCount(), first + 1);
}

TEST(PlanView, APlotFittedToTheDrawingFitsWhatTheViewDrawsAndNotEveryEntity)
{
    // Audit QT-12 / GEO-01, worked by hand. Drawn: a line (0, 0)-(100, 50)
    // and an alignment (0, 0)-(300, 0), which is in no entity's bounds. Not
    // drawn: a stray (5000, 5000)-(5010, 5010) on a layer this view hides.
    // What the view draws is x 0..300, y 0..50.
    //
    // A3 landscape less 10 mm margins is 400 x 277 mm. 300 m across needs
    // 1 : 300 000 / 400 = 1 : 750 and 50 m up 1 : 50 000 / 277 = 1 : 181, so
    // the first standard scale is 1 : 1000, about the middle (150, 25).
    // The spatial index's bounds - the hidden stray in, the alignment out -
    // are 0..5010 both ways: 1 : 5 010 000 / 277 = 1 : 18 087, so 1 : 20 000
    // about (2505, 2505), a sheet with the drawing a dot in its corner.
    katana::cad::Document document;
    ASSERT_TRUE(
        document.execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(100.0, 50.0)))
            .ok());
    katana::entity::Layer far;
    far.name = "far";
    ASSERT_TRUE(document.execute(katana::commands::createLayer(far)).ok());
    ASSERT_TRUE(document
                    .execute(katana::commands::createLine(Point2(5000.0, 5000.0),
                                                          Point2(5010.0, 5010.0)))
                    .ok());
    ASSERT_TRUE(
        document.execute(katana::commands::setEntityLayer(document.lastCreatedEntities(), "far"))
            .ok());
    katana::entity::Alignment road;
    road.name = "road";
    road.horizontal.pis = {katana::geometry::AlignmentPI{Point2(0.0, 0.0)},
                           katana::geometry::AlignmentPI{Point2(300.0, 0.0)}};
    ASSERT_TRUE(document.execute(katana::commands::createAlignment(road)).ok());
    katana::cad::ViewSet views;
    katana::cad::ViewState& state = views.add(katana::cad::ViewKind::Plan);
    state.layers.hide("far");
    katana::qt::ViewportWidget view(document, state);

    PlotSettings settings; // A3 landscape, 10 mm margins, 300 dpi
    settings.scaleDenominator = 500.0;
    const auto fitted = view.fittedPlot(settings);
    ASSERT_TRUE(fitted.ok()) << fitted.error().describe();
    EXPECT_EQ(fitted->scaleDenominator, 1000.0);
    EXPECT_NEAR(fitted->center.x, 150.0, 1e-9);
    EXPECT_NEAR(fitted->center.y, 25.0, 1e-9);
    // The sheet is the one asked for.
    EXPECT_EQ(fitted->paper, katana::cad::PaperSize::A3);
    EXPECT_TRUE(fitted->landscape);
    EXPECT_EQ(fitted->dpi, 300.0);

    // A view that draws nothing has nothing to fit, and says so.
    state.layers.hide("0");
    ASSERT_TRUE(document.execute(katana::commands::deleteAlignment("road")).ok());
    EXPECT_FALSE(view.fittedPlot(settings).ok());
}

// ---- what the cache keeps across paints ------------------------------------------------

TEST(PlanPainter, ALinetypeEditedBetweenTwoPaintsSharingACacheIsDrawnWithItsNewDashes)
{
    // A segment x = -85 to 85 at 1 px a unit in a view 200 px wide about the
    // origin runs from column 15 to 185 along row 20, on a layer whose model
    // linetype is 20 on, 20 off: dashes over columns 15-35, 55-75, 95-115,
    // 135-155 and 175-185 (the last cut short by the end), five runs of ink
    // (a square cap adds at most 0.75 px to each end, and the gaps are 20
    // px). The linetype is then edited to 10 on, 10 off - what the Style
    // Manager's updateLinetype does to the model - and painted again through
    // the SAME cache at the same scale: 15-25, 35-45, ..., 175-185, nine
    // runs. (170 is not a whole number of either period, so no dash begins
    // at the very end, where a cosmetic pen's last pixel would draw one.) A
    // dash pattern kept by the linetype's name drew the old five until the
    // view was zoomed, and a second plot at the same scale printed them.
    Model model;
    katana::entity::Linetype fence;
    fence.name = "fence";
    fence.pattern = {katana::entity::LinetypeElement{20.0}, katana::entity::LinetypeElement{-20.0}};
    ASSERT_TRUE(model.linetypes.add(fence));
    katana::entity::Layer layer;
    layer.name = "fences";
    layer.color = katana::entity::Color{255, 255, 255, 255};
    layer.linetype = "fence";
    ASSERT_TRUE(model.layers.add(layer));
    add(model, entityOf(Segment2{Point2(-85.0, 0.0), Point2(85.0, 0.0)}, "fences"));
    const PlanFrame frame = frameOf(200.0, 40.0, 1.0, Point2(0.0, 0.0));
    for (const bool thin : {false, true}) {
        PlanPaintOptions options;
        options.thinLines = thin;
        PlanPaintCache kept;
        fence.pattern = {katana::entity::LinetypeElement{20.0},
                         katana::entity::LinetypeElement{-20.0}};
        ASSERT_TRUE(model.linetypes.update(fence));
        EXPECT_EQ(runsAlong(painted(model, frame, options, kBlack, nullptr, &kept), 20, kBlack), 5)
            << "thin " << thin;
        fence.pattern = {katana::entity::LinetypeElement{10.0},
                         katana::entity::LinetypeElement{-10.0}};
        ASSERT_TRUE(model.linetypes.update(fence));
        const QImage again = painted(model, frame, options, kBlack, nullptr, &kept);
        EXPECT_EQ(runsAlong(again, 20, kBlack), 9) << "thin " << thin;
        // And exactly what a cache that never saw the old pattern paints.
        EXPECT_TRUE(again == painted(model, frame, options)) << "thin " << thin;
    }
}

// ---- off the GUI thread ----------------------------------------------------------------

TEST(PlanPainter, TwoThreadsPaintingTheSameDrawingAtOnceEachPaintWhatTheGuiThreadPaints)
{
    // The painter reads its arguments and writes only its painter and its
    // cache, so two worker threads, each with its own cache, painting the
    // same model at the same time onto their own images - what a sheet set
    // plotted in the background does - must each get exactly the GUI
    // thread's image: lines, a hatched square, text and sprite-stamped
    // symbols.
    Model model;
    katana::entity::Style style;
    style.name = "pit";
    style.symbol = "square";
    style.symbolSize = 4.0;
    ASSERT_TRUE(model.styles.add(style));
    for (int i = 0; i < 30; ++i) {
        add(model, entityOf(Segment2{Point2(-90.0 + 6.0 * i, -40.0), Point2(-60.0 + 4.0 * i, 40.0)}));
        Entity point = entityOf(katana::entity::PointGeometry{Point2(-80.0 + 5.3 * i, 10.0)});
        point.style = "pit";
        add(model, point);
    }
    add(model, entityOf(katana::entity::TextGeometry{Point2(-50.0, -20.0), "LOT 7", 12.0, 0.2}));
    const PlanFrame frame = frameOf(200.0, 100.0, 1.0, Point2(0.0, 0.0));
    PlanPaintOptions options;
    options.symbolSprites = true;
    const QImage expected = painted(model, frame, options);
    QImage first;
    QImage second;
    {
        std::jthread one([&] { first = painted(model, frame, options); });
        std::jthread two([&] { second = painted(model, frame, options); });
    }
    EXPECT_TRUE(first == expected);
    EXPECT_TRUE(second == expected);
}

// ---- the selection's ghosts (cad/selection_style.hpp) --------------------------------

namespace {

// A white line from (10, 50) to (90, 50) on "design", selected, through a
// frame 100 x 100 px at 1 px a unit about (50, 50): along row 50 from column
// 10 to 90, the frame's y running down. The view hides "design" of its own.
struct GhostPlan {
    Model model;
    katana::cad::SelectionSet selection;
    katana::cad::LayerOverrides view;
    PlanFrame frame = frameOf(100.0, 100.0, 1.0, Point2(50.0, 50.0));
    PlanPaintOptions options;

    GhostPlan()
    {
        katana::entity::Layer layer;
        layer.name = "design";
        layer.color = katana::entity::Color{255, 255, 255, 255};
        EXPECT_TRUE(model.layers.add(layer));
        auto id = model.entities.add(entityOf(Segment2{Point2(10, 50), Point2(90, 50)}, "design"));
        EXPECT_TRUE(id.ok());
        selection.add(*id);
        EXPECT_TRUE(view.hide("design"));
        frame.layers = &view;
        options.selectionGhosts = true;
    }

    QImage paint(PlanPaintStats& stats, bool selected = true) const
    {
        PlanSource source;
        source.model = &model;
        const katana::cad::SelectionSet none;
        source.selection = selected ? &selection : &none;
        return paintedSource(source, frame, options, kBlack, &stats);
    }
};

// Pixels of the line's rows, 48 to 52, in the selection's orange: red well
// above green, green well above blue - never the layer's white, the black
// ground or grey.
std::size_t orangeInk(const QImage& image)
{
    std::size_t ink = 0;
    for (int y = 48; y <= 52; ++y) {
        for (int x = 0; x < image.width(); ++x) {
            const QRgb c = image.pixel(x, y);
            ink += qRed(c) > qGreen(c) + 30 && qGreen(c) > qBlue(c) + 30 ? 1 : 0;
        }
    }
    return ink;
}

} // namespace

TEST(PlanPainter, ASelectedEntityOnALayerThisViewHidesIsDrawnAsAGhost)
{
    // The ghost: #FF9F1C at alpha 153, in 2 px square dots every 6 px from
    // the line's start, not antialiased (selection_style.hpp;
    // PlanPainter::dotPath). The 80 px run from column 10 has a dot at t = 0,
    // 6 ... 78 along it, fourteen, and a dot about x covers columns x - 1 and
    // x of rows 49 and 50 (as the long ghost's test below works out), so row
    // 50 has 28 pixels of ink - columns 9 and 10, then every 6 - and nothing
    // between. Ink is measured against a baseline, the same frame painted
    // with nothing selected, never as pixels lit in the ghosted image alone.
    // Over black a covered pixel is 153/255 of the orange: (153, 95.4,
    // 16.8), within 2 of that after the 8-bit premultiplied blend's rounding.
    GhostPlan plan;
    PlanPaintStats unselected;
    const QImage bare = plan.paint(unselected, false);
    PlanPaintStats selected;
    const QImage ghosted = plan.paint(selected);

    EXPECT_EQ(selected.ghostsDrawn, 1u);
    EXPECT_EQ(unselected.ghostsDrawn, 0u);
    EXPECT_EQ(selected.entitiesDrawn, unselected.entitiesDrawn) << "a ghost is not an entity drawn";
    EXPECT_EQ(selected.entitiesDrawn, 0u) << "the layer is hidden in this view";
    EXPECT_EQ(orangeInk(bare), 0u);
    EXPECT_GT(orangeInk(ghosted), orangeInk(bare));

    const auto inked = [&](int x, int y) { return ghosted.pixel(x, y) != bare.pixel(x, y); };
    int ink = 0;
    int exact = 0;
    for (int x = 0; x < 100; ++x) {
        const QRgb c = ghosted.pixel(x, 50);
        ink += inked(x, 50) ? 1 : 0;
        exact += inked(x, 50) && std::abs(qRed(c) - 153) <= 2 && std::abs(qGreen(c) - 95) <= 2 &&
                         std::abs(qBlue(c) - 17) <= 2
                     ? 1
                     : 0;
    }
    EXPECT_EQ(ink, 28) << "fourteen dots, 2 px each";
    EXPECT_EQ(exact, 28) << "each the selection colour at 60 % over black";
    for (int dot = 0; dot < 14; ++dot) {
        const int x = 10 + 6 * dot;
        EXPECT_TRUE(inked(x - 1, 50)) << "dot " << dot;
        EXPECT_TRUE(inked(x, 50)) << "dot " << dot;
    }
}

TEST(PlanPainter, NoGhostWhereTheDocumentHidesTheLayer)
{
    // A layer the drawing switches off stays off in every view: a ghost says
    // "selected, but hidden in this view", never "selected, though off".
    GhostPlan plan;
    katana::entity::Layer design = *plan.model.layers.find("design");
    design.visible = false;
    ASSERT_TRUE(plan.model.layers.update(design));
    PlanPaintStats stats;
    const QImage image = plan.paint(stats);
    EXPECT_EQ(stats.ghostsDrawn, 0u);
    EXPECT_EQ(orangeInk(image), 0u);
}

TEST(PlanPainter, NoGhostOfAnEntityMadeInvisible)
{
    // The same rule for the entity's own switch (cad::isGhost): invisible,
    // it is hidden in every view, selected or not.
    GhostPlan plan;
    const katana::entity::EntityId id = plan.selection.ids().front();
    katana::entity::Entity hidden = *plan.model.entities.find(id);
    hidden.visible = false;
    ASSERT_TRUE(plan.model.entities.replace(hidden).ok());
    PlanPaintStats stats;
    const QImage image = plan.paint(stats);
    EXPECT_EQ(stats.ghostsDrawn, 0u);
    EXPECT_EQ(orangeInk(image), 0u);
}

TEST(PlanPainter, NoGhostOnPaper)
{
    GhostPlan plan;
    PlotSettings settings;
    plan.options = paperOptions(settings, 4.0);
    plan.options.selectionGhosts = true;
    PlanPaintStats stats;
    (void)plan.paint(stats);
    EXPECT_EQ(stats.ghostsDrawn, 0u) << "a plot draws what the drawing shows, never a ghost";
}

TEST(PlanPainter, NoGhostWithGhostsOff)
{
    // PlanPaintOptions::selectionGhosts defaults to false, so every caller
    // but the plan view - which sets it from ViewState::selectionGhosts - is
    // unchanged.
    GhostPlan plan;
    plan.options = PlanPaintOptions{};
    ASSERT_FALSE(plan.options.selectionGhosts);
    PlanPaintStats stats;
    const QImage image = plan.paint(stats);
    EXPECT_EQ(stats.ghostsDrawn, 0u);
    EXPECT_EQ(orangeInk(image), 0u);
}

TEST(PlanPainter, ASelectedEntityTheViewShowsIsDrawnSelectedAndNotAsAGhost)
{
    // The view hiding nothing: the line is drawn, in the selection's 2 px
    // orange dashes, once.
    GhostPlan plan;
    plan.frame.layers = nullptr;
    PlanPaintStats stats;
    const QImage image = plan.paint(stats);
    EXPECT_EQ(stats.ghostsDrawn, 0u);
    EXPECT_EQ(stats.entitiesDrawn, 1u);
    EXPECT_GT(orangeInk(image), 0u);
}

TEST(PlanPainter, AGhostLongerThanTheViewIsDottedAcrossItFromItsOwnStart)
{
    // A line from (-1e6, 50) to (1e6, 50), both ends far outside the frame,
    // which shows x from 0 to 100 at 1 px a unit: the dots are laid only
    // where it crosses the view, and in step with the line's own start. A
    // dot every 6 px from x = -1e6 puts one at every x = 6k - 1e6, and
    // -1e6 = 2 - 6 x 166 667, so at x = 2, 8, 14 ... 98: 17 dots. A dot
    // 2 px square about x covers the columns x - 1 and x, so columns 1 and
    // 2 are lit and 3 to 6 are not.
    GhostPlan plan;
    auto id = plan.model.entities.add(
        entityOf(Segment2{Point2(-1.0e6, 50), Point2(1.0e6, 50)}, "design"));
    ASSERT_TRUE(id.ok());
    plan.selection.set({*id});
    PlanPaintStats stats;
    const QImage image = plan.paint(stats);
    EXPECT_EQ(stats.ghostsDrawn, 1u);
    // Ink against the baseline, the frame with nothing selected.
    PlanPaintStats none;
    const QImage bare = plan.paint(none, false);
    const auto lit = [&](int x) { return image.pixel(x, 50) != bare.pixel(x, 50); };
    EXPECT_TRUE(lit(1));
    EXPECT_TRUE(lit(2));
    for (int x = 3; x <= 6; ++x) {
        EXPECT_FALSE(lit(x)) << "column " << x;
    }
    EXPECT_TRUE(lit(7));
    EXPECT_TRUE(lit(8));
    int columns = 0;
    for (int x = 0; x < 100; ++x) {
        columns += lit(x) ? 1 : 0;
    }
    EXPECT_EQ(columns, 17 * 2);
}

// ---- a ghost of each kind of geometry -------------------------------------------------------
//
// drawSelectionGhosts has a branch for each kind: a line is dotted as it is;
// a polyline, an arc or a circle goes through drawGeometry's own
// strokePolyline, dotted only because the ghost pass asks it to be; a point
// is a ring; a text or a dimension the outline of the box it draws; a label
// nothing. Each is painted alone, selected, on the layer GhostPlan's view
// hides, and its ink read against the same frame with nothing selected.

namespace {

// The pixels the ghost inked: those that differ from the frame painted with
// nothing selected.
std::vector<QPoint> ghostInk(const GhostPlan& plan, PlanPaintStats& stats)
{
    PlanPaintStats none;
    const QImage bare = plan.paint(none, false);
    const QImage ghosted = plan.paint(stats);
    std::vector<QPoint> ink;
    for (int y = 0; y < ghosted.height(); ++y) {
        for (int x = 0; x < ghosted.width(); ++x) {
            if (ghosted.pixel(x, y) != bare.pixel(x, y)) {
                ink.emplace_back(x, y);
            }
        }
    }
    return ink;
}

// How many separate marks the ink makes: pixels grouped with their eight
// neighbours. A dotted trace is many; a solid one is one.
int marksIn(const std::vector<QPoint>& ink)
{
    std::vector<bool> seen(ink.size(), false);
    int marks = 0;
    for (std::size_t first = 0; first < ink.size(); ++first) {
        if (seen[first]) {
            continue;
        }
        ++marks;
        std::vector<std::size_t> open{first};
        seen[first] = true;
        while (!open.empty()) {
            const QPoint at = ink[open.back()];
            open.pop_back();
            for (std::size_t other = 0; other < ink.size(); ++other) {
                if (!seen[other] && std::abs(ink[other].x() - at.x()) <= 1 &&
                    std::abs(ink[other].y() - at.y()) <= 1) {
                    seen[other] = true;
                    open.push_back(other);
                }
            }
        }
    }
    return marks;
}

// Selects `geometry`, alone, on GhostPlan's hidden layer.
void selectOnly(GhostPlan& plan, katana::entity::Geometry geometry)
{
    auto id = plan.model.entities.add(entityOf(std::move(geometry), "design"));
    ASSERT_TRUE(id.ok());
    plan.selection.set({*id});
}

// The ink is the dotted outline of a box: every pixel within a pixel of an
// edge of the box round the ink, none inside it, and separate dots along
// each of its four sides. Returns that box, the ink's own bounds.
QRect expectDottedBox(const std::vector<QPoint>& ink)
{
    EXPECT_FALSE(ink.empty());
    if (ink.empty()) {
        return {};
    }
    int left = ink.front().x();
    int right = left;
    int top = ink.front().y();
    int bottom = top;
    for (const QPoint& p : ink) {
        left = std::min(left, p.x());
        right = std::max(right, p.x());
        top = std::min(top, p.y());
        bottom = std::max(bottom, p.y());
    }
    std::array<std::vector<QPoint>, 4> sides; // top, bottom, left, right
    for (const QPoint& p : ink) {
        const bool onTop = p.y() <= top + 1;
        const bool onBottom = p.y() >= bottom - 1;
        const bool onLeft = p.x() <= left + 1;
        const bool onRight = p.x() >= right - 1;
        EXPECT_TRUE(onTop || onBottom || onLeft || onRight)
            << "ink inside the box at " << p.x() << "," << p.y();
        if (onTop) {
            sides[0].push_back(p);
        }
        if (onBottom) {
            sides[1].push_back(p);
        }
        if (onLeft) {
            sides[2].push_back(p);
        }
        if (onRight) {
            sides[3].push_back(p);
        }
    }
    for (const auto& side : sides) {
        EXPECT_GE(marksIn(side), 2) << "dots along each side, not one mark";
    }
    return QRect(QPoint(left, top), QPoint(right, bottom));
}

} // namespace

TEST(PlanPainter, AGhostedPolylineIsDottedAlongEachLegWithItsStepCarriedRoundTheCorner)
{
    // (10,50) -> (40,50) -> (40,20) in the model is screen (10,50) -> (40,50)
    // -> (40,80) in the 100 x 100 frame at 1 px a unit about (50, 50). A dot
    // every 6 px from the start: at 0, 6 ... 24 along the 30 px first leg,
    // x = 10, 16, 22, 28 and 34 on row 50; 30 is a whole number of steps, so
    // the next falls on the corner, (40, 50); then y = 56, 62, 68 and 74 down
    // the second leg, whose end, 30 along it, is not dotted (a dot is laid
    // short of a leg's end). A 2 px dot about (x, y) covers columns x - 1 and
    // x of rows y - 1 and y: ten dots, forty pixels, and nothing between them
    // - where the solid stroke drawGeometry lays without the dots covers row
    // 50 from column 9 to 40.
    GhostPlan plan;
    selectOnly(plan, Polyline2{{Point2(10, 50), Point2(40, 50), Point2(40, 20)}, false});
    PlanPaintStats stats;
    const std::vector<QPoint> ink = ghostInk(plan, stats);
    EXPECT_EQ(stats.ghostsDrawn, 1u);
    EXPECT_EQ(stats.entitiesDrawn, 0u);

    std::vector<QPoint> expected;
    const auto dot = [&expected](int x, int y) {
        for (const QPoint p :
             {QPoint(x - 1, y - 1), QPoint(x, y - 1), QPoint(x - 1, y), QPoint(x, y)}) {
            expected.push_back(p);
        }
    };
    for (const int x : {10, 16, 22, 28, 34, 40}) {
        dot(x, 50);
    }
    for (const int y : {56, 62, 68, 74}) {
        dot(40, y);
    }
    const auto byPlace = [](const QPoint& a, const QPoint& b) {
        return a.y() != b.y() ? a.y() < b.y() : a.x() < b.x();
    };
    std::ranges::sort(expected, byPlace);
    std::vector<QPoint> got = ink;
    std::ranges::sort(got, byPlace);
    EXPECT_EQ(got, expected) << "ten 2 px dots and nothing between them";
}

TEST(PlanPainter, AGhostedCircleIsARowOfSeparateDotsOnItsCurve)
{
    // A circle of radius 30 about (50, 50): screen radius 30 about (50, 50),
    // chorded to a quarter of a pixel and dotted every 6 px of its 188.5 px
    // round, 32 dots (the last 2.5 px short of the first, so the two may
    // touch). A dot is put on the whole pixel nearest where it falls, at most
    // half a pixel off each way, and its 2 px square covers the two pixel
    // centres half a pixel either side of that: a pixel of ink is at most a
    // pixel off a point on a chord each way, 1.42 px in all, and a chord is
    // within 0.25 px of the circle - so 28.33 to 31.42 px from the centre.
    // Without the dots, the ghost pen strokes the circle solid: one mark,
    // not thirty.
    GhostPlan plan;
    selectOnly(plan, katana::geometry::Circle2{Point2(50, 50), 30.0});
    PlanPaintStats stats;
    const std::vector<QPoint> ink = ghostInk(plan, stats);
    EXPECT_EQ(stats.ghostsDrawn, 1u);
    for (const QPoint& p : ink) {
        const double r = std::hypot(p.x() + 0.5 - 50.0, p.y() + 0.5 - 50.0);
        EXPECT_GE(r, 28.33) << p.x() << "," << p.y();
        EXPECT_LE(r, 31.42) << p.x() << "," << p.y();
    }
    const int marks = marksIn(ink);
    EXPECT_GE(marks, 31) << "a dot every 6 px";
    EXPECT_LE(marks, 32);
}

TEST(PlanPainter, AGhostedPointIsADottedRingAroundItNotAMarkOnIt)
{
    // A ring of 6 px radius about the point (selection_style.hpp), outside
    // the cross the point is drawn with: a 24-gon whose round is 37.6 px,
    // dotted every 6 px - seven dots, the last 1.6 px short of the first, so
    // six marks or seven. A pixel of ink is at most 1.42 px from a point on
    // the ring's chords (as the circle's above), which are 5.95 to 6 px out:
    // 4.53 to 7.42 px from the centre, and nothing on the point itself.
    GhostPlan plan;
    selectOnly(plan, katana::entity::PointGeometry{Point2(50, 50)});
    PlanPaintStats stats;
    const std::vector<QPoint> ink = ghostInk(plan, stats);
    EXPECT_EQ(stats.ghostsDrawn, 1u);
    ASSERT_FALSE(ink.empty());
    for (const QPoint& p : ink) {
        const double r = std::hypot(p.x() + 0.5 - 50.0, p.y() + 0.5 - 50.0);
        EXPECT_GE(r, 4.53) << p.x() << "," << p.y();
        EXPECT_LE(r, 7.42) << p.x() << "," << p.y();
    }
    const int marks = marksIn(ink);
    EXPECT_GE(marks, 6);
    EXPECT_LE(marks, 7);
}

TEST(PlanPainter, AGhostedTextIsTheDottedOutlineOfItsBoxNotItsLetters)
{
    // "LOT 7", 10 units tall from its baseline's left end at (20, 40). One
    // line of baseline-left text is its height tall above the baseline
    // (textBlockExtent), so its box is x from 20, y from 40 to 50: screen
    // column 20 on, rows 50 to 60, and wider than it is tall. A dot about an
    // edge covers the pixel before it as well, so the ink runs from column 19
    // and over rows 49 to 60. Its words at a ghost's strength would be
    // unreadable, and letters would put ink inside the box.
    GhostPlan plan;
    selectOnly(plan, katana::entity::TextGeometry{Point2(20, 40), "LOT 7", 10.0, 0.0});
    PlanPaintStats stats;
    const std::vector<QPoint> ink = ghostInk(plan, stats);
    EXPECT_EQ(stats.ghostsDrawn, 1u);
    const QRect box = expectDottedBox(ink);
    EXPECT_EQ(box.left(), 19);
    EXPECT_EQ(box.top(), 49);
    EXPECT_EQ(box.bottom(), 60);
    EXPECT_GT(box.width(), box.height());
}

TEST(PlanPainter, AGhostedDimensionIsTheDottedOutlineOfWhatItDraws)
{
    // From (20, 30) to (80, 30), 10 above: screen (20, 70) to (80, 70), its
    // dimension line 10 px higher. The outline of the box the dimension
    // draws - extension lines, arrows and text - holds both points; drawn
    // as itself, its dimension line would cross the box's inside.
    GhostPlan plan;
    katana::entity::DimensionGeometry dimension;
    dimension.start = Point2(20, 30);
    dimension.end = Point2(80, 30);
    dimension.offset = 10.0;
    selectOnly(plan, dimension);
    PlanPaintStats stats;
    const std::vector<QPoint> ink = ghostInk(plan, stats);
    EXPECT_EQ(stats.ghostsDrawn, 1u);
    const QRect box = expectDottedBox(ink);
    EXPECT_LE(box.left(), 21);
    EXPECT_GE(box.right(), 79);
    EXPECT_GE(box.bottom(), 69);
    EXPECT_LE(box.top(), 60) << "the dimension line and its text stand above the points";
}

TEST(PlanPainter, ALabelIsNeverGhosted)
{
    // A label is placed with the others, and has no outline of its own to
    // dot: selected on a layer the view hides, it leaves no trace there.
    GhostPlan plan;
    // Labelling the fixture's line, a label's one target.
    const katana::entity::EntityId line = plan.selection.ids().front();
    selectOnly(plan, katana::entity::LabelGeometry{
                         .target = line, .style = "any", .anchor = Point2(50, 50)});
    ASSERT_EQ(plan.selection.size(), 1u);
    ASSERT_NE(plan.selection.ids().front(), line) << "the label alone is selected";
    PlanPaintStats stats;
    const std::vector<QPoint> ink = ghostInk(plan, stats);
    EXPECT_EQ(stats.ghostsDrawn, 0u);
    EXPECT_TRUE(ink.empty()) << ink.size() << " pixels";
}

TEST(PlanPainter, AtAFractionalScaleEveryGhostDotIsTheSameSquareOfWholePixels)
{
    // At 125 % - the owner's display (docs/render.md) - the 2 px pen was 2.5
    // device pixels, drawn without antialiasing as 2 or 3: a straight ghost
    // beaded 3, 2, 3 pixels, and a slanted one mixed four shapes of dot. The
    // pen and the pitch are whole device pixels now, rounded - 2 x 1.25 =
    // 2.5 to 3, 6 x 1.25 = 7.5 to 8 - and each dot is put on whole pixels.
    // GhostPlan's line, (10,50)-(90,50), is device (12.5, 62.5) to (112.5,
    // 62.5): 100 device pixels, a dot at 0, 8 ... 96 along it, thirteen, each
    // 3 px square about a pixel's centre - columns 11 to 13 of rows 61 to 63
    // for the first, and 8 columns on for each after. Ink against the frame
    // painted with nothing selected, at the same scale.
    GhostPlan plan;
    const auto paintAt = [&plan](bool selected) {
        QImage image(125, 125, QImage::Format_ARGB32_Premultiplied);
        image.setDevicePixelRatio(1.25);
        image.fill(kBlack);
        PlanSource source;
        source.model = &plan.model;
        const katana::cad::SelectionSet none;
        source.selection = selected ? &plan.selection : &none;
        PlanPaintCache cache;
        QPainter painter(&image);
        (void)paintPlan(painter, source, plan.frame, plan.options, cache);
        painter.end();
        return image;
    };
    const QImage bare = paintAt(false);
    const QImage ghosted = paintAt(true);
    std::vector<QPoint> ink;
    for (int y = 0; y < ghosted.height(); ++y) {
        for (int x = 0; x < ghosted.width(); ++x) {
            if (ghosted.pixel(x, y) != bare.pixel(x, y)) {
                ink.emplace_back(x, y);
            }
        }
    }
    std::vector<QPoint> expected;
    for (int dot = 0; dot < 13; ++dot) {
        for (int y = 61; y <= 63; ++y) {
            for (int x = 11 + 8 * dot; x <= 13 + 8 * dot; ++x) {
                expected.emplace_back(x, y);
            }
        }
    }
    const auto byPlace = [](const QPoint& a, const QPoint& b) {
        return a.y() != b.y() ? a.y() < b.y() : a.x() < b.x();
    };
    std::ranges::sort(expected, byPlace);
    std::ranges::sort(ink, byPlace);
    EXPECT_EQ(ink.size(), expected.size());
    EXPECT_EQ(ink, expected) << "thirteen 3 x 3 dots, 8 px apart";
}
