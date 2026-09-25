// The smart legend on paper (src/katana_qt/plotting/legend_painter): each
// entry's sample drawn as the plan prints it, and the Legend viewport listing
// what the plan beside it shows. Painted into QImages at 4 px a millimetre and
// read back pixel by pixel.
//
// With KATANA_SHEET_PNG set to a directory, every sheet a test paints is also
// written there as a PNG, to be looked at.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <string>

#include <QColor>
#include <QImage>
#include <QPainter>
#include <QRectF>

#include "katana/cad/plot.hpp"
#include "katana/cad/plotting/legend.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/entity/model.hpp"
#include "katana/entity/style_library.hpp"
#include "plotting/legend_painter.hpp"
#include "sheet_painter.hpp"

using katana::entity::Color;
using katana::entity::Entity;
using katana::entity::Model;
using katana::entity::Stroke;
using katana::entity::StrokeOp;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Segment2;
using katana::qt::LegendSampleContext;
using katana::qt::SheetPaintCache;
using katana::qt::SheetPaintOptions;
using katana::qt::SheetSource;
namespace plotting = katana::cad::plotting;

namespace {

constexpr double kPpmm = 4.0; // ~100 dpi
const QRgb kWhite = qRgb(255, 255, 255);
const Color kRed{200, 0, 0, 255};
const Color kBlue{0, 0, 255, 255};

// ---- one sample ---------------------------------------------------------------------

// A sample cell of the legend's 10 x 3.6 mm, centred at pixel (100, 50).
const QRectF kCell(80.0, 42.8, 10.0 * kPpmm, 3.6 * kPpmm);

QImage blank()
{
    QImage image(200, 100, QImage::Format_ARGB32_Premultiplied);
    image.fill(Qt::white);
    return image;
}

LegendSampleContext context(const Model* model = nullptr,
                            const katana::entity::StyleLibrary* library = nullptr)
{
    static const katana::cad::PlotSettings plot;
    LegendSampleContext c;
    c.model = model;
    c.library = library;
    c.plot = &plot;
    c.pixelsPerMillimetre = kPpmm;
    c.scale = 500.0;
    return c;
}

bool paintSample(QImage& image, const plotting::LegendEntry& entry, const LegendSampleContext& c)
{
    QPainter painter(&image);
    painter.setRenderHint(QPainter::Antialiasing);
    const bool drawn = katana::qt::paintLegendSample(painter, entry, kCell, c);
    painter.end();
    return drawn;
}

// The box of the pixels that are not white, and how many there are.
struct Ink {
    int count = 0;
    int left = 1 << 20;
    int right = -1;
    int top = 1 << 20;
    int bottom = -1;
    [[nodiscard]] int width() const { return count == 0 ? 0 : right - left + 1; }
    [[nodiscard]] int height() const { return count == 0 ? 0 : bottom - top + 1; }
};

Ink inkOf(const QImage& image)
{
    Ink ink;
    for (int y = 0; y < image.height(); ++y) {
        for (int x = 0; x < image.width(); ++x) {
            if (image.pixel(x, y) == kWhite) {
                continue;
            }
            ++ink.count;
            ink.left = std::min(ink.left, x);
            ink.right = std::max(ink.right, x);
            ink.top = std::min(ink.top, y);
            ink.bottom = std::max(ink.bottom, y);
        }
    }
    return ink;
}

// Whether pixel `c` is ink of `colour`, whole or antialiased onto the white
// paper: its shortfall from white a share of the colour's own. A 0.25 mm line
// is one pixel at 4 px a millimetre and usually straddles two, each half
// covered, so its pixels are the colour faded, never the colour itself.
bool near(const QColor& c, const QColor& colour)
{
    const double seen[] = {255.0 - c.red(), 255.0 - c.green(), 255.0 - c.blue()};
    const double full[] = {255.0 - colour.red(), 255.0 - colour.green(), 255.0 - colour.blue()};
    const double share =
        std::max({seen[0], seen[1], seen[2]}) / std::max({full[0], full[1], full[2]});
    if (share < 0.25) {
        return false;
    }
    for (int i = 0; i < 3; ++i) {
        if (std::abs(seen[i] - share * full[i]) > 30.0) {
            return false;
        }
    }
    return true;
}

int pixelsNear(const QImage& image, const QColor& colour)
{
    int count = 0;
    for (int y = 0; y < image.height(); ++y) {
        for (int x = 0; x < image.width(); ++x) {
            count += near(image.pixelColor(x, y), colour) ? 1 : 0;
        }
    }
    return count;
}

// Separate runs of inked columns along pixel row `y`: the dashes a line shows.
int runsAlong(const QImage& image, int y)
{
    int runs = 0;
    bool inRun = false;
    for (int x = 0; x < image.width(); ++x) {
        double ink = 0.0;
        for (int row = y - 2; row <= y + 2; ++row) {
            ink += 1.0 - image.pixelColor(x, row).lightnessF();
        }
        const bool inked = ink >= 0.5;
        runs += inked && !inRun ? 1 : 0;
        inRun = inked;
    }
    return runs;
}

plotting::LegendEntry entryOf(plotting::LegendKind kind, Color colour = {})
{
    plotting::LegendEntry entry;
    entry.kind = kind;
    entry.label = "SAMPLE";
    entry.colour = colour;
    return entry;
}

// ---- a whole sheet ------------------------------------------------------------------

plotting::Viewport planAt(std::string id, Box2 rect, double scale, Point2 centre)
{
    plotting::Viewport viewport;
    viewport.id = std::move(id);
    viewport.kind = plotting::ViewportKind::Plan;
    viewport.rect = rect;
    viewport.scale = scale;
    viewport.centre = centre;
    return viewport;
}

plotting::Viewport legendAt(Box2 rect, plotting::LegendScope scope = plotting::LegendScope::ThisSheet)
{
    plotting::Viewport viewport;
    viewport.id = "vpL";
    viewport.kind = plotting::ViewportKind::Legend;
    viewport.rect = rect;
    viewport.legendScope = scope;
    return viewport;
}

Box2 box(double x0, double y0, double x1, double y1) { return Box2(Point2(x0, y0), Point2(x1, y1)); }

void addLayer(Model& model, const std::string& name, Color colour)
{
    katana::entity::Layer layer;
    layer.name = name;
    layer.color = colour;
    ASSERT_TRUE(model.layers.add(layer).ok());
}

void addEntity(Model& model, katana::entity::Geometry geometry, const std::string& layer,
               const std::string& style = {})
{
    Entity entity;
    entity.geometry = std::move(geometry);
    entity.layer = layer;
    entity.style = style;
    ASSERT_TRUE(model.entities.add(std::move(entity)).ok());
}

QImage paintedSheet(const plotting::SheetSet& set, const Model& model, const char* tag = "")
{
    const auto paper = katana::cad::paperDimensions(set.sheets[0].paper, set.sheets[0].landscape);
    QImage image(static_cast<int>(std::lround(paper.widthMm * kPpmm)),
                 static_cast<int>(std::lround(paper.heightMm * kPpmm)),
                 QImage::Format_ARGB32_Premultiplied);
    image.fill(Qt::gray);
    SheetSource source;
    source.plan.model = &model;
    SheetPaintOptions options;
    options.pixelsPerMillimetre = kPpmm;
    SheetPaintCache cache;
    QPainter painter(&image);
    (void)katana::qt::paintSheet(painter, set, 0, source, options, cache);
    painter.end();
    if (const char* dir = std::getenv("KATANA_SHEET_PNG"); dir != nullptr) {
        const auto* test = ::testing::UnitTest::GetInstance()->current_test_info();
        image.save(QString("%1/%2%3.png").arg(dir, test->name(), tag));
    }
    return image;
}

// A paper box (mm, Y up on an A3 landscape sheet) as a device rectangle.
QRect pixelsOf(Box2 paper)
{
    const int left = static_cast<int>(std::floor(paper.min.x * kPpmm));
    const int right = static_cast<int>(std::floor(paper.max.x * kPpmm));
    const int top = static_cast<int>(std::floor((297.0 - paper.max.y) * kPpmm));
    const int bottom = static_cast<int>(std::floor((297.0 - paper.min.y) * kPpmm));
    return QRect(QPoint(left, top), QPoint(right, bottom));
}

int inkIn(const QImage& image, Box2 paper)
{
    const QRect r = pixelsOf(paper);
    int count = 0;
    for (int y = r.top(); y <= r.bottom(); ++y) {
        for (int x = r.left(); x <= r.right(); ++x) {
            count += image.pixel(x, y) != kWhite ? 1 : 0;
        }
    }
    return count;
}

int colourIn(const QImage& image, Box2 paper, const QColor& colour)
{
    const QRect r = pixelsOf(paper);
    int count = 0;
    for (int y = r.top(); y <= r.bottom(); ++y) {
        for (int x = r.left(); x <= r.right(); ++x) {
            count += near(image.pixelColor(x, y), colour) ? 1 : 0;
        }
    }
    return count;
}

// The sample cell of row `row` (from 0) in the first column of a legend
// whose top edge is at `top` (layoutLegend's default measures: rows from
// 8.25 mm under the top, 4.5 mm apart, samples 10 x 3.6 mm from 2.5 mm in).
Box2 sampleCell(double left, double top, int row)
{
    const double middle = top - 8.25 - (row + 0.5) * 4.5;
    return box(left + 2.5, middle - 1.8, left + 12.5, middle + 1.8);
}

} // namespace

