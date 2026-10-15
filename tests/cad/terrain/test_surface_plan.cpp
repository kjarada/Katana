// A surface in plan, in its style (cad/terrain/surface_plan.hpp,
// docs/surface_display.md "In plan"): culling, the level of detail, the
// faces' image, contours, the rim and breaklines, points and labels - on
// hand-made TINs whose answers are worked out beside each test.

#include <gtest/gtest.h>

#include <algorithm>
#include <array>
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

namespace {

// A fan of four triangles about the centre (4, 4), in the square from
// (0.25, 0.25) to (7.75, 7.75): every coordinate exact in binary, so the
// centre lands exactly on a pixel centre. `number` says which vertex index
// each point has: [0] the centre, then [1..4] the rim's south-west,
// south-east, north-east and north-west corners. The triangles are (centre,
// A, B) (centre, B, C) (centre, C, D) (centre, D, A), counter-clockwise.
std::shared_ptr<const TinSurface> fan(const std::array<std::uint32_t, 5>& number)
{
    std::vector<Point3> vertices(5);
    vertices[number[0]] = {4.0, 4.0, 12.0};
    vertices[number[1]] = {0.25, 0.25, 10.0};
    vertices[number[2]] = {7.75, 0.25, 10.0};
    vertices[number[3]] = {7.75, 7.75, 14.0};
    vertices[number[4]] = {0.25, 7.75, 14.0};
    const auto triangle = [&number](int a, int b) {
        return katana::terrain::TinTriangle{number[0], number[static_cast<std::size_t>(a)],
                                            number[static_cast<std::size_t>(b)]};
    };
    auto tin = TinSurface::create(std::move(vertices),
                                  {triangle(1, 2), triangle(2, 3), triangle(3, 4), triangle(4, 1)});
    EXPECT_TRUE(tin.ok());
    return std::make_shared<const TinSurface>(std::move(*tin));
}

} // namespace

TEST(SurfacePlanFaces, AVertexOnAPixelCentreIsOneTrianglesWhateverTheVertexNumbersAre)
{
    // Looking at -0.5 .. 7.5 a unit a pixel, the pixel centres are the whole
    // numbers 0 to 7 each way, and the fan's centre (4, 4) is exactly one -
    // as are the centres on the four lines from it to the rim's corners
    // ((3, 3), (5, 3), (5, 5) ...), which each go to one of the two triangles
    // sharing the line. The rim, 0.25 to 7.75, takes the centres 1 to 7 each
    // way: 7 x 7 = 49, each covered once.
    //
    // The rule that decided a tie by vertex numbers gave a centre on a
    // vertex to a triangle only if the vertex was numbered between its two
    // neighbours there: with the centre numbered first (0) or last (4) every
    // neighbour is above, or below, it, and no triangle had the pixel - 48
    // covered, a pinhole; with the rim numbered 0, 3, 1, 4 about a centre
    // numbered 2 the neighbours alternate and two triangles had it - 50 writes
    // for 49 pixels. (Worked out by running the old rule over these four
    // numberings by hand: 48, 48, 50 writes (49 pixels), 49.)
    const Box2 looking(Point2(-0.5, -0.5), Point2(7.5, 7.5));
    const std::array<std::array<std::uint32_t, 5>, 4> numberings{{
        {0, 1, 2, 3, 4}, // the centre below every neighbour
        {4, 0, 1, 2, 3}, // the centre above every neighbour
        {2, 0, 3, 1, 4}, // neighbours 0, 3, 1, 4 about it: above, below, above, below
        {2, 0, 1, 3, 4}, // neighbours 0, 1, 3, 4: one run below, one above (the old rule's good case)
    }};
    for (const auto& number : numberings) {
        const auto tin = fan(number);
        SurfacePlanCache cache;
        SurfaceDisplayStyle style = bare();
        style.planFaces = SurfaceFaces::Flat;
        const SurfacePlan plan = katana::cad::surfacePlan({"fan", tin, "test"}, style,
                                                          viewOf(looking, 1.0, tin->bounds()), cache);
        ASSERT_EQ(plan.image.width, 8);
        ASSERT_EQ(plan.image.height, 8);
        EXPECT_EQ(plan.stats.pixelsCovered, 49u) << "centre numbered " << number[0];
        // Pixel (i, j) has its centre at x = i, y = 7 - j.
        for (int j = 0; j < 8; ++j) {
            for (int i = 0; i < 8; ++i) {
                const bool inside = i >= 1 && j <= 6;
                EXPECT_EQ(pixelAt(plan, i, j) != 0u, inside)
                    << "pixel " << i << "," << j << " centre numbered " << number[0];
            }
        }
    }
}

