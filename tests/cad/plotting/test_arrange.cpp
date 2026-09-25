// Arranging a sheet, and the paper, scale and rotation advice
// (include/katana/cad/plotting/arrange.hpp).
//
// The rectangles are worked by hand from the A3 frame's tiling area -
// 24..409 x 36..286 mm, 385 x 250 - and, for the other sizes, from the A3
// drawing area 23..410 x 35..287 scaled by the frame's factor (A4 210 / 297,
// A2 420 / 297) and inset by 1 mm; a portrait or frameless sheet is the paper
// less 10 mm, inset by 1. The rotations are solved by hand in the comments.

#include <gtest/gtest.h>

#include <cmath>
#include <numbers>
#include <string>
#include <vector>

#include "katana/cad/plotting/arrange.hpp"
#include "katana/cad/plotting/layout.hpp"

using katana::cad::PaperSize;
using namespace katana::cad::plotting;
using katana::core::ErrorCode;
using katana::geometry::Vec2;

namespace {

constexpr double kPi = std::numbers::pi;
constexpr double kDegree = kPi / 180.0;

void expectBox(const Box2& box, double x0, double y0, double x1, double y1, double tolerance = 1e-9)
{
    EXPECT_NEAR(box.min.x, x0, tolerance);
    EXPECT_NEAR(box.min.y, y0, tolerance);
    EXPECT_NEAR(box.max.x, x1, tolerance);
    EXPECT_NEAR(box.max.y, y1, tolerance);
}

// A `width` x `height` rectangle's corners, turned `turn` about its centre.
std::vector<Point2> turnedRectangle(Point2 centre, double width, double height, double turn)
{
    std::vector<Point2> corners;
    for (const Vec2 corner : {Vec2(-width / 2, -height / 2), Vec2(width / 2, -height / 2),
                              Vec2(width / 2, height / 2), Vec2(-width / 2, height / 2)}) {
        corners.push_back(centre + corner.rotated(turn));
    }
    return corners;
}

Viewport viewportAt(std::string id, ViewportKind kind, Box2 rect, bool locked = false)
{
    Viewport viewport;
    viewport.id = std::move(id);
    viewport.kind = kind;
    viewport.rect = rect;
    viewport.locked = locked;
    return viewport;
}

const Viewport& byId(const Sheet& sheet, std::string_view id)
{
    for (const Viewport& viewport : sheet.viewports) {
        if (viewport.id == id) {
            return viewport;
        }
    }
    ADD_FAILURE() << "no viewport " << id;
    return sheet.viewports.front();
}

bool overlap(const Box2& a, const Box2& b)
{
    return a.min.x < b.max.x - 1e-9 && b.min.x < a.max.x - 1e-9 && a.min.y < b.max.y - 1e-9 &&
           b.min.y < a.max.y - 1e-9;
}

// No two placed viewports of `sheet` overlap, and every one is on the
// drawing area.
void expectTidy(const Sheet& sheet)
{
    const Box2 area = drawingArea(sheet);
    for (std::size_t i = 0; i < sheet.viewports.size(); ++i) {
        const Box2& a = sheet.viewports[i].rect;
        EXPECT_TRUE(area.contains(a)) << sheet.viewports[i].id << " is off the drawing area";
        for (std::size_t j = i + 1; j < sheet.viewports.size(); ++j) {
            EXPECT_FALSE(overlap(a, sheet.viewports[j].rect))
                << sheet.viewports[i].id << " overlaps " << sheet.viewports[j].id;
        }
    }
}

double areaOf(const Box2& box)
{
    return box.width() * box.height();
}

// The A3 tiling area as a size.
constexpr SizeMm kA3Room{385.0, 250.0};

} // namespace

// ---- The best rotation --------------------------------------------------------------

TEST(SheetArrange, ARectangleTurnedThirtyDegreesIsTurnedBackToLieAcrossThePaper)
{
    // 300 x 100 m lying along 30 degrees: square to it, it needs
    // 1 : 300000 / 385 = 779.22 across and 100000 / 250 = 400 up. Any other
    // turn widens it (the width grows at 100 m a radian either way), and a
    // quarter turn needs 1 : 1200.
    const auto content = turnedRectangle(Point2(1000.0, 2000.0), 300.0, 100.0, 30.0 * kDegree);
    const auto fit = bestFitRotation(content, kA3Room);
    ASSERT_TRUE(fit.ok()) << fit.error().describe();
    EXPECT_NEAR(fit->rotation, 30.0 * kDegree, 1e-9);
    EXPECT_NEAR(fit->width, 300.0, 1e-9);
    EXPECT_NEAR(fit->height, 100.0, 1e-9);
    EXPECT_NEAR(fit->scale, 300000.0 / 385.0, 1e-6);
    EXPECT_EQ(fit->standardScale, 1000.0);
    EXPECT_NEAR(fit->centre.x, 1000.0, 1e-9);
    EXPECT_NEAR(fit->centre.y, 2000.0, 1e-9);

    // Turned a half turn further it needs the same room upside down, and the
    // answer stays within a quarter turn of north up: 120 - 180 = -60.
    const auto again = bestFitRotation(
        turnedRectangle(Point2(0.0, 0.0), 300.0, 100.0, 120.0 * kDegree), kA3Room);
    ASSERT_TRUE(again.ok());
    EXPECT_NEAR(again->rotation, -60.0 * kDegree, 1e-9);
    EXPECT_NEAR(again->scale, 300000.0 / 385.0, 1e-6);
}

TEST(SheetArrange, AThinTriangleFitsBestWhereItsWidthAndHeightAskTheSameScale)
{
    // (0, 0), (100, 10), (100, -10) is 100 m long and 20 m high: 1 : 259.7
    // square. Turned by t, its width is 100 cos t + 10 sin t (to the
    // vertex at (100, 10)) and its height 100 sin t + 10 cos t (from the
    // origin down to (100, -10)). Both ask the same scale where
    // 250 (100 c + 10 s) = 385 (100 s + 10 c): tan t = 21150 / 36000 =
    // 0.5875, t = 30.43 degrees, at 1 : 237.1. Mirrored, -30.43 is as good;
    // the positive turn wins the tie.
    const std::vector<Point2> triangle{Point2(0.0, 0.0), Point2(100.0, 10.0), Point2(100.0, -10.0)};
    const auto fit = bestFitRotation(triangle, kA3Room);
    ASSERT_TRUE(fit.ok()) << fit.error().describe();
    const double t = std::atan(0.5875);
    EXPECT_NEAR(fit->rotation, t, 1e-9);
    EXPECT_NEAR(fit->scale, (100.0 * std::cos(t) + 10.0 * std::sin(t)) * 1000.0 / 385.0, 1e-6);
    EXPECT_LT(fit->scale, 100.0 * 1000.0 / 385.0);
    EXPECT_EQ(fit->standardScale, 250.0);
}

TEST(SheetArrange, ALineFitsAlongTheRectanglesDiagonal)
{
    // A line as long as the tiling area's diagonal, sqrt(385^2 + 250^2) =
    // 459.05 m, fills it corner to corner at exactly 1 : 1000, turned by
    // atan(250 / 385) = 33.0 degrees either way; the positive way wins.
    const double diagonal = std::hypot(385.0, 250.0);
    const std::vector<Point2> line{Point2(0.0, 0.0), Point2(diagonal, 0.0)};
    const auto fit = bestFitRotation(line, kA3Room);
    ASSERT_TRUE(fit.ok()) << fit.error().describe();
    EXPECT_NEAR(fit->rotation, std::atan(250.0 / 385.0), 1e-9);
    EXPECT_NEAR(fit->scale, 1000.0, 1e-6);
    EXPECT_EQ(fit->standardScale, 1000.0);
}

TEST(SheetArrange, ARotationWithinADegreeOfSquareIsSquared)
{
    // Half a degree off east: snapped to 0, at the scale that costs.
    const auto nearly =
        bestFitRotation(turnedRectangle(Point2(0.0, 0.0), 300.0, 100.0, 0.5 * kDegree), kA3Room);
    ASSERT_TRUE(nearly.ok());
    EXPECT_EQ(nearly->rotation, 0.0);
    EXPECT_GT(nearly->scale, 300000.0 / 385.0);
    // Half a degree off north, either side: snapped to a quarter turn.
    for (const double turn : {89.5, -89.5, 90.4}) {
        const auto fit =
            bestFitRotation(turnedRectangle(Point2(0.0, 0.0), 300.0, 100.0, turn * kDegree), kA3Room);
        ASSERT_TRUE(fit.ok());
        EXPECT_EQ(fit->rotation, kPi / 2.0) << turn;
    }
    // A square in a square room is as good square as turned a quarter: 0.
    const auto square = bestFitRotation(turnedRectangle(Point2(5.0, 5.0), 10.0, 10.0, 0.0), {100.0, 100.0});
    ASSERT_TRUE(square.ok());
    EXPECT_EQ(square->rotation, 0.0);
    // Two degrees off is not snapped.
    const auto off =
        bestFitRotation(turnedRectangle(Point2(0.0, 0.0), 300.0, 100.0, 2.0 * kDegree), kA3Room);
    ASSERT_TRUE(off.ok());
    EXPECT_NEAR(off->rotation, 2.0 * kDegree, 1e-9);
}