// ---- the samples ------------------------------------------------------------------------

TEST(SheetLegendSample, ASymbolDrawsInkInItsCellAtItsPlottedSize)
{
    // A built-in circle 1.5 m across at 1:500 prints 3 mm across: 12 pixels,
    // plus its 0.25 mm pen.
    plotting::LegendEntry entry = entryOf(plotting::LegendKind::Symbol, kRed);
    entry.symbol = "circle";
    entry.symbolSize = 1.5;
    QImage image = blank();
    ASSERT_TRUE(paintSample(image, entry, context()));
    const Ink ink = inkOf(image);
    ASSERT_GT(ink.count, 20);
    EXPECT_TRUE(kCell.adjusted(-1, -1, 1, 1).contains(QRectF(ink.left, ink.top, ink.width(), ink.height())));
    EXPECT_NEAR(ink.width(), 13, 2);
    EXPECT_NEAR(ink.height(), 13, 2);
    // Centred in the cell.
    EXPECT_NEAR(0.5 * (ink.left + ink.right), kCell.center().x(), 1.5);
    EXPECT_NEAR(0.5 * (ink.top + ink.bottom), kCell.center().y(), 1.5);
    // In its own colour.
    EXPECT_GT(pixelsNear(image, QColor(200, 0, 0)), 10);
}

