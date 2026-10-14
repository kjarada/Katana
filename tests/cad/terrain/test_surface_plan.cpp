// A surface in plan, in its style (cad/terrain/surface_plan.hpp,
// docs/surface_display.md "In plan"): culling, the level of detail, the
// faces' image, contours, the rim and breaklines, points and labels - on
// hand-made TINs whose answers are worked out beside each test.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <memory>
#include <string>
#include <vector>

#include "katana/cad/terrain/surface_plan.hpp"
#include "katana/core/task_pool.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace {

using katana::cad::SurfaceDisplayStyle;
using katana::cad::SurfaceFaces;
using katana::cad::SurfacePlan;
using katana::cad::SurfacePlanCache;
using katana::cad::SurfacePlanView;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Point3;
using katana::terrain::NamedSurface;
using katana::terrain::TinSurface;

// The 10 x 10 square from (0, 0), z = x, as two triangles meeting on the
// diagonal from (0, 0) to (10, 10); `breakline` makes that diagonal a
// constrained edge (triangle 0's edge 2, triangle 1's edge 0).
std::shared_ptr<const TinSurface> square(bool breakline = false)
{
    std::vector<std::uint8_t> flags;
    if (breakline) {
        flags = {0b100, 0b001};
    }
    auto tin = TinSurface::create({{0.0, 0.0, 0.0}, {10.0, 0.0, 10.0}, {10.0, 10.0, 10.0}, {0.0, 10.0, 0.0}},
                                  {{0, 1, 2}, {0, 2, 3}}, std::move(flags));
    EXPECT_TRUE(tin.ok());
    return std::make_shared<const TinSurface>(std::move(*tin));
}

// A grid of `columns` x `rows` cells of `cell`, each cut on its diagonal, z
// rolling gently so no two faces are alike.
std::shared_ptr<const TinSurface> grid(int columns, int rows, double cell)
{
    std::vector<Point3> vertices;
    for (int j = 0; j <= rows; ++j) {
        for (int i = 0; i <= columns; ++i) {
            const double x = i * cell;
            const double y = j * cell;
            vertices.emplace_back(x, y, 10.0 + 3.0 * std::sin(x / 17.0) * std::cos(y / 11.0) + 0.05 * x);
        }
    }
    const auto at = [columns](int i, int j) { return static_cast<std::uint32_t>(j * (columns + 1) + i); };
    std::vector<katana::terrain::TinTriangle> triangles;
    for (int j = 0; j < rows; ++j) {
        for (int i = 0; i < columns; ++i) {
            triangles.push_back({at(i, j), at(i + 1, j), at(i + 1, j + 1)});
            triangles.push_back({at(i, j), at(i + 1, j + 1), at(i, j + 1)});
        }
    }
    auto tin = TinSurface::create(std::move(vertices), std::move(triangles));
    EXPECT_TRUE(tin.ok());
    return std::make_shared<const TinSurface>(std::move(*tin));
}

// Looking at `visible` with `unitsPerPixel`, the image on the plan view's
// own grid (planImageGrid, no cap that matters here).
SurfacePlanView viewOf(const Box2& visible, double unitsPerPixel, const Box2& bounds)
{
    SurfacePlanView view;
    view.visible = visible;
    view.unitsPerPixel = unitsPerPixel;
    const auto grid = katana::cad::planImageGrid(visible, bounds, unitsPerPixel, 16'000'000);
    view.imageBox = grid.box;
    view.imageWidth = grid.width;
    view.imageHeight = grid.height;
    return view;
}

// Nothing drawn but what each test turns on.
SurfaceDisplayStyle bare()
{
    SurfaceDisplayStyle style;
    style.name = "bare";
    style.showBoundary = false;
    style.planFaces = SurfaceFaces::None;
    return style;
}

std::uint32_t pixelAt(const SurfacePlan& plan, int column, int row)
{
    return plan.image.pixels[static_cast<std::size_t>(row) * static_cast<std::size_t>(plan.image.width) +
                             static_cast<std::size_t>(column)];
}

constexpr std::uint32_t opaque(std::uint8_t r, std::uint8_t g, std::uint8_t b)
{
    return 0xff000000u | (static_cast<std::uint32_t>(r) << 16) | (static_cast<std::uint32_t>(g) << 8) | b;
}

const Box2 kSquareBox(Point2(0.0, 0.0), Point2(10.0, 10.0));

} // namespace