TEST(SurfacePlanCache, MoreSurfacesInViewThanAnyCapAreKeptNotRebuiltOnEveryPaint)
{
    // A paint asks for every visible surface, in the same order, each time.
    // A cache of the 32 most recently used evicted, for a 33rd surface, the
    // very one the next paint asked for first: every paint built every cull
    // grid and traced every contour again (61-69 ms a repaint with 33
    // surfaces against 3-7 ms with 32, measured in the window).
    constexpr int kSurfaces = 33;
    std::vector<std::shared_ptr<const TinSurface>> surfaces;
    for (int i = 0; i < kSurfaces; ++i) {
        surfaces.push_back(grid(3, 3, 3.0)); // a new surface, at its own address
    }
    SurfacePlanCache cache;
    SurfaceDisplayStyle style = bare();
    style.contours.show = true;
    style.contours.interval = 0.5;
    const SurfacePlanView view = viewOf(Box2(Point2(0, 0), Point2(9, 9)), 0.05, surfaces.front()->bounds());
    for (int pass = 0; pass < 3; ++pass) {
        for (int i = 0; i < kSurfaces; ++i) {
            (void)katana::cad::surfacePlan({"s" + std::to_string(i), surfaces[static_cast<std::size_t>(i)], "test"},
                                           style, view, cache);
        }
        // One grid and one tracing a surface, the first time only.
        EXPECT_EQ(cache.gridsBuilt(), 33u) << "after paint " << pass + 1;
        EXPECT_EQ(cache.contourTracings(), 33u) << "after paint " << pass + 1;
        EXPECT_EQ(cache.surfaceCount(), 33u);
    }
    // The caller bounds the cache: retain() lets go of all but those named.
    std::vector<const TinSurface*> alive;
    for (int i = 0; i < 10; ++i) {
        alive.push_back(surfaces[static_cast<std::size_t>(i)].get());
    }
    cache.retain(alive);
    EXPECT_EQ(cache.surfaceCount(), 10u);
}

TEST(SurfacePlanFaces, ARangeAboveTheWholeSurfaceColoursEveryFaceTheFirstBandNotTheMiddleOne)
{
    // The square z = x is 0 to 10; min=40 puts the range's low end at 40 and
    // its top (the surface's 10) below it, a range given back to front: the
    // point 40. Every height is below the point - the first band, the ramp's
    // start (40, 70, 140) - and the middle band (235, 220, 130) drew all of it.
    const auto tin = square();
    SurfacePlanCache cache;
    SurfaceDisplayStyle style = bare();
    style.planFaces = SurfaceFaces::Elevation;
    style.planOpacity = 1.0;
    style.banded = true;
    style.bands = 5;
    style.rangeMin = 40.0;
    const SurfacePlan plan = katana::cad::surfacePlan({"ground", tin, "test"}, style,
                                                      viewOf(kSquareBox, 0.125, tin->bounds()), cache);
    ASSERT_EQ(plan.image.width, 80);
    EXPECT_EQ(pixelAt(plan, 4, 40), opaque(40, 70, 140));
    EXPECT_EQ(pixelAt(plan, 76, 10), opaque(40, 70, 140));
}