TEST(SheetLegendSample, ASymbolTooBigForItsCellIsShrunkToFitIt)
{
    // 10 m at 1:500 would print 20 mm across, in a cell 3.6 mm high.
    plotting::LegendEntry entry = entryOf(plotting::LegendKind::Symbol);
    entry.symbol = "square";
    entry.symbolSize = 10.0;
    QImage image = blank();
    ASSERT_TRUE(paintSample(image, entry, context()));
    const Ink ink = inkOf(image);
    ASSERT_GT(ink.count, 20);
    EXPECT_LE(ink.height(), static_cast<int>(kCell.height()) + 2);
    EXPECT_GE(ink.height(), static_cast<int>(kCell.height()) - 3);
    EXPECT_LE(ink.width(), ink.height() + 2); // still square
}

TEST(SheetLegendSample, ALibrarySymbolIsDrawnByTheSharedStylePainter)
{
    // A ring of radius 0.5 m on the ground: 2 mm across at 1:500, hollow.
    katana::entity::LineStyle ring;
    ring.name = "TEST Ring";
    ring.atVertices = true;
    ring.strokes = {Stroke{.op = StrokeOp::Move, .point = {0.0, 0.0}},
                    Stroke{.op = StrokeOp::Circle, .radius = 0.5}};
    katana::entity::StyleLibrary library;
    ASSERT_TRUE(library.add(ring).ok());
    plotting::LegendEntry entry = entryOf(plotting::LegendKind::Symbol);
    entry.symbol = "TEST Ring";
    QImage image = blank();
    ASSERT_TRUE(paintSample(image, entry, context(nullptr, &library)));
    const Ink ink = inkOf(image);
    ASSERT_GT(ink.count, 10);
    EXPECT_NEAR(ink.width(), 9, 2);
    // Hollow: the middle of the ring is paper.
    EXPECT_EQ(image.pixel(QPoint(static_cast<int>(0.5 * (ink.left + ink.right)),
                                 static_cast<int>(0.5 * (ink.top + ink.bottom)))),
              kWhite);
}

