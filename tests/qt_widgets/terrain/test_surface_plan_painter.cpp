// The drawing's surfaces in the plan painter (terrain/surface_plan_painter.hpp,
// plan_painter.hpp; docs/surface_display.md "In plan"): a surface drawn where
// nothing was, in its style, beneath the entities, through the one painter
// the plan view, plots and sheets share. Ink is counted against the same
// paint without the surface; where a count is exact it is the square's own
// pixels, worked out from the frame, never a previous run's.

#include <gtest/gtest.h>

#include <QColor>
#include <QImage>
#include <QPainter>

#include <memory>

#include "katana/cad/document.hpp"
#include "katana/cad/plot.hpp"
#include "katana/cad/terrain/surface_style.hpp"
#include "katana/entity/model.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "plan_painter.hpp"

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