TEST(SurfacePlanFaces, ALevelSurfaceColouredBySlopeIsTheRampsStartNotItsMiddle)
{
    // Slope's range is 0 to the steepest face, and the steepest face of a
    // level pad is 0: a point. A face with no steepness is the start of the
    // slope ramp, green (0, 255, 38), as the 3D view drew it before styles -
    // not the ramp's middle, yellow (255, 255, 38), which reads as half way
    // to the steep end.
    auto level = TinSurface::create({{0.0, 0.0, 5.0}, {10.0, 0.0, 5.0}, {10.0, 10.0, 5.0}, {0.0, 10.0, 5.0}},
                                    {{0, 1, 2}, {0, 2, 3}});
    ASSERT_TRUE(level.ok());
    const auto tin = std::make_shared<const TinSurface>(std::move(*level));
    SurfacePlanCache cache;
    SurfaceDisplayStyle style = bare();
    style.planFaces = SurfaceFaces::Slope;
    style.planOpacity = 1.0;
    style.ramp = "slope";
    const SurfacePlan plan = katana::cad::surfacePlan({"pad", tin, "test"}, style,
                                                      viewOf(kSquareBox, 0.125, tin->bounds()), cache);
    ASSERT_EQ(plan.image.width, 80);
    EXPECT_EQ(pixelAt(plan, 40, 40), opaque(0, 255, 38));
}

// ---- why a style could not be drawn ------------------------------------------------------------

TEST(SurfaceProblems, ContoursAreRefusedExactlyWhenTheirCrossingsPassTheLimit)
{
    // The square is z = x from 0 to 10, so on each of its two triangles the
    // levels of an interval i cross floor(10 / i) times (base 0, both triangles
    // run from 0 to 10): 2 floor(10 / i) points in all, against the limit of
    // 4 000 000. i = 6e-6: 1 666 666 each, 3 333 332 points - drawn. i = 4e-6:
    // 2 500 000 each, 5 000 000 points - refused. (The two sit a fifth either
    // side of the limit, clear of the rounding of 10 / i.)
    const auto tin = square();
    katana::cad::SurfaceContours contours;
    contours.interval = 6.0e-6;
    EXPECT_EQ(katana::cad::contourRefusal(*tin, contours), "");
    contours.interval = 4.0e-6;
    EXPECT_EQ(katana::cad::contourRefusal(*tin, contours),
              "contours: an interval of 0.000004 crosses this surface's triangles more than "
              "4000000 times; not drawn - set a larger interval");
    // The ordinary interval of the built-in Contours style is no trouble.
    contours.interval = 1.0;
    EXPECT_EQ(katana::cad::contourRefusal(*tin, contours), "");
}

TEST(SurfaceProblems, AnIntervalThatIsNoPositiveNumberIsRefusedWhateverTheSurface)
{
    // kGeometric is 1e-7: an interval at or under it is no interval. Not
    // finite is none either, nor is a base that is not finite.
    const auto tin = square();
    const std::string expected = "contours: the interval must be a finite number greater than zero";
    for (const double interval : {0.0, -1.0, 1.0e-8, std::nan(""), HUGE_VAL}) {
        katana::cad::SurfaceContours contours;
        contours.interval = interval;
        EXPECT_EQ(katana::cad::contourRefusal(*tin, contours), expected) << interval;
    }
    katana::cad::SurfaceContours badBase;
    badBase.interval = 1.0;
    badBase.base = std::nan("");
    EXPECT_EQ(katana::cad::contourRefusal(*tin, badBase), expected);
}

TEST(SurfaceProblems, ALevelSurfaceHasNoContourToCrossSoNoIntervalIsTooFine)
{
    // Every vertex at 5: no triangle spans two levels whatever the interval.
    auto level = TinSurface::create({{0.0, 0.0, 5.0}, {10.0, 0.0, 5.0}, {10.0, 10.0, 5.0}, {0.0, 10.0, 5.0}},
                                    {{0, 1, 2}, {0, 2, 3}});
    ASSERT_TRUE(level.ok());
    katana::cad::SurfaceContours contours;
    contours.interval = 1.0e-6;
    EXPECT_EQ(katana::cad::contourRefusal(*level, contours), "");
}

