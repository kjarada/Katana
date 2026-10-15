// The drawing's surfaces in the plan painter (terrain/surface_plan_painter.hpp,
// plan_painter.hpp; docs/surface_display.md "In plan"): a surface drawn where
// nothing was, in its style, beneath the entities, through the one painter
// the plan view, plots and sheets share. Ink is counted against the same
// paint without the surface; where a count is exact it is the square's own
// pixels, worked out from the frame, never a previous run's.

#include <gtest/gtest.h>

#include <QColor>
#include <QFont>
#include <QFontMetricsF>
#include <QImage>
#include <QPainter>

#include <memory>

#include "katana/cad/document.hpp"
#include "katana/cad/plot.hpp"
#include "katana/cad/terrain/surface_colouring.hpp"
#include "katana/cad/terrain/surface_style.hpp"
#include "katana/entity/model.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "plan_painter.hpp"
#include "terrain/surface_plan_painter.hpp"

namespace {

using katana::cad::SurfaceDisplayStyle;
using katana::cad::SurfaceFaces;
using katana::cad::SurfaceStyleSet;
using katana::geometry::Point2;
using katana::qt::PlanFrame;
using katana::qt::PlanPaintCache;
using katana::qt::PlanPaintOptions;
using katana::qt::PlanPaintStats;
using katana::qt::PlanSource;
using katana::terrain::NamedSurface;
using katana::terrain::SurfaceStore;
using katana::terrain::TinSurface;

constexpr QRgb kBlack = 0xff000000u;

// The 10 x 10 square z = x, two triangles.
std::shared_ptr<const TinSurface> square()
{
    auto tin = TinSurface::create({{0.0, 0.0, 0.0}, {10.0, 0.0, 10.0}, {10.0, 10.0, 10.0}, {0.0, 10.0, 0.0}},
                                  {{0, 1, 2}, {0, 2, 3}});
    EXPECT_TRUE(tin.ok());
    return std::make_shared<const TinSurface>(std::move(*tin));
}

SurfaceDisplayStyle bare(const std::string& name)
{
    SurfaceDisplayStyle style;
    style.name = name;
    style.showBoundary = false;
    style.planFaces = SurfaceFaces::None;
    style.contours.show = false;
    return style;
}

// "Bands": opaque elevation faces in five bands of 2; "Outline": the rim
// alone; "Contours": contours every 1, no faces; "Triangles": the edges.
SurfaceStyleSet styles()
{
    SurfaceStyleSet set;
    SurfaceDisplayStyle bands = bare("Bands");
    bands.planFaces = SurfaceFaces::Elevation;
    bands.banded = true;
    bands.bands = 5;
    bands.planOpacity = 1.0;
    SurfaceDisplayStyle outline = bare("Outline");
    outline.showBoundary = true;
    SurfaceDisplayStyle contours = bare("Contours");
    contours.contours.show = true;
    contours.contours.interval = 1.0;
    SurfaceDisplayStyle triangles = bare("Triangles");
    triangles.showTriangles = true;
    set.styles = {bands, outline, contours, triangles};
    set.defaultStyle = "Bands";
    return set;
}

// An eighth of a unit a pixel, exact in binary: the square's corner (0, 10)
// lands on pixel (8, 8) of a 96 x 96 frame about (5, 5), and the square
// covers pixels 8 to 87 each way - 80 x 80.
PlanFrame frame()
{
    PlanFrame f;
    f.transform.resize(96.0, 96.0);
    f.transform.scale = 8.0;
    f.transform.center = Point2(5.0, 5.0);
    return f;
}

struct Painted {
    QImage image;
    PlanPaintStats stats;
};

Painted paint(const SurfaceStore* surfaces, const SurfaceStyleSet& set, const PlanPaintOptions& options = {},
              PlanPaintCache* cache = nullptr)
{
    static const katana::entity::Model empty;
    PlanSource source;
    source.model = &empty;
    source.surfaces = surfaces;
    source.surfaceStyles = &set;
    Painted painted;
    painted.image = QImage(96, 96, QImage::Format_ARGB32_Premultiplied);
    painted.image.fill(kBlack);
    PlanPaintCache local;
    QPainter painter(&painted.image);
    painted.stats = katana::qt::paintPlan(painter, source, frame(), options, cache != nullptr ? *cache : local);
    painter.end();
    return painted;
}

SurfaceStore storeIn(const std::string& style, bool visible = true)
{
    SurfaceStore store;
    EXPECT_TRUE(store.add(NamedSurface{"ground", square(), "test", style, visible}).ok());
    return store;
}

std::size_t differing(const QImage& a, const QImage& b)
{
    std::size_t count = 0;
    for (int y = 0; y < a.height(); ++y) {
        for (int x = 0; x < a.width(); ++x) {
            count += a.pixel(x, y) != b.pixel(x, y) ? 1u : 0u;
        }
    }
    return count;
}

} // namespace