TEST(SurfacePlanCull, TheGridFindsExactlyTheTrianglesTestingEveryOneFinds)
{
    const auto tin = grid(40, 30, 2.5); // 2 400 triangles over 100 x 75
    SurfacePlanCache cache;
    // A small linear congruential stream (Numerical Recipes' constants), so
    // the boxes are the same every run: inside, across the edge, outside,
    // tiny, thin, and the whole surface.
    std::uint32_t state = 12345;
    const auto next = [&state] {
        state = state * 1664525u + 1013904223u;
        return static_cast<double>(state >> 8) / static_cast<double>(1u << 24);
    };
    std::vector<Box2> boxes{Box2(Point2(-10, -10), Point2(200, 200)), Box2(Point2(50, 30), Point2(50, 30)),
                            Box2(Point2(-5, 10), Point2(-1, 20)), Box2(Point2(2.5, 2.5), Point2(2.5, 70))};
    for (int i = 0; i < 300; ++i) {
        const double x = -20.0 + 140.0 * next();
        const double y = -20.0 + 115.0 * next();
        boxes.emplace_back(Point2(x, y), Point2(x + 40.0 * next() * next(), y + 30.0 * next() * next()));
    }
    std::size_t nonEmpty = 0;
    for (const Box2& box : boxes) {
        const auto culled = katana::cad::planTrianglesInBox(tin, box, cache);
        const auto exhaustive = katana::cad::planTrianglesInBoxExhaustive(*tin, box);
        EXPECT_EQ(culled, exhaustive) << box.min.x << "," << box.min.y << " " << box.max.x << "," << box.max.y;
        nonEmpty += exhaustive.empty() ? 0 : 1;
    }
    // The boxes reach the cases they claim: most find triangles, some none.
    EXPECT_GT(nonEmpty, boxes.size() / 2);
    EXPECT_LT(nonEmpty, boxes.size());
    EXPECT_EQ(cache.gridsBuilt(), 1u);
}

TEST(SurfacePlanTriangles, EachEdgeAtLeastFourPixelsLongIsStrokedOnce)
{
    const auto tin = square();
    SurfacePlanCache cache;
    SurfaceDisplayStyle style = bare();
    style.showTriangles = true;
    // A unit a pixel: sides 10 px, the diagonal 14 px - all stroked, the
    // shared diagonal once: 4 sides and 1 diagonal.
    const SurfacePlan plan = katana::cad::surfacePlan({"ground", tin, "test"}, style,
                                                      viewOf(kSquareBox, 1.0, tin->bounds()), cache);
    EXPECT_EQ(plan.stats.trianglesInView, 2u);
    EXPECT_EQ(plan.triangles.size(), 5u);
    EXPECT_EQ(plan.stats.trianglesTinted, 0u);
    EXPECT_TRUE(plan.image.empty()); // nothing to put in one
}

TEST(SurfacePlanTriangles, TrianglesWhoseEdgesAreUnderFourPixelsAreTintedNotStroked)
{
    const auto tin = square();
    SurfacePlanCache cache;
    SurfaceDisplayStyle style = bare();
    style.showTriangles = true;
    style.triangleColour = {150, 150, 150, 255};
    // Five units a pixel: sides 2 px, the diagonal 2.8 px - all under 4.
    const SurfacePlan plan = katana::cad::surfacePlan({"ground", tin, "test"}, style,
                                                      viewOf(kSquareBox, 5.0, tin->bounds()), cache);
    EXPECT_TRUE(plan.triangles.empty());
    EXPECT_EQ(plan.stats.trianglesTinted, 2u);
    ASSERT_FALSE(plan.image.empty());
    // The tint: the triangle colour at 35 %, alpha lround(0.35 x 255) = 89,
    // each channel (150 x 89 + 127) / 255 = 52 (52.85 truncated).
    EXPECT_EQ(pixelAt(plan, 0, 0), 0x59343434u);
}