TEST(SheetArrange, TheLeastRotationToFitIsTheTurnNearestNorthUpThatHolds)
{
    // The 300 x 100 m rectangle at 30 degrees. Square to the paper its box is
    // 309.8 x 236.6 m: 1 : 946.4 - so 1 : 1000 needs no turn.
    const auto content = turnedRectangle(Point2(0.0, 0.0), 300.0, 100.0, 30.0 * kDegree);
    const auto square = leastRotationToFit(content, kA3Room, 1000.0);
    ASSERT_TRUE(square.ok());
    EXPECT_EQ(square->rotation, 0.0);

    // 1 : 900 allows 225 m up. Turned by t, the height is twice the corner
    // (150, 50)'s reach up the paper, 50 cos(t - 30) - 150 sin(t - 30):
    // sqrt(25000) cos(t - 30 + atan(150 / 50)) = 112.5, so t = 30 -
    // atan2(150, 50) + acos(112.5 / sqrt(25000)) = 3.08 degrees.
    const auto turned = leastRotationToFit(content, kA3Room, 900.0);
    ASSERT_TRUE(turned.ok()) << turned.error().describe();
    const double t = 30.0 * kDegree - std::atan2(150.0, 50.0) + std::acos(112.5 / std::sqrt(25000.0));
    EXPECT_NEAR(turned->rotation, t, 1e-9);
    EXPECT_NEAR(turned->scale, 900.0, 1e-6);
    // Nothing nearer north up fits.
    for (int i = -99; i <= 99; ++i) {
        const auto nearer = fitAtRotation(content, kA3Room, t * i / 100.0);
        ASSERT_TRUE(nearer.ok());
        EXPECT_GT(nearer->scale, 900.0) << i;
    }

    // Exactly the best scale fits only at the best rotation; a larger scale
    // fits at none, and says what the best rotation needs.
    const auto exact = leastRotationToFit(content, kA3Room, 300000.0 / 385.0);
    ASSERT_TRUE(exact.ok());
    EXPECT_NEAR(exact->rotation, 30.0 * kDegree, 1e-6);
    const auto none = leastRotationToFit(content, kA3Room, 700.0);
    ASSERT_FALSE(none.ok());
    EXPECT_EQ(none.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(none.error().message.find("1:1000"), std::string::npos) << none.error().message;
}

TEST(SheetArrange, TheStandardRotationTurnsOnlyWhenThatBuysALargerScale)
{
    // The 300 x 100 m rectangle at 30 degrees fits 1 : 779 turned, but
    // 1 : 1000 is the standard scale either way, and square it is 1 : 946:
    // it is not turned.
    const auto rectangle = bestStandardRotation(
        turnedRectangle(Point2(0.0, 0.0), 300.0, 100.0, 30.0 * kDegree), kA3Room);
    ASSERT_TRUE(rectangle.ok());
    EXPECT_EQ(rectangle->rotation, 0.0);
    EXPECT_EQ(rectangle->standardScale, 1000.0);

    // A 450 x 20 m strip at 45 degrees: square, its box is 332.3 m each way,
    // 1 : 1329, so 1 : 2000; along it, 450000 / 385 = 1 : 1168.8, so 1 : 1250.
    // Turned by t the height is 2 (10 cos a - 225 sin a), a = t - 45, which
    // 1 : 1250 allows up to 312.5: t = 45 - atan2(225, 10) +
    // acos(156.25 / sqrt(225^2 + 10^2)) = 3.62 degrees, the least turn that
    // reaches 1 : 1250.
    const auto strip = turnedRectangle(Point2(0.0, 0.0), 450.0, 20.0, 45.0 * kDegree);
    const auto fit = bestStandardRotation(strip, kA3Room);
    ASSERT_TRUE(fit.ok());
    const double t = 45.0 * kDegree - std::atan2(225.0, 10.0) +
                     std::acos(156.25 / std::hypot(225.0, 10.0));
    EXPECT_NEAR(fit->rotation, t, 1e-9);
    EXPECT_EQ(fit->standardScale, 1250.0);
    EXPECT_LE(fit->scale, 1250.0 + 1e-6);
}

TEST(SheetArrange, ContentWithNoSizeOrARoomWithNoneIsRefused)
{
    const std::vector<Point2> point{Point2(1.0, 1.0), Point2(1.0, 1.0)};
    EXPECT_EQ(bestFitRotation(point, kA3Room).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(bestFitRotation(std::vector<Point2>{}, kA3Room).error().code,
              ErrorCode::InvalidArgument);
    const std::vector<Point2> nan{Point2(0.0, 0.0), Point2(std::nan(""), 1.0)};
    EXPECT_EQ(fitAtRotation(nan, kA3Room, 0.0).error().code, ErrorCode::InvalidArgument);
    const std::vector<Point2> line{Point2(0.0, 0.0), Point2(10.0, 0.0)};
    EXPECT_EQ(bestFitRotation(line, {0.0, 10.0}).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(leastRotationToFit(line, kA3Room, 0.0).error().code, ErrorCode::InvalidArgument);
    // A line along an axis has a size: its length.
    const auto along = fitAtRotation(line, {100.0, 50.0}, 0.0);
    ASSERT_TRUE(along.ok());
    EXPECT_NEAR(along->scale, 100.0, 1e-9);
}

TEST(SheetArrange, TheDrawnOutlineIsTheHullOfWhatTheViewDraws)
{
    katana::entity::Model model;
    ASSERT_TRUE(model.layers.add(katana::entity::Layer{.name = "far"}).ok());
    const auto add = [&model](katana::entity::Geometry geometry, std::string layer) {
        katana::entity::Entity entity;
        entity.geometry = std::move(geometry);
        entity.layer = std::move(layer);
        ASSERT_TRUE(model.entities.add(entity).ok());
    };
    add(katana::geometry::Segment2{Point2(0.0, 0.0), Point2(100.0, 0.0)}, "0");
    add(katana::geometry::Circle2{Point2(50.0, 50.0), 10.0}, "0");
    add(katana::geometry::Segment2{Point2(1000.0, 1000.0), Point2(2000.0, 1000.0)}, "far");

    katana::cad::LayerOverrides view;
    EXPECT_TRUE(view.hide("far"));
    const std::vector<Point2> outline = drawnOutline(model, view);
    Box2 box;
    for (const Point2& point : outline) {
        box.expand(point);
    }
    // The line and the circle, traced a 32nd of a turn apart - its top and
    // sides are traced points - and nothing of the hidden layer.
    expectBox(box, 0.0, 0.0, 100.0, 60.0, 1e-9);
    // Shown, the far layer counts.
    const std::vector<Point2> all = drawnOutline(model, katana::cad::LayerOverrides{});
    Box2 allBox;
    for (const Point2& point : all) {
        allBox.expand(point);
    }
    EXPECT_EQ(allBox.max, Point2(2000.0, 1000.0));
    EXPECT_TRUE(drawnOutline(katana::entity::Model{}, view).empty());
}

TEST(SheetArrange, TheDrawnOutlineHoldsTheWholeOfACurve)
{
    // A circle of radius 10 is traced by a polygon round it: 32 corners at
    // 10 / cos(5.625 degrees) = 10.0484 out. Its width is never less than the
    // circle's 20 in any direction - the chords alone are 19.90 across at
    // their narrowest - and never more than the corners' 20.0968.
    katana::entity::Model model;
    katana::entity::Entity circle;
    circle.geometry = katana::geometry::Circle2{Point2(500.0, 500.0), 10.0};
    circle.layer = "0";
    ASSERT_TRUE(model.entities.add(circle).ok());
    const std::vector<Point2> outline = drawnOutline(model, katana::cad::LayerOverrides{});
    const double corner = 10.0 / std::cos(kPi / 32.0);
    for (int degrees = 0; degrees < 180; ++degrees) {
        const auto fit = fitAtRotation(outline, {1.0, 1.0}, degrees * kDegree);
        ASSERT_TRUE(fit.ok());
        EXPECT_GE(fit->width, 20.0 - 1e-9) << degrees;
        EXPECT_LE(fit->width, 2.0 * corner + 1e-9) << degrees;
    }
    // A quarter arc from east to north is held too, its ends exactly.
    katana::entity::Model quarter;
    katana::entity::Entity arc;
    arc.geometry = katana::geometry::Arc2{Point2(0.0, 0.0), 100.0, 0.0, kPi / 2.0};
    arc.layer = "0";
    ASSERT_TRUE(quarter.entities.add(arc).ok());
    const std::vector<Point2> hull = drawnOutline(quarter, katana::cad::LayerOverrides{});
    Box2 box;
    for (const Point2& point : hull) {
        box.expand(point);
    }
    expectBox(box, 0.0, 0.0, 100.0, 100.0, 1e-9);
    // Across the diagonal the arc reaches 100 from its centre; the hull does too.
    const auto diagonal = fitAtRotation(hull, {1.0, 1.0}, kPi / 4.0);
    ASSERT_TRUE(diagonal.ok());
    EXPECT_GE(diagonal->centre.dot(Vec2(std::sqrt(0.5), std::sqrt(0.5))) + diagonal->width / 2.0,
              100.0 - 1e-9);
}

TEST(SheetArrange, AViewShowsItsStretchOfItsAlignmentOrTheDrawing)
{
    katana::entity::Model model;
    katana::entity::Alignment road;
    road.name = "ROAD";
    road.horizontal.pis = {{Point2(0.0, 0.0)}, {Point2(1000.0, 0.0)}};
    ASSERT_TRUE(model.alignments.add(road).ok());
    katana::entity::Entity line;
    line.geometry = katana::geometry::Segment2{Point2(0.0, -50.0), Point2(10.0, 40.0)};
    line.layer = "0";
    ASSERT_TRUE(model.entities.add(line).ok());
    const auto boxOf = [](const std::vector<Point2>& points) {
        Box2 box;
        for (const Point2& point : points) {
            box.expand(point);
        }
        return box;
    };

    // A strip from CH 100 to 300: that stretch and nothing else.
    Viewport strip = viewportAt("vp1", ViewportKind::Plan, Box2(Point2(30, 40), Point2(200, 200)));
    strip.source.alignment = "ROAD";
    strip.source.chainageFrom = 100.0;
    strip.source.chainageTo = 300.0;
    expectBox(boxOf(viewportContent(model, strip)), 100.0, 0.0, 300.0, 0.0);
    // No range: the whole alignment; a range past its end stops at the end.
    strip.source.chainageTo = 0.0;
    strip.source.chainageFrom = 0.0;
    expectBox(boxOf(viewportContent(model, strip)), 0.0, 0.0, 1000.0, 0.0);
    strip.source.chainageFrom = 900.0;
    strip.source.chainageTo = 1500.0;
    expectBox(boxOf(viewportContent(model, strip)), 900.0, 0.0, 1000.0, 0.0);
    // A range wholly past the end shows none of the alignment: the drawing,
    // as the painter then fits it.
    strip.source.chainageFrom = 1200.0;
    strip.source.chainageTo = 1500.0;
    expectBox(boxOf(viewportContent(model, strip)), 0.0, -50.0, 1000.0, 40.0);

    // A plan of the drawing: the line and the alignment. An alignment the
    // model does not have leaves the drawing.
    Viewport plan = viewportAt("vp2", ViewportKind::Plan, Box2(Point2(30, 40), Point2(200, 200)));
    expectBox(boxOf(viewportContent(model, plan)), 0.0, -50.0, 1000.0, 40.0);
    plan.source.alignment = "NOWHERE";
    expectBox(boxOf(viewportContent(model, plan)), 0.0, -50.0, 1000.0, 40.0);

    // A key plan's stored outlines are stale copies the painter no longer
    // draws: alone, it shows the drawing.
    Viewport key = viewportAt("vp3", ViewportKind::KeyPlan, Box2(Point2(30, 40), Point2(120, 110)));
    WorldMark outline;
    outline.kind = WorldMark::Kind::SheetOutline;
    outline.points = {Point2(-100.0, -100.0), Point2(1200.0, -100.0), Point2(1200.0, 300.0)};
    key.marks.push_back(outline);
    expectBox(boxOf(viewportContent(model, key)), 0.0, -50.0, 1000.0, 40.0);
    // In a set, it shows the live outlines of the sheets' plans, as the
    // painter fits it: here one plan, 100 x 50 mm at 1:1000 about (2000, 0).
    SheetSet set;
    Sheet planSheet;
    planSheet.id = "s1";
    Viewport far = viewportAt("vp6", ViewportKind::Plan, Box2(Point2(0, 0), Point2(100, 50)));
    far.scale = 1000.0;
    far.centre = Point2(2000.0, 0.0);
    far.autoScale = false;
    far.autoCentre = false;
    planSheet.viewports = {far};
    Sheet keySheet;
    keySheet.id = "s2";
    keySheet.viewports = {key};
    set.sheets = {planSheet, keySheet};
    expectBox(boxOf(viewportContent(model, set, key)), 1950.0, -25.0, 2050.0, 25.0);

    // Nothing for a view that is not a plan.
    EXPECT_TRUE(viewportContent(model, viewportAt("vp4", ViewportKind::Legend, {})).empty());
    EXPECT_TRUE(viewportContent(model, viewportAt("vp5", ViewportKind::LongSection, {})).empty());
}

// ---- Paper and scale ----------------------------------------------------------------

TEST(SheetArrange, TheSmallestSheetThatHoldsTheContentIsSuggested)
{
    // A4 landscape: its tiling area is 387 x 252 x 210 / 297, less 2:
    // 271.64 x 176.18 mm. 100 x 60 m at 1 : 500 is 200 x 120 mm: A4.
    const double a4 = 210.0 / 297.0;
    PaperRequest request;
    request.scale = 500.0;
    const auto small = suggestPaper(Box2(Point2(0.0, 0.0), Point2(100.0, 60.0)), request);
    ASSERT_TRUE(small.ok()) << small.error().describe();
    EXPECT_EQ(small->paper, PaperSize::A4);
    EXPECT_TRUE(small->landscape);
    EXPECT_EQ(small->frame, kBuiltInFrameId);
    expectBox(small->area, 23.0 * a4 + 1.0, 35.0 * a4 + 1.0, 410.0 * a4 - 1.0, 287.0 * a4 - 1.0);
    EXPECT_NEAR(small->fill, 200.0 / (387.0 * a4 - 2.0), 1e-9);

    // 200 x 100 m is 400 x 200 mm: wider than A3 landscape (385), A3
    // portrait (297 - 22 = 275) and A4; A2 landscape is 387 x 420 / 297 - 2 =
    // 545.27 wide and 354.36 high.
    const auto larger = suggestPaper(Box2(Point2(0.0, 0.0), Point2(200.0, 100.0)), request);
    ASSERT_TRUE(larger.ok());
    EXPECT_EQ(larger->paper, PaperSize::A2);
    EXPECT_TRUE(larger->landscape);

    // 100 x 300 m at 1 : 1000 is 300 mm high: taller than A4 portrait (297 -
    // 22 = 275) and A3 landscape (250); A3 portrait is 275 x 398 - and has no
    // frame.
    request.scale = 1000.0;
    const Box2 tall(Point2(0.0, 0.0), Point2(100.0, 300.0));
    const auto portrait = suggestPaper(tall, request);
    ASSERT_TRUE(portrait.ok());
    EXPECT_EQ(portrait->paper, PaperSize::A3);
    EXPECT_FALSE(portrait->landscape);
    EXPECT_TRUE(portrait->frame.empty());
    expectBox(portrait->area, 11.0, 11.0, 286.0, 409.0);

    // Turned a quarter, it runs 300 mm across: A3 landscape.
    request.rotation = kPi / 2.0;
    const auto turned = suggestPaper(tall, request);
    ASSERT_TRUE(turned.ok());
    EXPECT_EQ(turned->paper, PaperSize::A3);
    EXPECT_TRUE(turned->landscape);
    EXPECT_EQ(turned->frame, kBuiltInFrameId);
}

TEST(SheetArrange, APaperSuggestionHonoursTheFrameAndTheShareOfTheSheet)
{
    // 200 x 100 m at 1 : 1000 is 200 x 100 mm: A4 landscape. Beside panels
    // taking 36% of the width it has 0.64 x 271.64 = 173.8 mm on A4 and
    // 0.64 x 188 = 120.3 on A4 portrait: A3 landscape (246.4).
    PaperRequest request;
    request.scale = 1000.0;
    const Box2 extent(Point2(0.0, 0.0), Point2(200.0, 100.0));
    const auto whole = suggestPaper(extent, request);
    ASSERT_TRUE(whole.ok());
    EXPECT_EQ(whole->paper, PaperSize::A4);
    request.shareAcross = 0.64;
    const auto beside = suggestPaper(extent, request);
    ASSERT_TRUE(beside.ok());
    EXPECT_EQ(beside->paper, PaperSize::A3);
    EXPECT_TRUE(beside->landscape);

    // 272 m across is just too wide for A4 landscape's frame (271.64) and
    // just fits without one: 297 - 22 = 275.
    PaperRequest frameless;
    frameless.scale = 1000.0;
    const Box2 wide(Point2(0.0, 0.0), Point2(272.0, 100.0));
    const auto framed = suggestPaper(wide, frameless);
    ASSERT_TRUE(framed.ok());
    EXPECT_EQ(framed->paper, PaperSize::A3);
    frameless.frame.clear();
    const auto plain = suggestPaper(wide, frameless);
    ASSERT_TRUE(plain.ok());
    EXPECT_EQ(plain->paper, PaperSize::A4);
    EXPECT_TRUE(plain->landscape);
    EXPECT_TRUE(plain->frame.empty());
}

TEST(SheetArrange, ContentTooLargeForA0SaysTheScaleA0WouldTake)
{
    // 5 km square at 1 : 500 is 10 m of paper. A0 portrait, frameless, has
    // 819 x 1167 mm: 1 : 6105 would do, so 1 : 10000.
    PaperRequest request;
    request.scale = 500.0;
    const auto huge = suggestPaper(Box2(Point2(0.0, 0.0), Point2(5000.0, 5000.0)), request);
    ASSERT_FALSE(huge.ok());
    EXPECT_EQ(huge.error().code, ErrorCode::NotFound);
    EXPECT_NE(huge.error().message.find("1:10000"), std::string::npos) << huge.error().message;

    request.scale = 0.0;
    EXPECT_EQ(suggestPaper(Box2(Point2(0.0, 0.0), Point2(1.0, 1.0)), request).error().code,
              ErrorCode::InvalidArgument);
    request.scale = 500.0;
    request.shareUp = 1.5;
    EXPECT_EQ(suggestPaper(Box2(Point2(0.0, 0.0), Point2(1.0, 1.0)), request).error().code,
              ErrorCode::InvalidArgument);
}

TEST(SheetArrange, TheSuggestedScaleIsTheLargestStandardOneThatFits)
{
    // 300 x 100 m on A3: 1 : 779 is needed, so 1 : 1000; turned a quarter it
    // is 100 wide and 300 high, 1 : 1200, so 1 : 1250.
    const auto content = turnedRectangle(Point2(0.0, 0.0), 300.0, 100.0, 0.0);
    const Sheet sheet;
    EXPECT_EQ(suggestScale(content, sheet, 0.0).value(), 1000.0);
    EXPECT_EQ(suggestScale(content, sheet, kPi / 2.0).value(), 1250.0);
    // 100 x 40 m in a 200 x 100 mm viewport: 1 : max(500, 400).
    const auto small = turnedRectangle(Point2(0.0, 0.0), 100.0, 40.0, 0.0);
    EXPECT_EQ(suggestScale(small, Box2(Point2(0.0, 0.0), Point2(200.0, 100.0)), 0.0).value(), 500.0);
    EXPECT_FALSE(suggestScale(small, Box2{}, 0.0).ok());
}

TEST(SheetArrange, ApplyingPaperKeepsTheLayoutInProportion)
{
    // A3 to A1: the tiling area 24..409 x 36..286 becomes the doubled frame's
    // 46..820 x 70..574 inset by 1, 47..819 x 71..573.
    Sheet sheet;
    sheet.viewports.push_back(viewportAt("vp1", ViewportKind::Plan, Box2(Point2(24.0, 36.0), Point2(409.0, 286.0))));
    sheet.viewports.push_back(viewportAt("vp2", ViewportKind::Legend, Box2(Point2(300.0, 50.0), Point2(400.0, 150.0))));
    sheet.viewports.push_back(viewportAt("vp3", ViewportKind::Notes, Box2{}));
    PaperAdvice advice;
    advice.paper = PaperSize::A1;
    advice.landscape = true;
    advice.frame = std::string(kBuiltInFrameId);
    const std::vector<std::string> moved = applyPaper(sheet, advice);
    EXPECT_EQ(moved, (std::vector<std::string>{"vp1", "vp2"}));
    EXPECT_EQ(sheet.paper, PaperSize::A1);
    expectBox(byId(sheet, "vp1").rect, 47.0, 71.0, 819.0, 573.0);
    const double kx = 772.0 / 385.0;
    const double ky = 502.0 / 250.0;
    expectBox(byId(sheet, "vp2").rect, 47.0 + 276.0 * kx, 71.0 + 14.0 * ky, 47.0 + 376.0 * kx,
              71.0 + 114.0 * ky);
    EXPECT_TRUE(byId(sheet, "vp3").rect.empty());

    // Portrait paper has no frame, whatever the advice names.
    advice.paper = PaperSize::A3;
    advice.landscape = false;
    (void)applyPaper(sheet, advice);
    EXPECT_TRUE(sheet.frame.empty());
    expectBox(byId(sheet, "vp1").rect, 11.0, 11.0, 286.0, 409.0);
}

TEST(SheetArrange, ThePaperForAScaleHoldsTheMainViewInTheShareItTakes)
{
    // 500 x 300 m at 1 : 1000, with the painter's 4% to spare, is 520 x
    // 312 mm. Filling the sheet it needs A2 landscape: 387 x 420 / 297 - 2 =
    // 545.27 mm across (A3 either way round and A4 are too small).
    const auto content = turnedRectangle(Point2(1000.0, 2000.0), 500.0, 300.0, 0.0);
    Sheet sheet;
    Viewport plan = viewportAt("vp1", ViewportKind::Plan, tilingArea(sheet));
    plan.autoScale = true;
    plan.autoCentre = true;
    sheet.viewports.push_back(plan);
    const auto change = fitPaperToViewport(sheet, "vp1", content, 1000.0);
    ASSERT_TRUE(change.ok()) << change.error().describe();
    EXPECT_EQ(change->advice.paper, PaperSize::A2);
    EXPECT_TRUE(change->advice.landscape);
    EXPECT_NEAR(change->advice.fill, 520.0 / (387.0 * 420.0 / 297.0 - 2.0), 1e-9);
    EXPECT_EQ(change->moved, std::vector<std::string>{"vp1"});
    EXPECT_EQ(sheet.paper, PaperSize::A2);
    EXPECT_EQ(sheet.frame, kBuiltInFrameId);
    const Viewport& fitted = byId(sheet, "vp1");
    EXPECT_EQ(fitted.rect, tilingArea(sheet));
    // The scale asked for is kept, and the view centred on what it shows.
    EXPECT_EQ(fitted.scale, 1000.0);
    EXPECT_FALSE(fitted.autoScale);
    EXPECT_FALSE(fitted.autoCentre);
    EXPECT_NEAR(fitted.centre.x, 1000.0, 1e-9);
    EXPECT_NEAR(fitted.centre.y, 2000.0, 1e-9);

    // Beside a panel it has only its 251.1 of the 385 mm across: 520 mm then
    // needs 797 of tiling width, more than A1's 772 - so A0 - and the legend
    // keeps its column.
    Sheet beside;
    const std::vector<Box2> cells = presetCells(TilingPreset::MainRight, tilingArea(beside));
    beside.viewports.push_back(viewportAt("vp1", ViewportKind::Plan, cells[0]));
    beside.viewports.push_back(viewportAt("vp2", ViewportKind::Legend, cells[1]));
    const auto wide = fitPaperToViewport(beside, "vp1", content, 1000.0);
    ASSERT_TRUE(wide.ok()) << wide.error().describe();
    EXPECT_EQ(wide->advice.paper, PaperSize::A0);
    EXPECT_TRUE(wide->advice.landscape);
    EXPECT_EQ(wide->moved, (std::vector<std::string>{"vp1", "vp2"}));
    const Box2 area = tilingArea(beside);
    EXPECT_NEAR(byId(beside, "vp1").rect.width() / area.width(), 251.1 / 385.0, 1e-9);
    EXPECT_NEAR(byId(beside, "vp2").rect.max.x, area.max.x - 1.5 * area.width() / 385.0, 1e-9);

    // A panel has no scale to choose paper for; an unknown id is not found.
    EXPECT_EQ(fitPaperToViewport(beside, "vp2", content, 1000.0).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(fitPaperToViewport(beside, "vp9", content, 1000.0).error().code,
              ErrorCode::NotFound);
    EXPECT_EQ(fitPaperToViewport(beside, "vp1", content, 0.0).error().code,
              ErrorCode::InvalidArgument);
    // Too big for A0: refused, and the sheet is left as it was.
    const Sheet before = beside;
    EXPECT_EQ(fitPaperToViewport(beside, "vp1", content, 200.0).error().code, ErrorCode::NotFound);
    EXPECT_EQ(beside, before);
}

// ---- autoArrange --------------------------------------------------------------------

TEST(SheetArrange, AutoArrangeLeavesATidySheetAlone)
{
    Sheet sheet;
    sheet.viewports.push_back(viewportAt("vp1", ViewportKind::Plan, {}));
    sheet.viewports.push_back(viewportAt("vp2", ViewportKind::Model3D, {}));
    sheet.viewports.push_back(viewportAt("vp3", ViewportKind::Legend, {}));
    (void)tileViewports(sheet, TilingPreset::MainTwoRight);
    const Sheet tiled = sheet;
    const ArrangeResult result = autoArrange(sheet);
    EXPECT_EQ(result, ArrangeResult{});
    EXPECT_EQ(sheet, tiled);
}

TEST(SheetArrange, PanelsOverTheMainViewAreMovedBesideItAndLockedOnesStay)
{
    // The plan in "Main and panel right"'s main cell, 25.5..276.6 x
    // 37.5..284.5; a locked note at the bottom of the right column; a key
    // plan and a legend dropped on the plan. They go, one gutter clear of
    // everything, highest first: the key plan (rank 4) at the top of the
    // column, 279.6 = 276.6 + 3 from the left, and the legend under it, 3 mm
    // below its 214.5.
    Sheet sheet;
    const Box2 main = presetCells(TilingPreset::MainRight, tilingArea(sheet))[0];
    expectBox(main, 25.5, 37.5, 276.6, 284.5);
    sheet.viewports.push_back(viewportAt("vp1", ViewportKind::Plan, main));
    sheet.viewports.push_back(viewportAt("vp2", ViewportKind::Legend, Box2(Point2(100.0, 100.0), Point2(180.0, 200.0))));
    sheet.viewports.push_back(viewportAt("vp3", ViewportKind::Notes, Box2(Point2(300.0, 40.0), Point2(400.0, 90.0)), true));
    sheet.viewports.push_back(viewportAt("vp4", ViewportKind::KeyPlan, Box2(Point2(120.0, 120.0), Point2(210.0, 190.0))));

    const ArrangeResult result = autoArrange(sheet);
    EXPECT_EQ(result.moved, (std::vector<std::string>{"vp2", "vp4"}));
    EXPECT_TRUE(result.overlapping.empty());
    EXPECT_TRUE(result.unplaced.empty());
    EXPECT_FALSE(result.mainShrunk);
    expectTidy(sheet);
    EXPECT_EQ(byId(sheet, "vp1").rect, main);
    EXPECT_EQ(byId(sheet, "vp3").rect, Box2(Point2(300.0, 40.0), Point2(400.0, 90.0)));
    expectBox(byId(sheet, "vp4").rect, 279.6, 214.5, 369.6, 284.5);
    expectBox(byId(sheet, "vp2").rect, 279.6, 111.5, 359.6, 211.5);

    // Arranged again, nothing moves.
    const Sheet arranged = sheet;
    EXPECT_TRUE(autoArrange(sheet).moved.empty());
    EXPECT_EQ(sheet, arranged);
}

TEST(SheetArrange, TheMainViewIsTheLowestRankWhereverItIsOnTheSheet)
{
    // A long section first on the sheet, over a plan: the plan (rank 0) is
    // the main view and stays; the section gives way.
    Sheet sheet;
    const Box2 plan(Point2(25.5, 100.0), Point2(300.0, 284.5));
    sheet.viewports.push_back(viewportAt("vp1", ViewportKind::LongSection, Box2(Point2(25.5, 37.5), Point2(300.0, 150.0))));
    sheet.viewports.push_back(viewportAt("vp2", ViewportKind::Plan, plan));
    const ArrangeResult result = autoArrange(sheet);
    EXPECT_EQ(result.moved, std::vector<std::string>{"vp1"});
    EXPECT_EQ(byId(sheet, "vp2").rect, plan);
    expectTidy(sheet);
    EXPECT_GT(areaOf(byId(sheet, "vp2").rect), areaOf(byId(sheet, "vp1").rect));
    // Never below a long section's 70 x 45 mm.
    EXPECT_GE(byId(sheet, "vp1").rect.width(), 70.0 - 1e-9);
    EXPECT_GE(byId(sheet, "vp1").rect.height(), 45.0 - 1e-9);
}

TEST(SheetArrange, UnplacedViewsTakeTheCellsTilingWouldGiveThem)
{
    // A plan and a legend with no place yet: "Main and panel right", 66% and
    // 34%, packed left to right - exactly the tiled cells.
    Sheet sheet;
    sheet.viewports.push_back(viewportAt("vp1", ViewportKind::Legend, {}));
    sheet.viewports.push_back(viewportAt("vp2", ViewportKind::Plan, {}));
    const ArrangeResult result = autoArrange(sheet);
    EXPECT_EQ(result.moved, (std::vector<std::string>{"vp1", "vp2"}));
    const std::vector<Box2> cells = presetCells(TilingPreset::MainRight, tilingArea(sheet));
    expectBox(byId(sheet, "vp2").rect, cells[0].min.x, cells[0].min.y, cells[0].max.x, cells[0].max.y);
    expectBox(byId(sheet, "vp1").rect, cells[1].min.x, cells[1].min.y, cells[1].max.x, cells[1].max.y);
}

TEST(SheetArrange, WhenNothingElseMakesRoomTheMainViewShrinksFromItsTopLeftCorner)
{
    // The plan fills the sheet and a legend sits on it: no room anywhere
    // until the plan is 90% of its 382 x 247 mm, which leaves a column
    // 407.5 - (25.5 + 343.8 + 3) = 35.2 mm wide on the right.
    Sheet sheet;
    const Box2 full = presetCells(TilingPreset::Full, tilingArea(sheet))[0];
    sheet.viewports.push_back(viewportAt("vp1", ViewportKind::Plan, full));
    sheet.viewports.push_back(viewportAt("vp2", ViewportKind::Legend, Box2(Point2(100.0, 100.0), Point2(180.0, 200.0))));
    const ArrangeResult result = autoArrange(sheet);
    EXPECT_TRUE(result.mainShrunk);
    EXPECT_TRUE(result.overlapping.empty());
    expectTidy(sheet);
    const Box2& plan = byId(sheet, "vp1").rect;
    EXPECT_EQ(plan.min.x, full.min.x);
    EXPECT_EQ(plan.max.y, full.max.y);
    EXPECT_NEAR(plan.width(), 0.9 * full.width(), 1e-9);
    EXPECT_NEAR(plan.height(), 0.9 * full.height(), 1e-9);
    const Box2& legend = byId(sheet, "vp2").rect;
    EXPECT_LE(legend.width(), 35.2 + 1e-9);
    EXPECT_GE(legend.width(), 16.0);
    EXPECT_GT(areaOf(plan), areaOf(legend));
}

TEST(SheetArrange, AStrayViewIsBroughtBackOntoTheSheet)
{
    Sheet sheet;
    sheet.viewports.push_back(viewportAt("vp1", ViewportKind::Legend, Box2(Point2(500.0, 100.0), Point2(580.0, 160.0))));
    const ArrangeResult result = autoArrange(sheet);
    EXPECT_EQ(result.moved, std::vector<std::string>{"vp1"});
    // Against the right of the tiling area less half a gutter: 407.5.
    expectBox(byId(sheet, "vp1").rect, 327.5, 100.0, 407.5, 160.0);
}

TEST(SheetArrange, ACrowdedSheetIsArrangedTheSameWayEveryTimeAndNeverBelowTheMinimum)
{
    // A plan and seven panels all dropped in one spot on A4.
    Sheet sheet;
    sheet.paper = PaperSize::A4;
    const Box2 spot(Point2(60.0, 60.0), Point2(160.0, 140.0));
    sheet.viewports.push_back(viewportAt("vp1", ViewportKind::Plan, Box2(Point2(20.0, 30.0), Point2(250.0, 180.0))));
    const ViewportKind panels[] = {ViewportKind::Legend, ViewportKind::KeyPlan, ViewportKind::Notes,
                                   ViewportKind::Image,  ViewportKind::Model3D, ViewportKind::Legend,
                                   ViewportKind::Notes};
    int n = 2;
    for (const ViewportKind kind : panels) {
        sheet.viewports.push_back(viewportAt("vp" + std::to_string(n++), kind, spot));
    }
    Sheet again = sheet;
    const ArrangeResult result = autoArrange(sheet);
    EXPECT_EQ(autoArrange(again), result);
    EXPECT_EQ(again, sheet);
    EXPECT_TRUE(result.overlapping.empty());
    expectTidy(sheet);
    for (const Viewport& viewport : sheet.viewports) {
        const SizeMm least = minimumSize(viewport.kind);
        EXPECT_GE(viewport.rect.width(), least.width - 1e-9) << viewport.id;
        EXPECT_GE(viewport.rect.height(), least.height - 1e-9) << viewport.id;
        if (viewport.id != "vp1") {
            EXPECT_LE(areaOf(viewport.rect), areaOf(byId(sheet, "vp1").rect)) << viewport.id;
        }
    }
    // And a sheet arranged completely stays as it is.
    const Sheet arranged = sheet;
    EXPECT_TRUE(autoArrange(sheet).moved.empty());
    EXPECT_EQ(sheet, arranged);
}

TEST(SheetArrange, APanelLargerThanTheMainViewGivesWayToIt)
{
    // A 100 x 100 mm plan in the top-left corner and a 250 x 240 mm legend
    // beside it, overlapping nothing: the legend is the larger, so it is
    // packed again, no larger than the plan.
    Sheet sheet;
    const Box2 plan(Point2(25.5, 184.5), Point2(125.5, 284.5));
    sheet.viewports.push_back(viewportAt("vp1", ViewportKind::Plan, plan));
    sheet.viewports.push_back(viewportAt("vp2", ViewportKind::Legend, Box2(Point2(150.0, 40.0), Point2(400.0, 280.0))));
    const ArrangeResult result = autoArrange(sheet);
    EXPECT_EQ(result.moved, std::vector<std::string>{"vp2"});
    EXPECT_EQ(byId(sheet, "vp1").rect, plan);
    expectTidy(sheet);
    EXPECT_LE(areaOf(byId(sheet, "vp2").rect), areaOf(plan) + 1e-9);
    // A legend no larger than the plan stays where it is.
    Sheet small;
    small.viewports.push_back(viewportAt("vp1", ViewportKind::Plan, plan));
    const Box2 legend(Point2(150.0, 40.0), Point2(230.0, 140.0));
    small.viewports.push_back(viewportAt("vp2", ViewportKind::Legend, legend));
    EXPECT_TRUE(autoArrange(small).moved.empty());
    EXPECT_EQ(byId(small, "vp2").rect, legend);
}

TEST(SheetArrange, AnUnplacedMainViewIsPackedFirstAndStaysTheLargest)
{
    // The plan has no place yet and a large 3D view fills most of the sheet:
    // the plan takes the first cell, and the 3D view is made no larger.
    Sheet sheet;
    sheet.viewports.push_back(viewportAt("vp1", ViewportKind::Model3D, Box2(Point2(30.0, 40.0), Point2(400.0, 280.0))));
    sheet.viewports.push_back(viewportAt("vp2", ViewportKind::Plan, {}));
    const ArrangeResult result = autoArrange(sheet);
    EXPECT_TRUE(result.unplaced.empty());
    EXPECT_TRUE(result.overlapping.empty());
    expectTidy(sheet);
    ASSERT_FALSE(byId(sheet, "vp2").rect.empty());
    EXPECT_GE(areaOf(byId(sheet, "vp2").rect), areaOf(byId(sheet, "vp1").rect) - 1e-9);
}

TEST(SheetArrange, AViewInTheTilingAreasMarginIsBroughtInsideIt)
{
    // Inside the drawing area (23..410) but over the tiling area's inset:
    // it is moved to 25.5, half a gutter inside the tiling area's 24.
    Sheet sheet;
    sheet.viewports.push_back(viewportAt("vp1", ViewportKind::Legend, Box2(Point2(23.5, 100.0), Point2(103.5, 160.0))));
    const ArrangeResult result = autoArrange(sheet);
    EXPECT_EQ(result.moved, std::vector<std::string>{"vp1"});
    expectBox(byId(sheet, "vp1").rect, 25.5, 100.0, 105.5, 160.0);
}

TEST(SheetArrange, AViewAsLargeAsTheSpaceIsBroughtInWithoutFailing)
{
    // On A2 and A4 the packing space's width less a view's own can round to
    // just below its left edge; the view is brought in, not refused.
    for (const PaperSize paper : {PaperSize::A4, PaperSize::A2}) {
        Sheet sheet;
        sheet.paper = paper;
        const Box2 space = tilingArea(sheet).inflated(-kTilingGutterMm / 2.0);
        const Box2 wide(Point2(space.min.x + 40.0, space.min.y - 30.0),
                        Point2(space.max.x + 40.0, space.max.y - 30.0));
        sheet.viewports.push_back(viewportAt("vp1", ViewportKind::Plan, wide));
        (void)autoArrange(sheet);
        expectTidy(sheet);
        EXPECT_NEAR(byId(sheet, "vp1").rect.width(), space.width(), 1e-9);
        EXPECT_NEAR(byId(sheet, "vp1").rect.min.x, space.min.x, 1e-9);
    }
}

TEST(SheetArrange, AViewWithNoRoomAtAllIsReportedAndLeftWhereItWas)
{
    // A locked panel covers the whole tiling area; the plan under it has
    // nowhere to go.
    Sheet sheet;
    const Box2 area = tilingArea(sheet);
    const Box2 plan(Point2(50.0, 50.0), Point2(150.0, 150.0));
    sheet.viewports.push_back(viewportAt("vp1", ViewportKind::Image, area, true));
    sheet.viewports.push_back(viewportAt("vp2", ViewportKind::Plan, plan));
    sheet.viewports.push_back(viewportAt("vp3", ViewportKind::Legend, {}));
    const ArrangeResult result = autoArrange(sheet);
    EXPECT_TRUE(result.moved.empty());
    EXPECT_EQ(result.overlapping, std::vector<std::string>{"vp2"});
    EXPECT_EQ(result.unplaced, std::vector<std::string>{"vp3"});
    EXPECT_EQ(byId(sheet, "vp2").rect, plan);
    EXPECT_EQ(byId(sheet, "vp1").rect, area);
}

// ---- Align, distribute, match, fit ----------------------------------------------------

namespace {

Sheet threeViews()
{
    Sheet sheet;
    sheet.viewports.push_back(viewportAt("a", ViewportKind::Legend, Box2(Point2(30.0, 40.0), Point2(80.0, 80.0))));
    sheet.viewports.push_back(viewportAt("b", ViewportKind::Notes, Box2(Point2(100.0, 50.0), Point2(130.0, 120.0))));
    sheet.viewports.push_back(viewportAt("c", ViewportKind::Image, Box2(Point2(200.0, 45.0), Point2(260.0, 60.0))));
    return sheet;
}

const std::vector<std::string> kAbc{"a", "b", "c"};

} // namespace

TEST(SheetArrange, ViewsAlignOnTheirOutermostEdgeOrTheMiddleOfTheirBox)
{
    // The box round them is 30..260 x 40..120, its middle (145, 80).
    struct Case {
        AlignEdge edge;
        std::vector<std::string> moved;
        Box2 a, b, c;
    };
    const Case cases[] = {
        {AlignEdge::Left, {"b", "c"}, Box2(Point2(30, 40), Point2(80, 80)), Box2(Point2(30, 50), Point2(60, 120)), Box2(Point2(30, 45), Point2(90, 60))},
        {AlignEdge::Right, {"a", "b"}, Box2(Point2(210, 40), Point2(260, 80)), Box2(Point2(230, 50), Point2(260, 120)), Box2(Point2(200, 45), Point2(260, 60))},
        {AlignEdge::Top, {"a", "c"}, Box2(Point2(30, 80), Point2(80, 120)), Box2(Point2(100, 50), Point2(130, 120)), Box2(Point2(200, 105), Point2(260, 120))},
        {AlignEdge::Bottom, {"b", "c"}, Box2(Point2(30, 40), Point2(80, 80)), Box2(Point2(100, 40), Point2(130, 110)), Box2(Point2(200, 40), Point2(260, 55))},
        {AlignEdge::HorizontalCentre, {"a", "b", "c"}, Box2(Point2(120, 40), Point2(170, 80)), Box2(Point2(130, 50), Point2(160, 120)), Box2(Point2(115, 45), Point2(175, 60))},
        {AlignEdge::VerticalCentre, {"a", "b", "c"}, Box2(Point2(30, 60), Point2(80, 100)), Box2(Point2(100, 45), Point2(130, 115)), Box2(Point2(200, 72.5), Point2(260, 87.5))},
    };
    for (const Case& expected : cases) {
        Sheet sheet = threeViews();
        const auto moved = alignViewports(sheet, kAbc, expected.edge);
        ASSERT_TRUE(moved.ok()) << moved.error().describe();
        EXPECT_EQ(*moved, expected.moved) << toString(expected.edge);
        EXPECT_EQ(byId(sheet, "a").rect, expected.a) << toString(expected.edge);
        EXPECT_EQ(byId(sheet, "b").rect, expected.b) << toString(expected.edge);
        EXPECT_EQ(byId(sheet, "c").rect, expected.c) << toString(expected.edge);
        EXPECT_EQ(alignEdgeFrom(toString(expected.edge)), expected.edge);
    }
    EXPECT_FALSE(alignEdgeFrom("middle").has_value());
}

TEST(SheetArrange, ALockedViewCountsWhereItIsAndOneViewAlignsToTheTilingArea)
{
    Sheet sheet = threeViews();
    sheet.viewports[0].locked = true;
    // Right: the locked "a" does not move; the others meet the box's right.
    const auto moved = alignViewports(sheet, kAbc, AlignEdge::Right);
    ASSERT_TRUE(moved.ok());
    EXPECT_EQ(*moved, std::vector<std::string>{"b"});
    EXPECT_EQ(byId(sheet, "a").rect, threeViews().viewports[0].rect);

    // One alone goes to the tiling area's edge, 24..409 x 36..286.
    Sheet one = threeViews();
    const std::vector<std::string> b{"b"};
    ASSERT_TRUE(alignViewports(one, b, AlignEdge::Top).ok());
    EXPECT_EQ(byId(one, "b").rect, Box2(Point2(100.0, 216.0), Point2(130.0, 286.0)));
    ASSERT_TRUE(alignViewports(one, b, AlignEdge::HorizontalCentre).ok());
    EXPECT_EQ(byId(one, "b").rect.center().x, 216.5);

    // An unknown id is NotFound; views none of which is placed are refused.
    const std::vector<std::string> unknown{"a", "zz"};
    EXPECT_EQ(alignViewports(one, unknown, AlignEdge::Left).error().code, ErrorCode::NotFound);
    Sheet unplaced;
    unplaced.viewports.push_back(viewportAt("x", ViewportKind::Legend, {}));
    EXPECT_EQ(alignViewports(unplaced, std::vector<std::string>{"x"}, AlignEdge::Left).error().code,
              ErrorCode::InvalidArgument);
}

TEST(SheetArrange, DistributingLeavesEqualGapsBetweenTheFirstAndTheLast)
{
    // Left edges 10, 40, 150, 300; the first ends at 30 and the last starts
    // at 300, and the middle two are 60 + 20 wide: (270 - 80) / 3 = 63.33
    // between each.
    Sheet sheet;
    sheet.viewports.push_back(viewportAt("d", ViewportKind::Legend, Box2(Point2(300.0, 50.0), Point2(320.0, 60.0))));
    sheet.viewports.push_back(viewportAt("a", ViewportKind::Legend, Box2(Point2(10.0, 50.0), Point2(30.0, 60.0))));
    sheet.viewports.push_back(viewportAt("c", ViewportKind::Legend, Box2(Point2(150.0, 90.0), Point2(170.0, 100.0))));
    sheet.viewports.push_back(viewportAt("b", ViewportKind::Legend, Box2(Point2(40.0, 70.0), Point2(100.0, 80.0))));
    Sheet reversed = sheet;
    const std::vector<std::string> ids{"a", "b", "c", "d"};
    const auto moved = distributeViewports(sheet, ids, DistributeAxis::Horizontal);
    ASSERT_TRUE(moved.ok()) << moved.error().describe();
    EXPECT_EQ(*moved, (std::vector<std::string>{"c", "b"}));
    const double gap = 190.0 / 3.0;
    EXPECT_NEAR(byId(sheet, "b").rect.min.x, 30.0 + gap, 1e-9);
    EXPECT_NEAR(byId(sheet, "c").rect.min.x, 90.0 + 2.0 * gap, 1e-9);
    EXPECT_EQ(byId(sheet, "a").rect.min.x, 10.0);
    EXPECT_EQ(byId(sheet, "d").rect.min.x, 300.0);
    EXPECT_EQ(byId(sheet, "c").rect.min.y, 90.0) << "a horizontal spacing leaves the heights";
    // The order the ids come in does not matter.
    const std::vector<std::string> backwards{"d", "c", "b", "a"};
    ASSERT_TRUE(distributeViewports(reversed, backwards, DistributeAxis::Horizontal).ok());
    EXPECT_EQ(reversed, sheet);

    EXPECT_EQ(distributeAxisFrom("vertical"), DistributeAxis::Vertical);
    EXPECT_EQ(distributeAxisFrom(toString(DistributeAxis::Horizontal)), DistributeAxis::Horizontal);
}

TEST(SheetArrange, DistributingNeedsThreeViewsThatCanMove)
{
    Sheet sheet = threeViews();
    sheet.viewports[1].locked = true;
    const auto locked = distributeViewports(sheet, kAbc, DistributeAxis::Vertical);
    ASSERT_FALSE(locked.ok());
    EXPECT_EQ(locked.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(locked.error().message.find("there are 2"), std::string::npos);
    EXPECT_EQ(distributeViewports(sheet, std::vector<std::string>{"a", "q"}, DistributeAxis::Vertical)
                  .error()
                  .code,
              ErrorCode::NotFound);

    // Vertically: bottoms 40 (a, ends 80), 45 (c, 15 high) and 50 (b, starts
    // 50 ends 120): c's 15 mm would have to fit between 80 and 50, a gap of
    // (50 - 80 - 15) / 2 = -22.5 mm that would lay the views over each
    // other. Refused, and nothing moves.
    Sheet free = threeViews();
    const Sheet unmoved = free;
    const auto moved = distributeViewports(free, kAbc, DistributeAxis::Vertical);
    ASSERT_FALSE(moved.ok());
    EXPECT_EQ(moved.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(moved.error().message.find("15.0 mm high together, and there is -30.0 mm"),
              std::string::npos)
        << moved.error().message;
    EXPECT_EQ(free, unmoved);
}

TEST(SheetArrange, MatchingAScaleCopiesItToEveryScaledViewOnAnySheet)
{
    SheetSet set;
    Sheet first;
    first.id = "s1";
    Viewport plan = viewportAt("vp1", ViewportKind::Plan, Box2(Point2(30, 40), Point2(200, 200)));
    plan.scale = 500.0;
    first.viewports.push_back(plan);
    Viewport legend = viewportAt("vp4", ViewportKind::Legend, Box2(Point2(300, 40), Point2(380, 100)));
    first.viewports.push_back(legend);
    Sheet second;
    second.id = "s2";
    Viewport longSection = viewportAt("vp2", ViewportKind::LongSection, Box2(Point2(30, 40), Point2(400, 150)));
    longSection.scale = 1000.0;
    longSection.verticalExaggeration = 10.0;
    longSection.autoScale = true;
    second.viewports.push_back(longSection);
    Viewport crossSections = viewportAt("vp3", ViewportKind::CrossSections, Box2(Point2(30, 160), Point2(200, 280)));
    crossSections.scale = 200.0;
    crossSections.verticalExaggeration = 5.0;
    second.viewports.push_back(crossSections);
    set.sheets = {first, second};

    SheetSet fromPlan = set;
    const std::vector<std::string> targets{"vp2", "vp3", "vp4"};
    const auto changed = matchScale(fromPlan, targets, "vp1");
    ASSERT_TRUE(changed.ok()) << changed.error().describe();
    EXPECT_EQ(*changed, (std::vector<std::string>{"vp2", "vp3"}));
    EXPECT_EQ(fromPlan.sheets[1].viewports[0].scale, 500.0);
    EXPECT_FALSE(fromPlan.sheets[1].viewports[0].autoScale);
    EXPECT_EQ(fromPlan.sheets[1].viewports[0].verticalExaggeration, 10.0) << "a plan has none to give";
    EXPECT_EQ(fromPlan.sheets[1].viewports[1].scale, 500.0);
    EXPECT_EQ(fromPlan.sheets[0].viewports[1], legend);

    // Section to section: the exaggeration too.
    SheetSet fromSection = set;
    ASSERT_TRUE(matchScale(fromSection, std::vector<std::string>{"vp3"}, "vp2").ok());
    EXPECT_EQ(fromSection.sheets[1].viewports[1].scale, 1000.0);
    EXPECT_EQ(fromSection.sheets[1].viewports[1].verticalExaggeration, 10.0);

    // An automatic scale is passed as drawn.
    SheetSet drawn = set;
    ASSERT_TRUE(matchScale(drawn, std::vector<std::string>{"vp1"}, "vp2", 750.0).ok());
    EXPECT_EQ(drawn.sheets[0].viewports[0].scale, 750.0);
    // An automatic section's exaggeration is passed as drawn too, not its
    // stale stored one.
    SheetSet fitted = set;
    ASSERT_TRUE(matchScale(fitted, std::vector<std::string>{"vp3"}, "vp2", 750.0, 5.0).ok());
    EXPECT_EQ(fitted.sheets[1].viewports[1].scale, 750.0);
    EXPECT_EQ(fitted.sheets[1].viewports[1].verticalExaggeration, 5.0);

    SheetSet refused = set;
    EXPECT_EQ(matchScale(refused, targets, "vp9").error().code, ErrorCode::NotFound);
    EXPECT_EQ(matchScale(refused, std::vector<std::string>{"vp9"}, "vp1").error().code, ErrorCode::NotFound);
    EXPECT_EQ(matchScale(refused, targets, "vp4").error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(matchScale(refused, targets, "vp1", 0.0).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(refused, set);
}

TEST(SheetArrange, AViewIsSizedToItsContentAboutItsCentre)
{
    // 100 x 50 m at 1 : 500 is 200 x 100 mm, and 5% more: 210 x 105 about
    // the old centre (150, 150).
    Viewport plan = viewportAt("vp1", ViewportKind::Plan, Box2(Point2(100.0, 100.0), Point2(200.0, 200.0)));
    plan.scale = 500.0;
    plan.autoCentre = true;
    const auto content = turnedRectangle(Point2(1000.0, 2000.0), 100.0, 50.0, 0.0);
    const Box2 area = drawingArea(Sheet{});
    ASSERT_TRUE(fitViewportToContent(plan, content, area).ok());
    expectBox(plan.rect, 45.0, 97.5, 255.0, 202.5);
    EXPECT_NEAR(plan.centre.x, 1000.0, 1e-9);
    EXPECT_NEAR(plan.centre.y, 2000.0, 1e-9);
    EXPECT_FALSE(plan.autoCentre) << "the centre is set, so it is no longer automatic";
    EXPECT_FALSE(plan.autoScale);
    EXPECT_EQ(plan.scale, 500.0);

    // Turned a quarter, the 50 m runs across.
    plan.rotation = kPi / 2.0;
    ASSERT_TRUE(fitViewportToContent(plan, content, area).ok());
    expectBox(plan.rect, 97.5, 45.0, 202.5, 255.0);

    // Too big for the paper: the drawing area. Too small: the kind's minimum.
    plan.rotation = 0.0;
    ASSERT_TRUE(fitViewportToContent(plan, turnedRectangle(Point2(0, 0), 1000.0, 1000.0, 0.0), area).ok());
    EXPECT_EQ(plan.rect, area);
    ASSERT_TRUE(fitViewportToContent(plan, turnedRectangle(Point2(0, 0), 1.0, 1.0, 0.0), area).ok());
    EXPECT_NEAR(plan.rect.width(), 35.0, 1e-9);
    EXPECT_NEAR(plan.rect.height(), 30.0, 1e-9);

    // A long section's levels are exaggerated: 5 m at 1 : 500 x 10 = 100 mm.
    Viewport section = viewportAt("vp2", ViewportKind::LongSection, Box2(Point2(100.0, 100.0), Point2(300.0, 200.0)));
    section.scale = 500.0;
    section.verticalExaggeration = 10.0;
    const std::vector<Point2> profile{Point2(0.0, 10.0), Point2(100.0, 15.0)};
    ASSERT_TRUE(fitViewportToContent(section, profile, area).ok());
    expectBox(section.rect, 95.0, 97.5, 305.0, 202.5);
    EXPECT_NEAR(section.centre.x, 50.0, 1e-9);
    EXPECT_NEAR(section.centre.y, 12.5, 1e-9);

    Viewport legend = viewportAt("vp3", ViewportKind::Legend, Box2(Point2(0, 0), Point2(10, 10)));
    EXPECT_EQ(fitViewportToContent(legend, content, area).error().code, ErrorCode::InvalidArgument);
    Viewport unplaced = viewportAt("vp4", ViewportKind::Plan, {});
    EXPECT_EQ(fitViewportToContent(unplaced, content, area).error().code, ErrorCode::InvalidArgument);
}

TEST(SheetArrange, RotatingAPlanToBestFitTurnsItOnlyAsFarAsTheScaleNeeds)
{
    // The 450 x 20 m strip at 45 degrees in the whole tiling area, less the
    // painter's 4%: 370.2 x 240.4 mm. Along it 1 : 1215.6, so 1 : 1250;
    // square to the paper 1 : 1382.
    Viewport plan = viewportAt("vp1", ViewportKind::Plan, tilingArea(Sheet{}));
    plan.scale = 2000.0;
    plan.autoScale = true;
    plan.autoCentre = true;
    const auto strip = turnedRectangle(Point2(5000.0, 7000.0), 450.0, 20.0, 45.0 * kDegree);
    const auto chosen = rotateToBestFit(plan, strip);
    ASSERT_TRUE(chosen.ok());
    EXPECT_EQ(chosen->rotation, plan.rotation);
    EXPECT_EQ(chosen->standardScale, plan.scale);
    // The painter's own fit measures the drawing's box, which a turned
    // drawing overfills: the view keeps the scale and centre chosen here.
    EXPECT_FALSE(plan.autoScale);
    EXPECT_FALSE(plan.autoCentre);
    EXPECT_EQ(plan.scale, 1250.0);
    EXPECT_GT(plan.rotation, 0.0);
    EXPECT_LT(plan.rotation, 45.0 * kDegree);
    EXPECT_NEAR(plan.centre.x, 5000.0, 1e-6);
    EXPECT_NEAR(plan.centre.y, 7000.0, 1e-6);
    EXPECT_EQ(plan.rect, tilingArea(Sheet{})) << "the rectangle stays";
    // At that turn the strip fits 1 : 1250 with the 4% to spare.
    const auto check = fitAtRotation(strip, {385.0 / kAutoScaleSpare, 250.0 / kAutoScaleSpare}, plan.rotation);
    ASSERT_TRUE(check.ok());
    EXPECT_LE(check->scale, 1250.0 * (1.0 + 1e-9));

    Viewport legend = viewportAt("vp2", ViewportKind::Legend, tilingArea(Sheet{}));
    EXPECT_EQ(rotateToBestFit(legend, strip).error().code, ErrorCode::InvalidArgument);
    Viewport unplaced = viewportAt("vp3", ViewportKind::Plan, {});
    EXPECT_EQ(rotateToBestFit(unplaced, strip).error().code, ErrorCode::InvalidArgument);
}

TEST(SheetArrange, TheFittedLayoutIsTurnedWhenThatFitsALargerScaleOrFewerSheets)
{
    // The strip at 45 degrees: its box is 332.3 m square, which one A3 sheet
    // holds at 1 : 2000 (332.3 / 250 needs 1 : 1329). Turned 3.62 degrees
    // (as above) it is 1 : 1250.
    const auto strip = turnedRectangle(Point2(0.0, 0.0), 450.0, 20.0, 45.0 * kDegree);
    Box2 box;
    for (const Point2& corner : strip) {
        box.expand(corner);
    }
    LayoutRequest request;
    request.planArea = box;
    const katana::entity::Model model;
    const double t = 45.0 * kDegree - std::atan2(225.0, 10.0) +
                     std::acos(156.25 / std::hypot(225.0, 10.0));

    const auto square = smartLayout(model, request);
    ASSERT_TRUE(square.ok());
    EXPECT_EQ(square->front().viewports.front().scale, 2000.0);
    const auto turned = smartLayoutRotated(model, request, strip);
    ASSERT_TRUE(turned.ok()) << turned.error().describe();
    ASSERT_EQ(turned->size(), 1u);
    const Viewport& plan = turned->front().viewports.front();
    EXPECT_EQ(plan.scale, 1250.0);
    EXPECT_NEAR(plan.rotation, t, 1e-9);
    EXPECT_EQ(plan.rect, square->front().viewports.front().rect);

    // At a fixed 1 : 1250 the square layout needs a key plan and tiles; turned,
    // one sheet holds it.
    request.scale = 1250.0;
    const auto tiles = smartLayout(model, request);
    ASSERT_TRUE(tiles.ok());
    EXPECT_GT(tiles->size(), 1u);
    const auto one = smartLayoutRotated(model, request, strip);
    ASSERT_TRUE(one.ok());
    ASSERT_EQ(one->size(), 1u);
    EXPECT_EQ(one->front().viewports.front().scale, 1250.0);
    EXPECT_NEAR(one->front().viewports.front().rotation, t, 1e-9);

    // At 1 : 2000 it fits square: nothing is turned.
    request.scale = 2000.0;
    EXPECT_EQ(smartLayoutRotated(model, request, strip).value(), smartLayout(model, request).value());
    // At 1 : 1000 it fits no single sheet even turned: the tiles stay.
    request.scale = 1000.0;
    EXPECT_EQ(smartLayoutRotated(model, request, strip).value(), smartLayout(model, request).value());
}

TEST(SheetArrange, TheFittedLayoutTurnsOnlyAPlanOfAnArea)
{
    const katana::entity::Model model;
    const auto strip = turnedRectangle(Point2(0.0, 0.0), 450.0, 20.0, 45.0 * kDegree);
    LayoutRequest request;
    EXPECT_EQ(smartLayoutRotated(model, request, strip).error().code, ErrorCode::InvalidArgument);
    request.planArea = Box2(Point2(0.0, 0.0), Point2(100.0, 100.0));
    request.alignment = "ROAD";
    request.longSection = true;
    EXPECT_EQ(smartLayoutRotated(model, request, strip).error().code, ErrorCode::InvalidArgument);
    // Content with no size is simply not turned.
    request.alignment.clear();
    request.longSection = false;
    const std::vector<Point2> point{Point2(5.0, 5.0)};
    EXPECT_EQ(smartLayoutRotated(model, request, point).value(), smartLayout(model, request).value());
}