TEST(SurfacePlanPainter, ASurfaceInItsStyleInksExactlyItsOwnPixelsWhereNothingWasBefore)
{
    PlanPaintOptions options;
    options.surfaceLegends = false; // screen furniture, tested on its own below
    const SurfaceStyleSet set = styles();
    const SurfaceStore store = storeIn("Bands");
    const Painted without = paint(nullptr, set, options);
    const Painted with = paint(&store, set, options);
    // Opaque faces over the square's 80 x 80 pixels, each a device pixel of
    // the image, laid on whole pixels: every one changed, and nothing else.
    EXPECT_EQ(with.stats.surfacesDrawn, 1u);
    EXPECT_EQ(with.stats.surfaceTrianglesInView, 2u);
    EXPECT_EQ(with.stats.surfacePixelsCovered, 6400u);
    std::size_t inside = 0;
    for (int y = 8; y < 88; ++y) {
        for (int x = 8; x < 88; ++x) {
            inside += with.image.pixel(x, y) != without.image.pixel(x, y) ? 1u : 0u;
        }
    }
    EXPECT_EQ(inside, 6400u);
    EXPECT_EQ(differing(with.image, without.image), 6400u);
}

TEST(SurfacePlanPainter, AFaceInAKnownBandIsDrawnInThatBandsColour)
{
    PlanPaintOptions options;
    options.surfaceLegends = false;
    const SurfaceStyleSet set = styles();
    const SurfaceStore store = storeIn("Bands");
    const Painted with = paint(&store, set, options);
    // Pixel column 8 + 4 is at x = 4.5 / 8 = 0.5625: band 0, the terrain
    // ramp's start (40, 70, 140). Column 8 + 28, x = 3.5625: band 1, the
    // ramp at a quarter, (60, 160, 90). Column 8 + 76: band 4, (245, 245, 245).
    EXPECT_EQ(QColor(with.image.pixel(12, 48)), QColor(40, 70, 140));
    EXPECT_EQ(QColor(with.image.pixel(36, 48)), QColor(60, 160, 90));
    EXPECT_EQ(QColor(with.image.pixel(84, 20)), QColor(245, 245, 245));
}

TEST(SurfacePlanPainter, EachStyleInksItsOwnWay)
{
    PlanPaintOptions options;
    options.surfaceLegends = false;
    const SurfaceStyleSet set = styles();
    const Painted baseline = paint(nullptr, set, options);
    std::size_t ink[4] = {};
    const char* names[4] = {"Bands", "Outline", "Contours", "Triangles"};
    for (int i = 0; i < 4; ++i) {
        const SurfaceStore store = storeIn(names[i]);
        ink[i] = differing(paint(&store, set, options).image, baseline.image);
        EXPECT_GT(ink[i], 0u) << names[i];
    }
    // Faces cover the square; lines only cross it.
    EXPECT_GT(ink[0], ink[1]);
    EXPECT_GT(ink[0], ink[2]);
    EXPECT_GT(ink[0], ink[3]);
    // Nine minor contours across the square ink more than its rim.
    EXPECT_GT(ink[2], ink[1]);
}

TEST(SurfacePlanPainter, AHiddenSurfaceDrawsNothingAndIsNotInTheExtents)
{
    const SurfaceStyleSet set = styles();
    const SurfaceStore hidden = storeIn("Bands", false);
    const Painted baseline = paint(nullptr, set);
    const Painted with = paint(&hidden, set);
    EXPECT_EQ(differing(with.image, baseline.image), 0u);
    EXPECT_EQ(with.stats.surfacesDrawn, 0u);

    static const katana::entity::Model empty;
    PlanSource source;
    source.model = &empty;
    source.surfaceStyles = &set;
    source.surfaces = &hidden;
    EXPECT_TRUE(katana::qt::planDrawnBounds(source, {}, {}).empty());
    const SurfaceStore shown = storeIn("Bands");
    source.surfaces = &shown;
    const auto bounds = katana::qt::planDrawnBounds(source, {}, {});
    EXPECT_EQ(bounds.min, Point2(0.0, 0.0));
    EXPECT_EQ(bounds.max, Point2(10.0, 10.0));
}

TEST(SurfacePlanPainter, TheLegendIsScreenFurnitureAndNeverPlotted)
{
    const SurfaceStyleSet set = styles();
    const SurfaceStore store = storeIn("Bands");
    EXPECT_EQ(paint(&store, set).stats.surfaceLegends, 1u);
    katana::cad::PlotSettings plot;
    PlanPaintOptions paper;
    paper.medium = katana::qt::PlanMedium::Paper;
    paper.pixelsPerMillimetre = 4.0;
    paper.plot = &plot;
    const Painted plotted = paint(&store, set, paper);
    EXPECT_EQ(plotted.stats.surfacesDrawn, 1u);
    EXPECT_EQ(plotted.stats.surfaceLegends, 0u);
    // A style without faces has no legend on screen either.
    const SurfaceStore outline = storeIn("Outline");
    EXPECT_EQ(paint(&outline, set).stats.surfaceLegends, 0u);
}