TEST(SurfacePlanFaces, AFaceInAKnownBandIsThatBandsColour)
{
    const auto tin = square();
    SurfacePlanCache cache;
    SurfaceDisplayStyle style = bare();
    style.planFaces = SurfaceFaces::Elevation;
    style.planOpacity = 1.0;
    style.banded = true;
    style.bands = 5; // edges at 2, 4, 6, 8 over the surface's own 0 to 10
    style.ramp = "terrain";
    // An eighth of a unit a pixel, exact in binary: 80 x 80 pixels, the
    // centre of column i at x = (i + 0.5) / 8, where z = x.
    const SurfacePlan plan = katana::cad::surfacePlan({"ground", tin, "test"}, style,
                                                      viewOf(kSquareBox, 0.125, tin->bounds()), cache);
    ASSERT_EQ(plan.image.width, 80);
    ASSERT_EQ(plan.image.height, 80);
    // Column 4: x = 0.5625, band 0, the ramp's start (40, 70, 140).
    EXPECT_EQ(pixelAt(plan, 4, 40), opaque(40, 70, 140));
    // Column 28: x = 3.5625, band 1 (2 to 4), the ramp at 1/4: (60, 160, 90).
    EXPECT_EQ(pixelAt(plan, 28, 40), opaque(60, 160, 90));
    // Column 76: x = 9.5625, band 4, the ramp's end (245, 245, 245).
    EXPECT_EQ(pixelAt(plan, 76, 10), opaque(245, 245, 245));
    // Either side of the band edge at x = 2, columns 15 (1.9375) and 16
    // (2.0625), on rows of both triangles.
    for (const int row : {5, 70}) {
        EXPECT_EQ(pixelAt(plan, 15, row), opaque(40, 70, 140));
        EXPECT_EQ(pixelAt(plan, 16, row), opaque(60, 160, 90));
    }
    ASSERT_TRUE(plan.legend.has_value());
    EXPECT_EQ(plan.legend->entries.size(), 5u);
}

TEST(SurfacePlanFaces, EveryPixelOfTheSurfaceIsCoveredOnceEvenOnTheSharedDiagonal)
{
    // At an eighth of a unit a pixel, 80 of the pixel centres lie exactly on
    // the diagonal the two triangles share (i + j = 79 in the image's rows
    // and columns). Each goes to one triangle: no pinholes, none twice - so
    // exactly 80 x 80 covered. Counting a tie for both triangles gives
    // 6 480, for neither 6 320.
    const auto tin = square();
    SurfacePlanCache cache;
    SurfaceDisplayStyle style = bare();
    style.planFaces = SurfaceFaces::Flat;
    const SurfacePlan plan = katana::cad::surfacePlan({"ground", tin, "test"}, style,
                                                      viewOf(kSquareBox, 0.125, tin->bounds()), cache);
    EXPECT_EQ(plan.stats.pixelsCovered, 6400u);
    for (const std::uint32_t pixel : plan.image.pixels) {
        ASSERT_NE(pixel, 0u);
    }
}

TEST(SurfacePlanFaces, OpacityPremultipliesTheFlatColour)
{
    const auto tin = square();
    SurfacePlanCache cache;
    SurfaceDisplayStyle style = bare();
    style.planFaces = SurfaceFaces::Flat;
    style.flatColour = {200, 100, 50, 255};
    style.planOpacity = 0.5;
    const SurfacePlan plan = katana::cad::surfacePlan({"ground", tin, "test"}, style,
                                                      viewOf(kSquareBox, 0.125, tin->bounds()), cache);
    // Alpha lround(127.5) = 128; red (200 x 128 + 127) / 255 = 100, green
    // (100 x 128 + 127) / 255 = 50, blue (50 x 128 + 127) / 255 = 25.
    EXPECT_EQ(pixelAt(plan, 40, 40), 0x80643219u);
}

TEST(SurfacePlanFaces, SlopeAndAspectColourAFaceByItsSteepnessAndDirection)
{
    // z = x rises 1 in 1 (100 %) towards the east, so every face looks west:
    // aspect 270 degrees, sector W.
    const auto tin = square();
    SurfacePlanCache cache;
    SurfaceDisplayStyle style = bare();
    style.planOpacity = 1.0;
    style.planFaces = SurfaceFaces::Aspect;
    const SurfacePlan aspect = katana::cad::surfacePlan({"ground", tin, "test"}, style,
                                                        viewOf(kSquareBox, 0.125, tin->bounds()), cache);
    const auto west = katana::cad::aspectSectorColour(6);
    EXPECT_EQ(pixelAt(aspect, 40, 40), opaque(west.r, west.g, west.b));
    // Slope over 0 to 200 % on the grey ramp: 100 % is its middle, (128,
    // 128, 128).
    style.planFaces = SurfaceFaces::Slope;
    style.ramp = "grey";
    style.rangeMin = 0.0;
    style.rangeMax = 200.0;
    const SurfacePlan slope = katana::cad::surfacePlan({"ground", tin, "test"}, style,
                                                       viewOf(kSquareBox, 0.125, tin->bounds()), cache);
    EXPECT_EQ(pixelAt(slope, 40, 40), opaque(128, 128, 128));
}

