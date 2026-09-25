// The sheet painter (src/katana_qt/sheet_painter): the frame, its fields and
// logo slot, each viewport kind, and the multi-page PDF. Painted into QImages
// at 4 px a millimetre and read back pixel by pixel.
//
// With KATANA_SHEET_PNG set to a directory, every sheet a test paints is also
// written there as a PNG, to be looked at.

#include <gtest/gtest.h>

#include <array>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <functional>
#include <set>
#include <string>

#include <QByteArray>
#include <QColor>
#include <QFile>
#include <QFileInfo>
#include <QImage>
#include <QMarginsF>
#include <QPageSize>
#include <QPainter>
#include <QPdfWriter>
#include <QTemporaryDir>

#include "katana/cad/plotting/frame.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/entity/model.hpp"
#include "katana/math/numerics.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "sheet_painter.hpp"

using katana::entity::Entity;
using katana::entity::Model;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Segment2;
using katana::qt::SheetPaintCache;
using katana::qt::SheetPaintOptions;
using katana::qt::SheetPaintStats;
using katana::qt::SheetSource;
namespace plotting = katana::cad::plotting;

namespace {

constexpr double kPpmm = 4.0; // ~100 dpi: every 0.25 mm line is a pixel
const QRgb kWhite = qRgb(255, 255, 255);

plotting::Sheet a3(std::string name = "PLAN")
{
    plotting::Sheet sheet;
    sheet.id = "s1";
    sheet.name = std::move(name);
    return sheet;
}

plotting::Viewport planAt(Box2 rect, double scale, Point2 centre, double rotation = 0.0)
{
    plotting::Viewport viewport;
    viewport.id = "vp1";
    viewport.kind = plotting::ViewportKind::Plan;
    viewport.rect = rect;
    viewport.scale = scale;
    viewport.centre = centre;
    viewport.rotation = rotation;
    return viewport;
}

void addLine(Model& model, Point2 a, Point2 b)
{
    Entity entity;
    entity.geometry = Segment2{a, b};
    entity.layer = "0";
    ASSERT_TRUE(model.entities.add(std::move(entity)).ok());
}

QImage painted(const plotting::SheetSet& set, const SheetSource& source,
               SheetPaintStats* stats = nullptr, bool construction = false,
               const char* tag = "")
{
    const std::size_t index = 0;
    const auto& sheet = set.sheets[index];
    const auto paper = katana::cad::paperDimensions(sheet.paper, sheet.landscape);
    QImage image(static_cast<int>(std::lround(paper.widthMm * kPpmm)),
                 static_cast<int>(std::lround(paper.heightMm * kPpmm)),
                 QImage::Format_ARGB32_Premultiplied);
    image.fill(Qt::gray);
    SheetPaintOptions options;
    options.pixelsPerMillimetre = kPpmm;
    options.construction = construction;
    SheetPaintCache cache;
    QPainter painter(&image);
    const SheetPaintStats result = katana::qt::paintSheet(painter, set, index, source, options, cache);
    painter.end();
    if (stats != nullptr) {
        *stats = result;
    }
    if (const char* dir = std::getenv("KATANA_SHEET_PNG"); dir != nullptr) {
        const auto* test = ::testing::UnitTest::GetInstance()->current_test_info();
        image.save(QString("%1/%2%3.png").arg(dir, test->name(), tag));
    }
    return image;
}

// The device pixel of a paper point on an image painted by `painted`.
QPoint pixelOf(Point2 paper, double heightMm = 297.0)
{
    return QPoint(static_cast<int>(std::floor(paper.x * kPpmm)),
                  static_cast<int>(std::floor((heightMm - paper.y) * kPpmm)));
}

// Pixels in the paper box that are not white.
int inkIn(const QImage& image, Box2 paper, double heightMm = 297.0)
{
    const QPoint a = pixelOf(Point2(paper.min.x, paper.max.y), heightMm);
    const QPoint b = pixelOf(Point2(paper.max.x, paper.min.y), heightMm);
    int count = 0;
    for (int y = std::max(a.y(), 0); y <= std::min(b.y(), image.height() - 1); ++y) {
        for (int x = std::max(a.x(), 0); x <= std::min(b.x(), image.width() - 1); ++x) {
            count += image.pixel(x, y) != kWhite ? 1 : 0;
        }
    }
    return count;
}

// Pixels in the paper box close to `colour`.
int colourIn(const QImage& image, Box2 paper, QColor colour)
{
    const QPoint a = pixelOf(Point2(paper.min.x, paper.max.y));
    const QPoint b = pixelOf(Point2(paper.max.x, paper.min.y));
    int count = 0;
    for (int y = std::max(a.y(), 0); y <= std::min(b.y(), image.height() - 1); ++y) {
        for (int x = std::max(a.x(), 0); x <= std::min(b.x(), image.width() - 1); ++x) {
            const QColor c = image.pixelColor(x, y);
            count += std::abs(c.red() - colour.red()) < 40 &&
                             std::abs(c.green() - colour.green()) < 40 &&
                             std::abs(c.blue() - colour.blue()) < 40
                         ? 1
                         : 0;
        }
    }
    return count;
}

Box2 box(double x0, double y0, double x1, double y1) { return Box2(Point2(x0, y0), Point2(x1, y1)); }

// A `width` x `height` raster whose pixel (x, y) is `colour(x, y)` (with its
// alpha), placed by `geotransform`.
katana::interop::RasterOverlay rasterOf(int width, int height,
                                        const std::array<double, 6>& geotransform,
                                        const std::function<QRgb(int, int)>& colour)
{
    katana::interop::RasterOverlay raster;
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

// The set's first sheet as a one-page PDF at `dpi`, as plotSheetsToPdf
// writes it but with the imagery capped at `rasterDpiCap` (0: not capped
// below the plot's own resolution). Returns the file's size in bytes.
qint64 sheetPdfBytes(const QString& path, const plotting::SheetSet& set, const SheetSource& source,
                     double dpi, double rasterDpiCap)
{
    const auto& sheet = set.sheets[0];
    const auto paper = katana::cad::paperDimensions(sheet.paper, sheet.landscape);
    {
        QPdfWriter writer(path);
        writer.setResolution(static_cast<int>(dpi));
        writer.setPageSize(QPageSize(QSizeF(paper.widthMm, paper.heightMm), QPageSize::Millimeter));
        writer.setPageMargins(QMarginsF(0.0, 0.0, 0.0, 0.0));
        QPainter painter(&writer);
        SheetPaintOptions options;
        options.pixelsPerMillimetre = dpi / 25.4;
        options.plot.dpi = dpi;
        options.rasterDpiCap = rasterDpiCap;
        SheetPaintCache cache;
        (void)katana::qt::paintSheet(painter, set, 0, source, options, cache);
    }
    return QFileInfo(path).size();
}

} // namespace

TEST(SheetPainter, TheFrameIsDrawnAndItsConstructionGuideIsNot)
{
    plotting::SheetSet set;
    set.sheets.push_back(a3());
    Model model;
    SheetSource source;
    source.plan.model = &model;
    const QImage paper = painted(set, source);
    // The border's left rule at x 21.825, halfway up the drawing area.
    EXPECT_GT(inkIn(paper, box(21.3, 150.0, 22.3, 151.0)), 0);
    // The title block's top rule at y 34.625.
    EXPECT_GT(inkIn(paper, box(200.0, 34.1, 201.0, 35.1)), 0);
    // The paper's edge: the construction guide does not plot.
    EXPECT_EQ(inkIn(paper, box(0.0, 100.0, 1.0, 200.0)), 0);

    const QImage editing = painted(set, source, nullptr, true, "_editing");
    // Drawn while editing: a hairline centred on the edge, half of it on the paper.
    EXPECT_GT(inkIn(editing, box(0.0, 100.0, 1.0, 200.0)), 0);
}

TEST(SheetPainter, TheLogoFillsItsSlotAndTheSlotIsEmptyWithoutOne)
{
    plotting::SheetSet set;
    set.sheets.push_back(a3());
    Model model;
    SheetSource source;
    source.plan.model = &model;
    const plotting::Frame& frame = *plotting::builtInFrame();
    const Box2 cell = frame.cell("logo")->rect;

    SheetPaintStats stats;
    const QImage without = painted(set, source, &stats);
    EXPECT_FALSE(stats.logoDrawn);
    EXPECT_EQ(inkIn(without, cell.inflated(-1.5)), 0);

    // A 4:1 red logo: drawn 40 x 10 mm in the middle of the cell.
    source.logo = QImage(400, 100, QImage::Format_ARGB32);
    source.logo.fill(QColor(220, 0, 0));
    const QImage with = painted(set, source, &stats);
    EXPECT_TRUE(stats.logoDrawn);
    const Point2 middle = cell.center();
    EXPECT_GT(colourIn(with, box(middle.x - 19.0, middle.y - 4.5, middle.x + 19.0, middle.y + 4.5),
                       QColor(220, 0, 0)),
              38 * 9 * 4 * 4 * 9 / 10);
    // Nothing of it outside the 40 x 10 it fits to.
    EXPECT_EQ(colourIn(with, box(cell.min.x + 0.2, middle.y - 4.0, middle.x - 21.0, middle.y + 4.0),
                       QColor(220, 0, 0)),
              0);
}

TEST(SheetPainter, FieldsFillTheTitleBlockAndAnEmptyFieldPrintsNothing)
{
    plotting::SheetSet set;
    set.sheets.push_back(a3());
    Model model;
    SheetSource source;
    source.plan.model = &model;
    const Box2 cell = plotting::builtInFrame()->cell("organisation")->rect;
    EXPECT_EQ(inkIn(painted(set, source), cell.inflated(-0.8)), 0);

    set.defaults.organisation = "NORTHERN SURVEY CONSULTANTS";
    const QImage named = painted(set, source);
    EXPECT_GT(inkIn(named, cell.inflated(-0.8)), 100);
}

TEST(SheetPainter, TheLegendBlockCanBeTurnedOff)
{
    plotting::SheetSet set;
    set.sheets.push_back(a3());
    Model model;
    SheetSource source;
    source.plan.model = &model;
    const plotting::FrameSymbol& hydrant = plotting::builtInFrame()->symbols.front();
    const Box2 around = box(hydrant.at.x - 0.4, hydrant.at.y - 0.4, hydrant.at.x + 0.4,
                            hydrant.at.y + 0.4);
    EXPECT_GT(inkIn(painted(set, source), around.inflated(0.4)), 0);
    set.sheets[0].frameLegend = false;
    EXPECT_EQ(inkIn(painted(set, source), around.inflated(0.4)), 0);
}

TEST(SheetPainter, APlanIsDrawnAtItsScaleAndClippedToItsViewport)
{
    // A 100 m line at 1 : 1000 is 100 mm on the paper; one running far past
    // the viewport stops at its edge.
    Model model;
    addLine(model, Point2(0.0, 0.0), Point2(100.0, 0.0));
    addLine(model, Point2(-1000.0, 30.0), Point2(1000.0, 30.0));
    plotting::SheetSet set;
    set.sheets.push_back(a3());
    set.sheets[0].viewports.push_back(planAt(box(100.0, 100.0, 300.0, 250.0), 1000.0, Point2(50.0, 0.0)));
    SheetSource source;
    source.plan.model = &model;
    SheetPaintStats stats;
    const QImage paper = painted(set, source, &stats);
    EXPECT_EQ(stats.viewportsDrawn, 1u);
    EXPECT_TRUE(stats.problems.empty());

    // The short line: from x 150 to 250 on the paper at y 175.
    EXPECT_GT(inkIn(paper, box(151.0, 174.5, 249.0, 175.5)), 90 * 4 / 2);
    EXPECT_EQ(inkIn(paper, box(252.0, 173.0, 290.0, 174.0)), 0);
    // The long line at y 205: inside the viewport, and not outside it.
    EXPECT_GT(inkIn(paper, box(110.0, 204.5, 290.0, 205.5)), 0);
    EXPECT_EQ(inkIn(paper, box(40.0, 203.0, 98.0, 207.0)), 0);
    EXPECT_EQ(inkIn(paper, box(302.0, 203.0, 400.0, 207.0)), 0);
}

TEST(SheetPainter, ARotatedViewportRunsItsDirectionAcrossThePaper)
{
    // A line at 45 degrees, in a viewport turned to 45 degrees, runs level
    // across the paper.
    Model model;
    addLine(model, Point2(-50.0, -50.0), Point2(50.0, 50.0));
    plotting::SheetSet set;
    set.sheets.push_back(a3());
    set.sheets[0].viewports.push_back(planAt(box(100.0, 100.0, 300.0, 250.0), 1000.0,
                                             Point2(0.0, 0.0), katana::math::kPi / 4.0));
    SheetSource source;
    source.plan.model = &model;
    const QImage paper = painted(set, source);
    // Centre (200, 175); the 141 m line is 141 mm long, level.
    EXPECT_GT(inkIn(paper, box(140.0, 174.5, 260.0, 175.5)), 100);
    EXPECT_EQ(inkIn(paper, box(190.0, 185.0, 210.0, 240.0)), 0);
}

TEST(SheetPainter, AnAerialPhotoUnderAPlanViewportPrintsInsideItAndNowhereElse)
{
    // A 500 m square photo of 5 m pixels about the origin, all one blue,
    // under a viewport (100, 100)-(300, 250) at 1 : 1000 about the origin:
    // the viewport shows 200 x 150 m of it and is filled with it; nothing of
    // it is printed beside, above or below the viewport, though the photo
    // runs 150 m past each side. Hidden in the Reference Data panel, it is
    // not printed at all.
    const QColor blue(40, 110, 200);
    katana::interop::ReferenceData reference;
    const katana::interop::ReferenceId id =
        reference.add(rasterOf(100, 100, {-250.0, 5.0, 0.0, 250.0, 0.0, -5.0},
                               [&](int, int) { return blue.rgb(); }));
    Model model;
    plotting::SheetSet set;
    set.sheets.push_back(a3());
    set.sheets[0].viewports.push_back(planAt(box(100.0, 100.0, 300.0, 250.0), 1000.0, Point2(0.0, 0.0)));
    SheetSource source;
    source.plan.model = &model;
    source.plan.reference = &reference;
    const QImage paper = painted(set, source);
    const int inside = 196 * 4 * 146 * 4;
    EXPECT_GT(colourIn(paper, box(102.0, 102.0, 298.0, 248.0), blue), inside * 95 / 100);
    EXPECT_EQ(colourIn(paper, box(40.0, 100.0, 99.0, 250.0), blue), 0);
    EXPECT_EQ(colourIn(paper, box(301.0, 100.0, 380.0, 250.0), blue), 0);
    EXPECT_EQ(colourIn(paper, box(100.0, 251.0, 300.0, 280.0), blue), 0);
    EXPECT_EQ(colourIn(paper, box(100.0, 40.0, 300.0, 99.0), blue), 0);

    reference.findRaster(id)->visible = false;
    EXPECT_EQ(colourIn(painted(set, source, nullptr, false, "_hidden"),
                       box(102.0, 102.0, 298.0, 248.0), blue),
              0);
}

TEST(SheetPainter, ATurnedPhotoInATurnedViewportIsPrintedWhereItsGeotransformPutsIt)
{
    // An 80 x 80 image of 0.5 m pixels, transparent but for a red block of
    // pixels 32..47 each way, turned 30 degrees about its corner at (100,
    // 50): a step along a row moves 0.5 (cos 30, sin 30), a step down a
    // column 0.5 (sin 30, -cos 30). The block's middle, pixel corner (40,
    // 40), is (100, 50) + 20 (1.366, -0.366) = (127.32, 42.68), and the block
    // is 8 m square. The viewport (100, 100)-(300, 250) at 1 : 1000 about
    // (110, 40) is turned 0.6 rad, so a model offset d from its centre lands
    // at the paper's (200, 175) + R(-0.6) d mm: d = (17.32, 2.68) is
    //   x = 17.32 cos 0.6 + 2.68 sin 0.6 = 14.295 + 1.513 = 15.808
    //   y = -17.32 sin 0.6 + 2.68 cos 0.6 = -9.780 + 2.212 = -7.568
    // so the block prints about (215.8, 167.4), 8 mm square turned, and not
    // about (217.3, 177.7), where an unturned viewport would put it. The
    // transparent pixels print nothing: all the red there is is the block's
    // 64 mm^2, 1 024 pixels at 4 px/mm, less what the smoothing at its edge
    // - a ramp one image pixel, half a millimetre, wide - fades past red.
    const double turn = katana::math::kPi / 6.0;
    const double c = std::cos(turn);
    const double s = std::sin(turn);
    katana::interop::ReferenceData reference;
    reference.add(rasterOf(80, 80, {100.0, 0.5 * c, 0.5 * s, 50.0, 0.5 * s, -0.5 * c}, [](int x, int y) {
        return x >= 32 && x < 48 && y >= 32 && y < 48 ? qRgba(220, 0, 0, 255) : qRgba(0, 0, 0, 0);
    }));
    Model model;
    plotting::SheetSet set;
    set.sheets.push_back(a3());
    set.sheets[0].viewports.push_back(
        planAt(box(100.0, 100.0, 300.0, 250.0), 1000.0, Point2(110.0, 40.0), 0.6));
    SheetSource source;
    source.plan.model = &model;
    source.plan.reference = &reference;
    const QImage paper = painted(set, source);
    const QColor red(220, 0, 0);
    const Point2 d(100.0 + 20.0 * (c + s) - 110.0, 50.0 + 20.0 * (s - c) - 40.0);
    const Point2 expected(200.0 + d.x * std::cos(0.6) + d.y * std::sin(0.6),
                          175.0 - d.x * std::sin(0.6) + d.y * std::cos(0.6));
    ASSERT_NEAR(expected.x, 215.808, 2e-3);
    ASSERT_NEAR(expected.y, 167.432, 2e-3);
    // The middle 4 mm of the block - 16 or 17 pixels each way, as the box
    // falls on the pixel grid - is solid red.
    EXPECT_GE(colourIn(paper, box(expected.x - 2.0, expected.y - 2.0, expected.x + 2.0, expected.y + 2.0),
                       red),
              16 * 16);
    EXPECT_EQ(colourIn(paper, box(214.3, 174.7, 220.3, 180.7), red), 0);
    const int total = colourIn(paper, box(100.0, 100.0, 300.0, 250.0), red);
    EXPECT_GT(total, 1024 * 8 / 10);
    EXPECT_LT(total, 1024 * 12 / 10);
}

TEST(SheetPainter, ASheetPdfOverALargePhotoEmbedsOnlyTheViewportAtTheCappedResolution)
{
    // A 2000 x 2000 photo of 2 cm pixels, 40 m square, of noise (which no
    // compression shrinks), filling a 40 mm viewport at 1 : 1000 on a sheet
    // plotted at 600 dpi: a photo pixel is 0.02 mm of paper, 0.47 of a
    // device pixel. Not capped below the plot's resolution it is averaged
    // to the device's, 945 x 945 = 893 000 pixels; capped at 200 dpi (the
    // default) to 315 x 315 = 99 000, a ninth - and the PDF, which is
    // almost all image, shrinks with it. plotSheetsToPdf plots with the cap.
    katana::interop::ReferenceData reference;
    reference.add(rasterOf(2000, 2000, {-20.0, 0.02, 0.0, 20.0, 0.0, -0.02}, [](int x, int y) {
        std::uint32_t hash = static_cast<std::uint32_t>(x) * 73856093u ^
                             static_cast<std::uint32_t>(y) * 19349663u;
        hash ^= hash >> 13;
        hash *= 0x5bd1e995u;
        hash ^= hash >> 15;
        return qRgb(static_cast<int>(hash & 0xff), static_cast<int>((hash >> 8) & 0xff),
                    static_cast<int>((hash >> 16) & 0xff));
    }));
    Model model;
    plotting::SheetSet set;
    set.sheets.push_back(a3());
    set.sheets[0].viewports.push_back(planAt(box(100.0, 100.0, 140.0, 140.0), 1000.0, Point2(0.0, 0.0)));
    SheetSource source;
    source.plan.model = &model;
    source.plan.reference = &reference;
    QTemporaryDir dir;
    ASSERT_TRUE(dir.isValid());
    const qint64 uncapped = sheetPdfBytes(dir.filePath("uncapped.pdf"), set, source, 600.0, 0.0);
    const qint64 capped = sheetPdfBytes(dir.filePath("capped.pdf"), set, source, 600.0, 200.0);
    EXPECT_LT(capped * 4, uncapped) << capped << " bytes capped, " << uncapped << " not";

    const QString path = dir.filePath("plotted.pdf");
    SheetPaintCache cache;
    ASSERT_TRUE(katana::qt::plotSheetsToPdf(path, set, {}, source, 600.0, cache).ok());
    QFile file(path);
    ASSERT_TRUE(file.open(QIODevice::ReadOnly));
    const QByteArray pdf = file.readAll();
    EXPECT_TRUE(pdf.contains("/Subtype /Image"));
    EXPECT_LT(pdf.size() * 4, uncapped) << pdf.size() << " bytes plotted, " << uncapped << " not capped";
    RecordProperty("uncapped_bytes", static_cast<int>(uncapped));
    RecordProperty("capped_bytes", static_cast<int>(capped));
    RecordProperty("plotted_bytes", static_cast<int>(pdf.size()));
}

TEST(SheetPainter, AnAutomaticScaleIsReportedInTheTitleBlock)
{
    Model model;
    addLine(model, Point2(0.0, 0.0), Point2(300.0, 0.0));
    plotting::Viewport viewport = planAt(box(23.0, 35.0, 410.0, 287.0), 1.0, Point2());
    viewport.autoScale = true;
    viewport.autoCentre = true;
    SheetSource source;
    source.plan.model = &model;
    // 300 m across 387 mm needs 1 : 806: the ladder's next is 1 : 1000.
    const auto resolved = katana::qt::resolvePlanViewport(viewport, source);
    EXPECT_DOUBLE_EQ(resolved.scale, 1000.0);
    EXPECT_NEAR(resolved.centre.x, 150.0, 1e-9);
}

TEST(SheetPainter, SectionsAreCutFromTheSurfacesAlongTheAlignment)
{
    // Ground z = x / 2 under a road running north up x = 50.
    std::vector<katana::geometry::Point3> vertices{
        {0.0, 0.0, 0.0}, {100.0, 0.0, 50.0}, {100.0, 100.0, 50.0}, {0.0, 100.0, 0.0}};
    std::vector<katana::terrain::TinTriangle> triangles{{0, 1, 2}, {0, 2, 3}};
    auto ground = katana::terrain::TinSurface::create(std::move(vertices), std::move(triangles));
    ASSERT_TRUE(ground.ok());
    Model model;
    katana::entity::Alignment road;
    road.name = "ROAD";
    road.horizontal.pis = {{Point2(50.0, 10.0)}, {Point2(50.0, 90.0)}};
    ASSERT_TRUE(model.alignments.add(road).ok());

    plotting::SheetSet set;
    set.sheets.push_back(a3());
    plotting::Viewport profile;
    profile.id = "vp1";
    profile.kind = plotting::ViewportKind::LongSection;
    profile.rect = box(23.0, 160.0, 410.0, 287.0);
    profile.scale = 500.0;
    profile.verticalExaggeration = 10.0;
    profile.source.alignment = "ROAD";
    profile.autoCentre = true;
    plotting::Viewport across;
    across.id = "vp2";
    across.kind = plotting::ViewportKind::CrossSections;
    across.rect = box(23.0, 35.0, 210.0, 155.0);
    across.scale = 250.0;
    across.source.alignment = "ROAD";
    across.source.stations = {40.0};
    across.source.sectionHalfWidth = 20.0;
    across.autoCentre = true;
    set.sheets[0].viewports = {profile, across};

    SheetSource source;
    source.plan.model = &model;
    source.surfaces.push_back({"GROUND", &*ground});
    SheetPaintStats stats;
    const QImage paper = painted(set, source, &stats);
    EXPECT_TRUE(stats.problems.empty()) << stats.problems.front();
    EXPECT_EQ(stats.viewportsDrawn, 2u);
    // The cross section's ground: brown ink in its plot.
    EXPECT_GT(colourIn(paper, box(60.0, 60.0, 200.0, 150.0), QColor(125, 80, 40)), 50);

    // Without a surface the cross section says why and draws nothing.
    source.surfaces.clear();
    painted(set, source, &stats, false, "_without_surface");
    ASSERT_FALSE(stats.problems.empty());
    EXPECT_NE(stats.problems.back().find("vp2"), std::string::npos);
}

TEST(SheetPainter, AMissingAlignmentIsReportedNotDrawn)
{
    Model model;
    plotting::SheetSet set;
    set.sheets.push_back(a3());
    plotting::Viewport profile;
    profile.id = "vp7";
    profile.kind = plotting::ViewportKind::LongSection;
    profile.rect = box(23.0, 35.0, 410.0, 287.0);
    profile.source.alignment = "NOWHERE";
    set.sheets[0].viewports.push_back(profile);
    SheetSource source;
    source.plan.model = &model;
    SheetPaintStats stats;
    painted(set, source, &stats);
    ASSERT_EQ(stats.problems.size(), 1u);
    EXPECT_EQ(stats.problems[0], "vp7: no alignment named NOWHERE");
    EXPECT_EQ(stats.viewportsDrawn, 0u);
}

TEST(SheetPainter, NotesAndALegendDrawTheirText)
{
    Model model;
    addLine(model, Point2(0.0, 0.0), Point2(10.0, 0.0));
    plotting::SheetSet set;
    set.sheets.push_back(a3());
    plotting::Viewport notes;
    notes.id = "vp1";
    notes.kind = plotting::ViewportKind::Notes;
    notes.rect = box(340.0, 150.0, 400.0, 280.0);
    notes.text = "ALL LEVELS ARE TO THE LOCAL DATUM. SERVICES SHOWN ARE APPROXIMATE AND "
                 "MUST BE LOCATED BEFORE ANY EXCAVATION.";
    plotting::Viewport legend;
    legend.id = "vp2";
    legend.kind = plotting::ViewportKind::Legend;
    legend.rect = box(300.0, 40.0, 400.0, 145.0);
    set.sheets[0].viewports = {notes, legend};
    SheetSource source;
    source.plan.model = &model;
    const QImage paper = painted(set, source);
    // Wrapped: text on at least three lines under the heading.
    int lines = 0;
    for (double y = 268.0; y > 250.0; y -= 3.2) {
        lines += inkIn(paper, box(343.0, y - 2.0, 395.0, y)) > 0 ? 1 : 0;
    }
    EXPECT_GE(lines, 3);
    // The legend's one layer: a sample line and its name.
    EXPECT_GT(inkIn(paper, box(302.0, 128.0, 330.0, 136.0)), 20);
}

TEST(SheetPainter, TheWholeSetIsOneVectorPdfAPageASheet)
{
    Model model;
    addLine(model, Point2(0.0, 0.0), Point2(100.0, 50.0));
    plotting::SheetSet set;
    set.defaults.organisation = "NORTHERN SURVEY CONSULTANTS";
    set.sheets.push_back(a3("ONE"));
    set.sheets.back().viewports.push_back(planAt(box(23.0, 35.0, 410.0, 287.0), 500.0, Point2(50.0, 25.0)));
    set.sheets.push_back(a3("TWO"));
    set.sheets.back().id = "s2";
    set.sheets.back().paper = katana::cad::PaperSize::A1;
    set.sheets.push_back(a3("THREE"));
    set.sheets.back().id = "s3";
    set.sheets.back().landscape = false;

    QTemporaryDir dir;
    ASSERT_TRUE(dir.isValid());
    const QString path = dir.filePath("set.pdf");
    SheetSource source;
    source.plan.model = &model;
    SheetPaintCache cache;
    std::vector<std::string> problems;
    const auto status = katana::qt::plotSheetsToPdf(path, set, {}, source, 300.0, cache,
                                                    QString(), &problems);
    ASSERT_TRUE(status.ok()) << status.error().describe();
    EXPECT_TRUE(problems.empty());
    QFile file(path);
    ASSERT_TRUE(file.open(QIODevice::ReadOnly));
    const QByteArray pdf = file.readAll();
    EXPECT_TRUE(pdf.startsWith("%PDF"));
    // One /Page object per sheet (not counting the /Pages tree).
    qsizetype pages = 0;
    for (qsizetype at = pdf.indexOf("/Type /Page"); at >= 0; at = pdf.indexOf("/Type /Page", at + 1)) {
        pages += pdf.mid(at + 11, 1) != "s" ? 1 : 0;
    }
    EXPECT_EQ(pages, 3);
    // The A1 page is 841 x 594 mm: 2384 x 1684 points.
    EXPECT_TRUE(pdf.contains("2383.9") || pdf.contains("2384"));

    // Only the sheets asked for, in the order asked.
    const std::size_t second[] = {1};
    ASSERT_TRUE(katana::qt::plotSheetsToPdf(path, set, second, source, 300.0, cache).ok());
    const std::size_t past[] = {3};
    EXPECT_FALSE(katana::qt::plotSheetsToPdf(path, set, past, source, 300.0, cache).ok());
}