TEST(SurfacePlanPainter, TheStatisticsSayWhatWasStroked)
{
    const SurfaceStyleSet set = styles();
    const SurfaceStore store = storeIn("Triangles");
    // At 8 px a unit the sides are 80 px and the diagonal 113: all five
    // edges stroked, the shared diagonal once.
    const Painted with = paint(&store, set);
    EXPECT_EQ(with.stats.surfaceEdgesStroked, 5u);
    EXPECT_EQ(with.stats.surfaceTrianglesTinted, 0u);
}

TEST(SurfacePlanPainter, ContoursAreTracedOnceAcrossPaintsThroughOneCache)
{
    const SurfaceStyleSet set = styles();
    const SurfaceStore store = storeIn("Contours");
    PlanPaintCache cache;
    (void)paint(&store, set, {}, &cache);
    (void)paint(&store, set, {}, &cache);
    EXPECT_EQ(cache.surfaceCache().contourTracings(), 1u);
    EXPECT_EQ(cache.surfaceCache().surfaceCount(), 1u);
    // A surface gone from the drawing is let go at the next paint.
    const SurfaceStore none;
    (void)paint(&none, set, {}, &cache);
    EXPECT_EQ(cache.surfaceCache().surfaceCount(), 0u);
}

TEST(SurfacePlanPainter, TheDocumentsSurfacesAndStylesAreWhatPlanSourceOfHands)
{
    katana::cad::Document document;
    ASSERT_TRUE(document.setSurfaces(storeIn(std::string{})).ok());
    const PlanSource source = katana::qt::planSourceOf(document);
    EXPECT_EQ(source.surfaces, &document.surfaces());
    EXPECT_EQ(source.surfaceStyles, &document.surfaceStyles());
}

namespace {

// The part of a pixel's colour that is not alpha, as a QColor.
QColor inkAt(const QImage& image, int x, int y) { return QColor(image.pixel(x, y)); }

int channelGap(const QColor& a, const QColor& b)
{
    return std::max({std::abs(a.red() - b.red()), std::abs(a.green() - b.green()),
                     std::abs(a.blue() - b.blue())});
}

} // namespace

TEST(SurfacePlanPainter, ASmoothLegendPlacesEachStopAtItsValueNotAtItsPlaceInTheList)
{
    // The built-in Cut Fill style: stops at -2, -1, -0.5, 0, 0.5, 1 and 2,
    // not evenly spaced, drawn as one bar labelled -2 at the foot and 2 at the
    // head. A stop is at (value + 2) / 4 of the way up: -1 a quarter, 0 half,
    // +1 three quarters. Placed by its place in the list (i / 6) the -1 stop
    // was at 0.167 and the +1 at 0.833: a reader of the labelled ends took a
    // 1 m fill (#EF8A62) for 1.33 m.
    const auto set = katana::cad::builtInSurfaceStyles();
    const katana::cad::SurfaceDisplayStyle* cutFill = set.find("Cut Fill");
    ASSERT_NE(cutFill, nullptr);
    const auto colouring = katana::cad::surfaceColouring(*cutFill, katana::cad::SurfaceFaces::Elevation, -2.0, 2.0);
    const katana::cad::SurfaceLegend legend = katana::cad::surfaceLegend(colouring);
    ASSERT_TRUE(legend.gradient);
    ASSERT_EQ(legend.entries.size(), 7u);

    QFont font;
    font.setPixelSize(11);
    QImage image(240, 240, QImage::Format_ARGB32_Premultiplied);
    image.fill(Qt::black);
    QPainter painter(&image);
    const QRectF panel = katana::qt::paintSurfaceLegend(painter, legend, "diff - Cut Fill", QPointF(230.0, 10.0),
                                                        font, Qt::white, QColor(0, 0, 0));
    painter.end();
    // The bar, as paintSurfaceLegend lays it: 6 px in from the panel's left,
    // below the caption and the title and 4 px; six lines tall; as wide as a
    // swatch.
    const QFontMetricsF metrics(font);
    const double line = metrics.height();
    const double swatch = std::max(10.0, line * 0.8);
    const double barTop = panel.top() + 6.0 + 2.0 * line + 4.0;
    const double barHeight = 6.0 * line;
    const int x = static_cast<int>(panel.left() + 6.0 + 0.5 * swatch);
    // The pixel row whose centre is nearest `fraction` of the way up the bar.
    const auto at = [&](double fraction) {
        return inkAt(image, x, static_cast<int>(std::floor(barTop + barHeight - fraction * barHeight)));
    };
    // The stops' own colours (the style's, ColorBrewer's seven-class RdBu):
    // -1 #67A9CF, 0 #F7F7F7, +1 #EF8A62. A pixel centre is within half a pixel
    // of the stop's place and the gradient moves about ten levels a pixel
    // here, so fourteen levels is the width of "at" - and the old place, a
    // sixth of the bar from the stop, was fifty levels off.
    EXPECT_LE(channelGap(at(0.25), QColor(0x67, 0xA9, 0xCF)), 14) << at(0.25).name().toStdString();
    EXPECT_LE(channelGap(at(0.50), QColor(0xF7, 0xF7, 0xF7)), 14) << at(0.50).name().toStdString();
    EXPECT_LE(channelGap(at(0.75), QColor(0xEF, 0x8A, 0x62)), 14) << at(0.75).name().toStdString();
    // The ends: -2 #2166AC at the foot, +2 #B2182B at the head.
    EXPECT_LE(channelGap(at(0.02), QColor(0x21, 0x66, 0xAC)), 14) << at(0.02).name().toStdString();
    EXPECT_LE(channelGap(at(0.98), QColor(0xB2, 0x18, 0x2B)), 14) << at(0.98).name().toStdString();
}