TEST(SurfacePlanFaces, TheImageIsTheSameAtAnyThreadCount)
{
    const auto tin = grid(60, 40, 1.7);
    SurfaceDisplayStyle style = bare();
    style.planFaces = SurfaceFaces::Elevation;
    style.banded = true;
    style.showTriangles = true; // dense at this scale: tinted
    const Box2 visible(Point2(-3, -2), Point2(105, 70));
    SurfacePlanView view = viewOf(visible, 0.3, tin->bounds());
    katana::core::TaskPool serial(0);
    katana::core::TaskPool several(3);
    SurfacePlanCache cacheA;
    SurfacePlanCache cacheB;
    view.pool = &serial;
    const SurfacePlan a = katana::cad::surfacePlan({"ground", tin, "test"}, style, view, cacheA);
    view.pool = &several;
    const SurfacePlan b = katana::cad::surfacePlan({"ground", tin, "test"}, style, view, cacheB);
    ASSERT_FALSE(a.image.empty());
    EXPECT_GT(a.stats.pixelsCovered, 0u);
    EXPECT_EQ(a.image.pixels, b.image.pixels);
    EXPECT_EQ(a.stats.pixelsCovered, b.stats.pixelsCovered);
}

TEST(SurfacePlanContours, AKnownLevelRunsWhereTheSurfaceIsAtItAndItsLabelSaysIt)
{
    const auto tin = square();
    SurfacePlanCache cache;
    SurfaceDisplayStyle style = bare();
    style.contours.show = true;
    style.contours.interval = 1.0;
    style.contours.majorEvery = 5;
    style.contours.labelMajor = true;
    // An eighth of a unit a pixel: contours 1 unit apart at slope 1 are 8 px
    // apart, minors drawn; the square is 80 px across.
    SurfacePlanView view = viewOf(Box2(Point2(-2, -2), Point2(12, 12)), 0.125, tin->bounds());
    view.labelSpacingPixels = 1000.0; // one label a contour, half way along
    const SurfacePlan plan = katana::cad::surfacePlan({"ground", tin, "test"}, style, view, cache);
    EXPECT_GE(plan.stats.contourLevels, 9u); // 1 to 9 at least
    EXPECT_FALSE(plan.stats.minorContoursHidden);
    bool sawFive = false;
    bool sawOne = false;
    for (const auto& contour : plan.contours) {
        if (contour.level == 5.0) {
            EXPECT_TRUE(contour.major);
            sawFive = true;
            for (const Point2& p : contour.points) {
                EXPECT_NEAR(p.x, 5.0, 1e-9);
            }
        }
        if (contour.level == 1.0) {
            EXPECT_FALSE(contour.major);
            sawOne = true;
        }
    }
    EXPECT_TRUE(sawFive);
    EXPECT_TRUE(sawOne);
    // The level-5 contour is 10 long (80 px), over the 90 px a label needs?
    // No: 80 < 90, so only the longer majors are labelled - here none.
    EXPECT_TRUE(plan.labels.empty());
}

TEST(SurfacePlanContours, ALongEnoughMajorIsLabelledWithItsLevelAlongIt)
{
    const auto tin = square();
    SurfacePlanCache cache;
    SurfaceDisplayStyle style = bare();
    style.contours.show = true;
    style.contours.interval = 1.0;
    style.contours.majorEvery = 5;
    style.contours.labelMajor = true;
    // A twentieth of a unit a pixel: the level-5 contour is 200 px long; one
    // label half way along it, at (5, 5), the contour running along y. The
    // majors are levels 0, 5 and 10: at 0 the whole surface is at or above
    // the level, so there is no contour (contours.hpp); at 10 the region at
    // or above it is the east edge, whose rim is a contour, labelled at
    // (10, 5). Two labels.
    SurfacePlanView view = viewOf(Box2(Point2(-2, -2), Point2(12, 12)), 0.05, tin->bounds());
    view.labelSpacingPixels = 1000.0;
    const SurfacePlan plan = katana::cad::surfacePlan({"ground", tin, "test"}, style, view, cache);
    ASSERT_EQ(plan.labels.size(), 2u);
    const auto five = std::ranges::find(plan.labels, std::string("5"), &katana::cad::SurfacePlanLabel::text);
    ASSERT_NE(five, plan.labels.end());
    EXPECT_NEAR(five->at.x, 5.0, 1e-9);
    EXPECT_NEAR(five->at.y, 5.0, 1e-9);
    EXPECT_NEAR(std::abs(std::sin(five->angle)), 1.0, 1e-9);
    const auto ten = std::ranges::find(plan.labels, std::string("10"), &katana::cad::SurfacePlanLabel::text);
    ASSERT_NE(ten, plan.labels.end());
    EXPECT_NEAR(ten->at.x, 10.0, 1e-9);
}

