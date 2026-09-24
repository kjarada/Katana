// The plan painter (src/katana_qt/plan_painter): what it draws through a
// rotated frame, on paper and on screen, and what its screen-speed measures
// leave unchanged. Painted into QImages and read back pixel by pixel, on the
// offscreen platform the widget tests run on.

#include <gtest/gtest.h>

#include <cmath>
#include <cstdint>
#include <functional>
#include <string>
#include <thread>

#include <QImage>
#include <QMouseEvent>
#include <QWheelEvent>
#include <QPainter>

#include "katana/cad/plot.hpp"
#include "katana/cad/view_set.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/alignment.hpp"
#include "plan_painter.hpp"
#include "widget_harness.hpp"

using katana::cad::PlotSettings;
using katana::entity::Entity;
using katana::entity::Model;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
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

// Paints `model` through `frame` onto a `fill`ed image of the frame's size.
QImage painted(const Model& model, const PlanFrame& frame, const PlanPaintOptions& options,
               QRgb fill = kBlack, PlanPaintStats* stats = nullptr, PlanPaintCache* cache = nullptr)
{
    QImage image(static_cast<int>(frame.transform.widthPixels),
                 static_cast<int>(frame.transform.heightPixels), QImage::Format_ARGB32_Premultiplied);
    image.fill(fill);
    PlanSource source;
    source.model = &model;
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