TEST(SheetLegendSample, ALibraryLinestyleIsDrawnAsItsOwnStrokes)
{
    // A paper linestyle of a 4 mm period: 1.5 on, 0.5 off, 1 on, 1 off. Along
    // the 10 mm sample that is five dashes; a plain line is one.
    katana::entity::LineStyle kerb;
    kerb.name = "TEST Dashed Kerb";
    kerb.units = katana::entity::StyleUnits::Paper;
    kerb.length = 4.0;
    kerb.strokes = {Stroke{.op = StrokeOp::Move, .point = {0.0, 0.0}},
                    Stroke{.op = StrokeOp::Draw, .point = {1.5, 0.0}},
                    Stroke{.op = StrokeOp::Move, .point = {2.0, 0.0}},
                    Stroke{.op = StrokeOp::Draw, .point = {3.0, 0.0}},
                    Stroke{.op = StrokeOp::Move, .point = {4.0, 0.0}}};
    katana::entity::StyleLibrary library;
    ASSERT_TRUE(library.add(kerb).ok());
    plotting::LegendEntry entry = entryOf(plotting::LegendKind::Line);
    entry.linetype = "TEST Dashed Kerb";
    QImage dashed = blank();
    ASSERT_TRUE(paintSample(dashed, entry, context(nullptr, &library)));
    EXPECT_EQ(runsAlong(dashed, 50), 5);

    QImage plain = blank();
    entry.linetype = std::string(katana::entity::kContinuousLinetype);
    ASSERT_TRUE(paintSample(plain, entry, context(nullptr, &library)));
    EXPECT_EQ(runsAlong(plain, 50), 1);
    EXPECT_NEAR(inkOf(plain).width(), 40, 1); // the cell's whole width
}

TEST(SheetLegendSample, APlainLinePrintsInItsPaperColourAndWeight)
{
    // White prints black on paper; 0.5 mm is two pixels at 4 px a millimetre.
    plotting::LegendEntry entry = entryOf(plotting::LegendKind::Line, Color{255, 255, 255, 255});
    entry.lineWeight = 0.5;
    QImage image = blank();
    ASSERT_TRUE(paintSample(image, entry, context()));
    int dark = 0;
    for (int y = 40; y < 60; ++y) {
        dark += image.pixelColor(100, y).lightnessF() < 0.3 ? 1 : 0;
    }
    EXPECT_EQ(dark, 2);

    // At a line weight scale of 2, as the plan beside it is drawn: 1 mm.
    katana::cad::PlotSettings bold;
    bold.lineWeightScale = 2.0;
    LegendSampleContext boldContext = context();
    boldContext.plot = &bold;
    QImage heavy = blank();
    ASSERT_TRUE(paintSample(heavy, entry, boldContext));
    int heavyDark = 0;
    for (int y = 40; y < 60; ++y) {
        heavyDark += heavy.pixelColor(100, y).lightnessF() < 0.3 ? 1 : 0;
    }
    EXPECT_EQ(heavyDark, 4);

    // A red line stays red.
    QImage red = blank();
    ASSERT_TRUE(paintSample(red, entryOf(plotting::LegendKind::Line, kRed), context()));
    EXPECT_GT(pixelsNear(red, QColor(200, 0, 0)), 30);
}