TEST(SurfacePlanPainter, ALegendTooTallForTheViewBelowTheOnesAlreadyDrawnIsLeftOutNotCutOff)
{
    // Three surfaces in a style with a five-band legend of height h, stacked
    // down from the top-right corner with 10 px between them. The view is 2 h
    // + 50 tall: the first legend ends at 10 + h, the second starts at 20 + h
    // and ends at 20 + 2 h, and a third would start at 30 + 2 h inside the
    // view (its foot margin is at 2 h + 40) and end at 30 + 3 h, far past it.
    // It was drawn, counted and its lowest rows lost.
    const SurfaceStyleSet set = styles();
    SurfaceStore store;
    for (const char* name : {"a", "b", "c"}) {
        ASSERT_TRUE(store.add(NamedSurface{name, square(), "test", "Bands", true}).ok());
    }
    QFont font(PlanPaintOptions{}.fontFamily);
    font.setPixelSize(11);
    const auto colouring = katana::cad::surfaceColouring(*set.find("Bands"), katana::cad::SurfaceFaces::Elevation, 0.0, 10.0);
    const double h = katana::qt::measureSurfaceLegend(katana::cad::surfaceLegend(colouring), "a - Bands", font).height();
    ASSERT_GT(h, 40.0);
    const double height = 2.0 * h + 50.0;
    PlanFrame f;
    f.transform.resize(300.0, height);
    f.transform.scale = 8.0;
    f.transform.center = Point2(5.0, 5.0);
    static const katana::entity::Model empty;
    PlanSource source;
    source.model = &empty;
    source.surfaces = &store;
    source.surfaceStyles = &set;
    QImage image(300, static_cast<int>(height), QImage::Format_ARGB32_Premultiplied);
    image.fill(kBlack);
    PlanPaintCache cache;
    QPainter painter(&image);
    const PlanPaintStats stats = katana::qt::paintPlan(painter, source, f, PlanPaintOptions{}, cache);
    painter.end();
    EXPECT_EQ(stats.surfacesDrawn, 3u);
    EXPECT_EQ(stats.surfaceLegends, 2u);
}

TEST(SurfacePlanPainter, AStyleWhoseContoursAreRefusedSaysWhyInTheStatisticsNotNowhere)
{
    // An interval of a millionth over 10 units of relief crosses each
    // triangle ten million times, past the four million points the plan
    // allows: the contours are not drawn, and the paint's statistics say so -
    // the plan view logs it once, the headless report prints it.
    SurfaceStyleSet set = styles();
    SurfaceDisplayStyle fine = bare("Fine");
    fine.contours.show = true;
    fine.contours.interval = 1.0e-6;
    set.styles.push_back(fine);
    const SurfaceStore refused = storeIn("Fine");
    const Painted painted = paint(&refused, set);
    ASSERT_EQ(painted.stats.surfaceProblems.size(), 1u);
    EXPECT_NE(painted.stats.surfaceProblems.front().find("ground: contours"), std::string::npos)
        << painted.stats.surfaceProblems.front();
    EXPECT_NE(painted.stats.surfaceProblems.front().find("interval"), std::string::npos);
    // A style whose contours are drawn says nothing.
    const SurfaceStore fine1 = storeIn("Contours");
    EXPECT_TRUE(paint(&fine1, set).stats.surfaceProblems.empty());
}