TEST(SurfacePlanContours, MinorsCloserThanThreePixelsAreLeftOutAndTheMajorsKept)
{
    const auto tin = square();
    SurfacePlanCache cache;
    SurfaceDisplayStyle style = bare();
    style.contours.show = true;
    style.contours.interval = 1.0;
    style.contours.majorEvery = 5;
    // Half a unit a pixel: minors 2 px apart (hidden), majors 10 px (kept).
    const SurfacePlan plan = katana::cad::surfacePlan({"ground", tin, "test"}, style,
                                                      viewOf(Box2(Point2(-2, -2), Point2(12, 12)), 0.5, tin->bounds()),
                                                      cache);
    EXPECT_TRUE(plan.stats.minorContoursHidden);
    EXPECT_FALSE(plan.stats.majorContoursHidden);
    ASSERT_FALSE(plan.contours.empty());
    for (const auto& contour : plan.contours) {
        EXPECT_TRUE(contour.major);
    }
}

TEST(SurfacePlanContours, ContoursAreTracedOnceAndAgainOnlyWhenTheSettingsChange)
{
    const auto tin = grid(20, 20, 2.0);
    SurfacePlanCache cache;
    SurfaceDisplayStyle style = bare();
    style.contours.show = true;
    style.contours.interval = 0.5;
    const SurfacePlanView view = viewOf(Box2(Point2(0, 0), Point2(40, 40)), 0.1, tin->bounds());
    (void)katana::cad::surfacePlan({"ground", tin, "test"}, style, view, cache);
    (void)katana::cad::surfacePlan({"ground", tin, "test"}, style, view, cache);
    EXPECT_EQ(cache.contourTracings(), 1u);
    style.contours.interval = 0.25;
    (void)katana::cad::surfacePlan({"ground", tin, "test"}, style, view, cache);
    EXPECT_EQ(cache.contourTracings(), 2u);
}

TEST(SurfacePlanContours, AnIntervalFarTooFineIsRefusedForThePlanSayingWhy)
{
    // A millionth of a unit over 10 units of relief: ten million levels
    // cross each triangle, past kPlanMaximumContourPoints.
    const auto tin = square();
    SurfacePlanCache cache;
    SurfaceDisplayStyle style = bare();
    style.contours.show = true;
    style.contours.interval = 1.0e-6;
    const SurfacePlan plan = katana::cad::surfacePlan({"ground", tin, "test"}, style,
                                                      viewOf(kSquareBox, 0.1, tin->bounds()), cache);
    EXPECT_TRUE(plan.contours.empty());
    EXPECT_NE(plan.problem.find("interval"), std::string::npos) << plan.problem;
}

TEST(SurfacePlanLines, TheRimIsOneClosedRingAndABreaklineIsOneLine)
{
    const auto tin = square(true);
    SurfacePlanCache cache;
    SurfaceDisplayStyle style = bare();
    style.showBoundary = true;
    style.showBreaklines = true;
    const SurfacePlan plan = katana::cad::surfacePlan({"ground", tin, "test"}, style,
                                                      viewOf(Box2(Point2(-1, -1), Point2(11, 11)), 0.05,
                                                             tin->bounds()),
                                                      cache);
    ASSERT_EQ(plan.boundary.size(), 1u);
    // Four corners and the first again.
    ASSERT_EQ(plan.boundary.front().size(), 5u);
    EXPECT_EQ(plan.boundary.front().front(), plan.boundary.front().back());
    ASSERT_EQ(plan.breaklines.size(), 1u);
    ASSERT_EQ(plan.breaklines.front().size(), 2u);
    const Point2 a = plan.breaklines.front().front();
    const Point2 b = plan.breaklines.front().back();
    EXPECT_NEAR(std::abs(a.x - b.x), 10.0, 1e-12);
    EXPECT_NEAR(std::abs(a.y - b.y), 10.0, 1e-12);
}