TEST(SheetLegendSample, AnAreaIsASwatchFilledWithItsPattern)
{
    Model model;
    ASSERT_TRUE(model.hatchPatterns.add(katana::entity::HatchPattern{"SOLIDFILL", "", true, {}}).ok());
    plotting::LegendEntry entry = entryOf(plotting::LegendKind::Area, Color{0, 160, 0, 255});
    entry.hatchPattern = "SOLIDFILL";
    QImage image = blank();
    ASSERT_TRUE(paintSample(image, entry, context(&model)));
    const int cell = static_cast<int>(kCell.width() * kCell.height());
    EXPECT_GT(pixelsNear(image, QColor(0, 160, 0)), cell * 6 / 10);
    // Outlined and filled inside the cell, and nowhere else.
    const Ink ink = inkOf(image);
    EXPECT_TRUE(kCell.adjusted(-1, -1, 1, 1).contains(QRectF(ink.left, ink.top, ink.width(), ink.height())));
}

// In monochrome a solid area prints as the plan prints its fill: black or
// paper white by its lightness (cad::paperFillColour), not in the pen's
// black whatever its colour - a light swatch beside a dark one would
// otherwise print the same.
TEST(SheetLegendSample, AnAreasFillFollowsTheMonochromeRuleForFills)
{
    Model model;
    ASSERT_TRUE(model.hatchPatterns.add(katana::entity::HatchPattern{"SOLIDFILL", "", true, {}}).ok());
    katana::cad::PlotSettings mono;
    mono.colourMode = katana::cad::PlotColourMode::Monochrome;
    LegendSampleContext c = context(&model);
    c.plot = &mono;
    const int cell = static_cast<int>(kCell.width() * kCell.height());
    plotting::LegendEntry light = entryOf(plotting::LegendKind::Area, Color{200, 236, 200, 255});
    light.hatchPattern = "SOLIDFILL";
    QImage pale = blank();
    ASSERT_TRUE(paintSample(pale, light, c));
    plotting::LegendEntry dark = entryOf(plotting::LegendKind::Area, Color{0, 60, 0, 255});
    dark.hatchPattern = "SOLIDFILL";
    QImage solid = blank();
    ASSERT_TRUE(paintSample(solid, dark, c));
    EXPECT_GT(pixelsNear(solid, QColor(0, 0, 0)), cell * 6 / 10) << "a dark fill prints solid";
    // The light one keeps its black outline and nothing more.
    EXPECT_LT(pixelsNear(pale, QColor(0, 0, 0)), pixelsNear(solid, QColor(0, 0, 0)) / 2)
        << "a light fill drops out";
}

TEST(SheetLegendSample, APointATextAndAnEmptyCell)
{
    QImage point = blank();
    ASSERT_TRUE(paintSample(point, entryOf(plotting::LegendKind::Point), context()));
    // The plain mark: a cross 2 mm (8 px) each way.
    EXPECT_NEAR(inkOf(point).width(), 9, 2);
    EXPECT_NEAR(inkOf(point).height(), 9, 2);

    QImage text = blank();
    ASSERT_TRUE(paintSample(text, entryOf(plotting::LegendKind::Text), context()));
    EXPECT_GT(inkOf(text).count, 20);

    QImage none = blank();
    QPainter painter(&none);
    EXPECT_FALSE(katana::qt::paintLegendSample(painter, entryOf(plotting::LegendKind::Line),
                                               QRectF(10.0, 10.0, 0.0, 12.0), context()));
    painter.end();
    EXPECT_EQ(inkOf(none).count, 0);
}

// ---- the panel on a sheet -----------------------------------------------------------------