TEST(SurfaceProblems, ThePlanAndTheCheckGiveOneAnswer)
{
    // What a paint puts in SurfacePlan::problem is contourRefusal's answer,
    // word for word: one decision, said by the plan view and found before a
    // plot.
    const auto tin = square();
    SurfacePlanCache cache;
    SurfaceDisplayStyle style = bare();
    style.contours.show = true;
    style.contours.interval = 1.0e-6;
    const SurfacePlan plan = katana::cad::surfacePlan({"ground", tin, "test"}, style,
                                                      viewOf(kSquareBox, 0.1, tin->bounds()), cache);
    EXPECT_FALSE(plan.problem.empty());
    EXPECT_EQ(plan.problem, katana::cad::contourRefusal(*tin, style.contours));
}

TEST(SurfaceProblems, AProblemReadsAsALineForAPersonAndARecordForAnAgent)
{
    const katana::cad::SurfaceProblem problem{"ground", "Fine", "contours: too fine"};
    EXPECT_EQ(problem.text(), "ground (style Fine): contours: too fine");
    EXPECT_EQ(problem.record(), "warning surface=\"ground\" style=\"Fine\" reason=\"contours: too fine\"");
    // A blank in a name and a quote in the reason are quoted and escaped, so
    // the record still splits into its three fields.
    const katana::cad::SurfaceProblem odd{"east lot", "Two \"bands\"", "said \"no\""};
    EXPECT_EQ(odd.record(),
              "warning surface=\"east lot\" style=\"Two \\\"bands\\\"\" reason=\"said \\\"no\\\"\"");
}

TEST(SurfaceProblems, TheStylesOfTheDrawingsVisibleSurfacesAreCheckedWithoutPaintingAnything)
{
    // Fine: contours every millionth of a unit, which the 10-unit relief of the
    // square makes 10 000 000 crossings a triangle. Three surfaces use it or
    // not: "first" and "second" in it, drawn, in the order added; "hidden" in
    // it but hidden, so not drawn; "plain" in the built-in Contours (every
    // metre), which can be traced; "outline" in Standard, which shows no
    // contours at all.
    const auto tin = square();
    katana::cad::SurfaceStyleSet styles = katana::cad::builtInSurfaceStyles();
    SurfaceDisplayStyle fine = bare();
    fine.name = "Fine";
    fine.contours.show = true;
    fine.contours.interval = 1.0e-6;
    styles.styles.push_back(fine);
    katana::terrain::SurfaceStore store;
    ASSERT_TRUE(store.add(NamedSurface{"first", tin, "test", "Fine", true}).ok());
    ASSERT_TRUE(store.add(NamedSurface{"hidden", tin, "test", "Fine", false}).ok());
    ASSERT_TRUE(store.add(NamedSurface{"plain", tin, "test", "Contours", true}).ok());
    ASSERT_TRUE(store.add(NamedSurface{"outline", tin, "test", "Standard", true}).ok());
    ASSERT_TRUE(store.add(NamedSurface{"second", tin, "test", "Fine", true}).ok());

    const auto problems = katana::cad::surfaceStyleProblems(store, styles);
    ASSERT_EQ(problems.size(), 2u);
    const std::string reason = katana::cad::contourRefusal(*tin, fine.contours);
    ASSERT_FALSE(reason.empty());
    EXPECT_EQ(problems[0].problem, (katana::cad::SurfaceProblem{"first", "Fine", reason}));
    EXPECT_EQ(problems[1].problem, (katana::cad::SurfaceProblem{"second", "Fine", reason}));
    EXPECT_EQ(problems[0].bounds.min.x, 0.0);
    EXPECT_EQ(problems[0].bounds.max.x, 10.0);
    EXPECT_EQ(problems[0].bounds.max.y, 10.0);

    // No surfaces, no problems; the surface mended, none.
    EXPECT_TRUE(katana::cad::surfaceStyleProblems(katana::terrain::SurfaceStore{}, styles).empty());
    styles.styles.back().contours.interval = 1.0;
    EXPECT_TRUE(katana::cad::surfaceStyleProblems(store, styles).empty());
}