TEST(SurfacePlanPoints, VerticesAreMarksWhenApartAndPixelsWhenDense)
{
    // Four vertices over 100 square units: 5 apart on average.
    const auto tin = square();
    SurfacePlanCache cache;
    SurfaceDisplayStyle style = bare();
    style.showPoints = true;
    const SurfacePlan apart = katana::cad::surfacePlan({"ground", tin, "test"}, style,
                                                       viewOf(Box2(Point2(-1, -1), Point2(11, 11)), 1.0, tin->bounds()),
                                                       cache);
    EXPECT_EQ(apart.stats.pointsMarked, 4u);
    EXPECT_EQ(apart.points.size(), 4u);
    // Two units a pixel: 2.5 px apart, under 4 - pixels in the image. The
    // corners at x = 10 or y = 0 fall on the image's far edges; those on it
    // are counted.
    const SurfacePlan dense = katana::cad::surfacePlan({"ground", tin, "test"}, style,
                                                       viewOf(Box2(Point2(-1, -1), Point2(11, 11)), 2.0, tin->bounds()),
                                                       cache);
    EXPECT_TRUE(dense.points.empty());
    EXPECT_GE(dense.stats.pointsInImage, 1u);
    EXPECT_LE(dense.stats.pointsInImage, 4u);
}

TEST(SurfacePlanView, ASurfaceOutOfViewDrawsNothingAndAnEmptyOneNothingAtAll)
{
    const auto tin = square();
    SurfacePlanCache cache;
    SurfaceDisplayStyle style = bare();
    style.planFaces = SurfaceFaces::Flat;
    style.showBoundary = true;
    const Box2 away(Point2(100, 100), Point2(120, 110));
    const SurfacePlan plan = katana::cad::surfacePlan({"ground", tin, "test"}, style,
                                                      viewOf(away, 0.1, tin->bounds()), cache);
    EXPECT_EQ(plan.stats.trianglesInView, 0u);
    EXPECT_TRUE(plan.image.empty());
    EXPECT_TRUE(plan.boundary.empty());
    const SurfacePlan none = katana::cad::surfacePlan(
        {"empty", std::make_shared<const TinSurface>(), "test"}, style, viewOf(kSquareBox, 0.1, kSquareBox), cache);
    EXPECT_EQ(none.stats.trianglesInView, 0u);
    EXPECT_TRUE(none.image.empty());
}

TEST(SurfacePlanImageGrid, TheGridLinesUpWithTheViewsCornerAndCoversTheSurfaceInView)
{
    const Box2 visible(Point2(0, 0), Point2(100, 50));
    const Box2 bounds(Point2(10.3, 5.2), Point2(30.7, 40.9));
    const auto grid = katana::cad::planImageGrid(visible, bounds, 1.0, 1'000'000);
    // Columns floor(10.3) = 10 to ceil(30.7) = 31; rows down from the top,
    // floor(50 - 40.9) = 9 to ceil(50 - 5.2) = 45.
    EXPECT_EQ(grid.width, 21);
    EXPECT_EQ(grid.height, 36);
    EXPECT_EQ(grid.box.min.x, 10.0);
    EXPECT_EQ(grid.box.max.x, 31.0);
    EXPECT_EQ(grid.box.max.y, 41.0);
    EXPECT_EQ(grid.box.min.y, 5.0);
    // Apart: nothing.
    EXPECT_EQ(katana::cad::planImageGrid(visible, Box2(Point2(200, 0), Point2(300, 10)), 1.0, 100).width, 0);
}

TEST(SurfacePlanImageGrid, AGridOverItsCapIsCoarsenedEvenlyUnderIt)
{
    const Box2 square(Point2(0, 0), Point2(1000, 1000));
    // 10 000 x 10 000 at a tenth: a hundred million pixels, against a cap
    // of a million.
    const auto grid = katana::cad::planImageGrid(square, square, 0.1, 1'000'000);
    const std::size_t pixels = static_cast<std::size_t>(grid.width) * static_cast<std::size_t>(grid.height);
    EXPECT_LE(pixels, 1'000'000u);
    EXPECT_GT(pixels, 900'000u);
    EXPECT_NEAR(static_cast<double>(grid.width), static_cast<double>(grid.height), 1.0);
}