TEST(SheetLegendPainter, TheLegendListsWhatThePlanBesideItShows)
{
    Model model;
    addLayer(model, "PITS", Color{0, 0, 0, 255});
    addLayer(model, "ROAD", kRed);
    addLayer(model, "FAR", kBlue);
    katana::entity::Style pit;
    pit.name = "Pit";
    pit.symbol = "square";
    ASSERT_TRUE(model.styles.add(pit).ok());
    // The plan is 200 x 200 mm at 1:500 about the origin: 100 m each way.
    addEntity(model, katana::entity::PointGeometry{Point2(0.0, 0.0)}, "PITS", "Pit");
    addEntity(model, Segment2{Point2(-20.0, 5.0), Point2(20.0, 5.0)}, "ROAD");
    addEntity(model, Segment2{Point2(500.0, 500.0), Point2(510.0, 500.0)}, "FAR");
    plotting::SheetSet set;
    plotting::Sheet sheet;
    sheet.id = "s1";
    const Box2 legendRect = box(300.0, 40.0, 400.0, 145.0);
    sheet.viewports = {planAt("vp1", box(30.0, 40.0, 230.0, 240.0), 500.0, Point2(0.0, 0.0)),
                       legendAt(legendRect)};
    set.sheets.push_back(sheet);

    // The pit's symbol, then the road; nothing of the far line.
    const QImage own = paintedSheet(set, model, "_sheet");
    EXPECT_GT(inkIn(own, sampleCell(300.0, 145.0, 0)), 10);
    EXPECT_GT(colourIn(own, sampleCell(300.0, 145.0, 1), QColor(200, 0, 0)), 10);
    const Box2 third = sampleCell(300.0, 145.0, 2);
    EXPECT_EQ(inkIn(own, box(301.0, third.min.y, 399.0, third.max.y)), 0);

    // The whole drawing lists the far line too, between them: FAR, then ROAD.
    set.sheets[0].viewports[1].legendScope = plotting::LegendScope::WholeDrawing;
    const QImage drawing = paintedSheet(set, model, "_drawing");
    EXPECT_GT(inkIn(drawing, sampleCell(300.0, 145.0, 0)), 10);
    EXPECT_GT(colourIn(drawing, sampleCell(300.0, 145.0, 1), QColor(0, 0, 255)), 10);
    EXPECT_GT(colourIn(drawing, sampleCell(300.0, 145.0, 2), QColor(200, 0, 0)), 10);
}

TEST(SheetLegendPainter, WhatDoesNotFitIsCountedInTheLastPlace)
{
    // A panel 40 x 60 mm holds one column of ten rows. Thirty blue layers
    // leave nine samples and "+21 more" in the tenth place, in grey; ten
    // layers fill all ten places with samples.
    const auto sheetWith = [](int layers, Model& model) {
        for (int i = 1; i <= layers; ++i) {
            const std::string name = QString("LAYER %1").arg(i, 2, 10, QChar('0')).toStdString();
            addLayer(model, name, kBlue);
            addEntity(model, Segment2{Point2(-10.0, i), Point2(10.0, i)}, name);
        }
        plotting::SheetSet set;
        plotting::Sheet sheet;
        sheet.id = "s1";
        sheet.viewports = {planAt("vp1", box(30.0, 40.0, 230.0, 240.0), 500.0, Point2(0.0, 0.0)),
                           legendAt(box(300.0, 40.0, 340.0, 100.0))};
        set.sheets.push_back(sheet);
        return set;
    };
    Model many;
    const QImage overflowing = paintedSheet(sheetWith(30, many), many, "_30");
    for (int row = 0; row < 9; ++row) {
        EXPECT_GT(colourIn(overflowing, sampleCell(300.0, 100.0, row), QColor(0, 0, 255)), 10)
            << "row " << row;
    }
    const Box2 last = sampleCell(300.0, 100.0, 9);
    EXPECT_GT(inkIn(overflowing, last), 10);
    EXPECT_EQ(colourIn(overflowing, last, QColor(0, 0, 255)), 0);

    Model ten;
    const QImage fitting = paintedSheet(sheetWith(10, ten), ten, "_10");
    EXPECT_GT(colourIn(fitting, last, QColor(0, 0, 255)), 10);
}
