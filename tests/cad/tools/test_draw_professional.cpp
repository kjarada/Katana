// The drawing system's draw tools (src/katana_cad/tools/draw_professional.cpp,
// and the Polyline tool's arc segments and the Point tool's heights in
// draw_lines.cpp), each driven as a user would against geometry worked out
// by hand.

#include <gtest/gtest.h>

#include <cmath>

#include "katana/cad/drawing/construction.hpp"
#include "katana/cad/selection.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/math/numerics.hpp"
#include "tool_driver.hpp"
#include "tools/families.hpp"

using katana::cad::testing::ToolDriver;
using katana::cad::ToolStep;
using katana::entity::Entity;
using katana::geometry::Arc2;
using katana::geometry::CurvePolyline2;
using katana::geometry::Ellipse2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Spline2;
using katana::math::kHalfPi;
using katana::math::kPi;

namespace {

std::vector<Entity> entitiesOf(ToolDriver& driver)
{
    std::vector<Entity> out;
    driver.document().model().entities.forEach([&](const Entity& e) { out.push_back(e); });
    return out;
}

template <typename T> const T* only(ToolDriver& driver)
{
    const T* found = nullptr;
    int count = 0;
    driver.document().model().entities.forEach([&](const Entity& e) {
        if (const auto* g = std::get_if<T>(&e.geometry)) {
            found = g;
            ++count;
        }
    });
    EXPECT_EQ(count, 1);
    return found;
}

void expectNear(const Point2& a, const Point2& b, double tolerance = 1e-9)
{
    EXPECT_NEAR(a.x, b.x, tolerance);
    EXPECT_NEAR(a.y, b.y, tolerance);
}

} // namespace

// ---- Polyline with arc segments ---------------------------------------------------------

TEST(DrawPolylineArcs, AnArcSegmentIsTangentToTheSegmentBefore)
{
    // East 10, then an arc to (15, 5): tangent to east at (10, 0), a quarter
    // circle of radius 5 about (10, 5).
    ToolDriver driver;
    driver.start("draw.polyline");
    driver.click(0, 0);
    driver.click(10, 0);
    driver.type("A");
    EXPECT_EQ(driver.tool().prompt(), "Specify end point of arc or [Line/Second/Undo]");
    driver.click(15, 5);
    driver.type("L");
    driver.click(15, 15);
    driver.enter();
    const auto* shape = only<CurvePolyline2>(driver);
    ASSERT_NE(shape, nullptr);
    ASSERT_EQ(shape->vertices.size(), 4u);
    const auto arc = std::get<Arc2>(shape->segment(1));
    expectNear(arc.center, Point2(10, 5));
    EXPECT_NEAR(arc.radius, 5.0, 1e-9);
    EXPECT_NEAR(arc.sweep, kHalfPi, 1e-9);
    EXPECT_FALSE(shape->isArc(2)) << "back in Line mode";
    EXPECT_EQ(driver.document().history().undoName(), "CREATE_POLYLINE");
}

TEST(DrawPolylineArcs, TheFirstArcHeadsEastAndSecondPassesThroughAPoint)
{
    ToolDriver driver;
    driver.start("draw.polyline");
    driver.click(0, 0);
    driver.type("Arc");
    // From (0,0) heading east to (4,4): a quarter turn left about (0,4).
    driver.click(4, 4);
    driver.type("S");
    driver.click(6, 6);      // through
    driver.click(8, 4);      // to
    driver.enter();
    const auto* shape = only<CurvePolyline2>(driver);
    ASSERT_NE(shape, nullptr);
    const auto first = std::get<Arc2>(shape->segment(0));
    expectNear(first.center, Point2(0, 4));
    const auto second = std::get<Arc2>(shape->segment(1));
    EXPECT_NEAR(second.center.distanceTo(Point2(6, 6)), second.radius, 1e-9) << "through (6,6)";
}

TEST(DrawPolylineArcs, StraightOnlyIsStillAPolyline2AndUndoTakesBackAnArc)
{
    ToolDriver driver;
    driver.start("draw.polyline");
    driver.click(0, 0);
    driver.type("A");
    driver.click(4, 4);
    driver.undo();
    driver.type("L");
    driver.click(5, 0);
    driver.enter();
    ASSERT_NE(only<Polyline2>(driver), nullptr) << "no arc left: the Polyline2 it always was";
}

// ---- Point with height -------------------------------------------------------------------

TEST(DrawPointHeights, ATypedHeightOrTheCurrentOneGoesInTheElevation)
{
    ToolDriver driver;
    driver.start("draw.point");
    driver.type("1,2,31.5");
    driver.type("H");
    driver.type("40");
    driver.click(5, 5);
    driver.click(6, 6); // after a restart, still at 40
    const auto entities = entitiesOf(driver);
    ASSERT_EQ(entities.size(), 3u);
    EXPECT_EQ(katana::entity::heightsOf(entities[0].properties, 1)[0], 31.5);
    EXPECT_EQ(katana::entity::heightsOf(entities[1].properties, 1)[0], 40.0);
    EXPECT_EQ(katana::entity::heightsOf(entities[2].properties, 1)[0], 40.0);
    driver.type("H");
    driver.type("None"); // back to plan points for the next test
}

// ---- 3D Polyline --------------------------------------------------------------------------

TEST(DrawPolyline3d, HeightsComeFromTypedZSnappedPointsAndTheCurrentHeight)
{
    ToolDriver driver;
    Entity mark;
    mark.geometry = katana::entity::PointGeometry{Point2(10, 0)};
    katana::entity::setHeights(mark.properties, {12.5});
    driver.add(katana::commands::createEntities({mark}));

    driver.start("draw.polyline3d");
    driver.type("0,0,10");
    driver.click(10, 0); // snapped onto the surveyed point: its height
    driver.click(10, 10); // nothing there and no current height: none
    driver.enter();
    const Polyline2* line = only<Polyline2>(driver);
    ASSERT_NE(line, nullptr) << "straight: a Polyline2 with its heights in the properties";
    Entity string;
    driver.document().model().entities.forEach([&](const Entity& e) {
        if (std::holds_alternative<Polyline2>(e.geometry)) {
            string = e;
        }
    });
    const auto heights = katana::entity::heightsOf(string.properties, 3);
    EXPECT_EQ(heights[0], 10.0);
    EXPECT_EQ(heights[1], 12.5);
    EXPECT_FALSE(heights[2].has_value()) << "no height is not zero";
}

// ---- Spline -------------------------------------------------------------------------------

TEST(DrawSpline, ThroughTheClickedPointsOrOnThemAsControlPoints)
{
    ToolDriver driver;
    driver.start("draw.spline");
    driver.click(0, 0);
    driver.click(3, 4);
    driver.click(7, 3);
    driver.enter();
    const auto* fit = only<Spline2>(driver);
    ASSERT_NE(fit, nullptr);
    EXPECT_EQ(fit->fitPoints.size(), 3u);
    EXPECT_LT(fit->distanceTo(Point2(3, 4)), 1e-3);

    ToolDriver second;
    second.start("draw.spline");
    second.type("C");
    second.type("D");
    second.type("2");
    second.click(0, 0);
    second.click(5, 5);
    second.click(10, 0);
    second.enter();
    const auto* control = only<Spline2>(second);
    ASSERT_NE(control, nullptr);
    EXPECT_TRUE(control->fitPoints.empty());
    EXPECT_EQ(control->degree, 2);
    expectNear(control->pointAt(0.5), Point2(5, 2.5));
}

// ---- Ellipse ------------------------------------------------------------------------------

TEST(DrawEllipse, AxisAndEndCentreAndArc)
{
    ToolDriver driver;
    driver.start("draw.ellipse");
    driver.click(-10, 0);
    driver.click(10, 0);
    driver.click(0, 4); // 4 from the axis
    const auto* ellipse = only<Ellipse2>(driver);
    ASSERT_NE(ellipse, nullptr);
    expectNear(ellipse->center, Point2(0, 0));
    EXPECT_NEAR(ellipse->majorRadius(), 10.0, 1e-12);
    EXPECT_NEAR(ellipse->minorRadius(), 4.0, 1e-12);

    ToolDriver centre;
    centre.start("draw.ellipse.centre");
    centre.click(5, 5);
    centre.click(5, 8); // a 3 m half axis, north
    centre.type("6");   // the other: 6, so it becomes the major
    const auto* tall = only<Ellipse2>(centre);
    ASSERT_NE(tall, nullptr);
    EXPECT_NEAR(tall->majorRadius(), 6.0, 1e-12);
    EXPECT_NEAR(tall->minorRadius(), 3.0, 1e-12);

    ToolDriver arc;
    arc.start("draw.ellipse.arc");
    arc.click(-10, 0);
    arc.click(10, 0);
    arc.type("5");
    arc.type("0");
    arc.type("90");
    const auto* part = only<Ellipse2>(arc);
    ASSERT_NE(part, nullptr);
    EXPECT_FALSE(part->isFull());
    EXPECT_NEAR(part->sweep, kHalfPi, 1e-12);
    expectNear(part->endPoint(), Point2(0, 5));
}

// ---- Construction Line and Ray ---------------------------------------------------------------

TEST(DrawConstruction, LinesAndRaysGoOnTheConstructionLayerInOneStep)
{
    ToolDriver driver;
    ASSERT_TRUE(driver.document().execute(katana::commands::createLine(Point2(0, 0), Point2(10, 10))).ok());
    driver.start("draw.xline");
    driver.click(0, 0);
    driver.click(1, 0);
    driver.click(0, 1);
    const std::size_t before = driver.document().history().undoCount();
    driver.enter();
    EXPECT_EQ(driver.document().history().undoCount(), before + 1);
    std::size_t construction = 0;
    driver.document().model().entities.forEach([&](const Entity& e) {
        if (katana::cad::isConstructionLayer(e.layer)) {
            ++construction;
            const auto& line = std::get<Segment2>(e.geometry);
            EXPECT_NEAR(line.length(), 2.0 * katana::cad::kConstructionReach, 1e-3);
        }
    });
    EXPECT_EQ(construction, 2u);
    EXPECT_NE(driver.document().model().layers.find("construction"), nullptr);
    // Zoom Extents frames the drawing, not the construction lines.
    const auto extent =
        katana::cad::drawnExtent(driver.document().model(), katana::cad::kNoLayerOverrides);
    EXPECT_NEAR(extent.max.x, 10.0, 1e-9);

    ToolDriver ray;
    ray.start("draw.ray");
    ray.click(0, 0);
    ray.click(0, 5);
    ray.enter();
    const auto* made = only<Segment2>(ray);
    ASSERT_NE(made, nullptr);
    EXPECT_EQ(made->start, Point2(0, 0));
    EXPECT_NEAR(made->end.y, katana::cad::kConstructionReach, 1e-6);
}

// ---- Double Line, Sketch, Revision Cloud ---------------------------------------------------------

TEST(DrawDoubleLine, BothSidesAtHalfTheWidth)
{
    ToolDriver driver;
    driver.start("draw.dline");
    driver.type("W");
    driver.type("2");
    driver.click(0, 0);
    driver.click(10, 0);
    driver.enter();
    const auto entities = entitiesOf(driver);
    ASSERT_EQ(entities.size(), 2u);
    std::vector<double> ys;
    for (const auto& e : entities) {
        ys.push_back(std::get<Polyline2>(e.geometry).vertices[0].y);
    }
    std::sort(ys.begin(), ys.end());
    EXPECT_NEAR(ys[0], -1.0, 1e-12);
    EXPECT_NEAR(ys[1], 1.0, 1e-12);
}

TEST(DrawSketch, AStrokeIsRecordedBetweenClicksAndWeeded)
{
    ToolDriver driver;
    driver.start("draw.sketch");
    driver.type("T");
    driver.type("0.01");
    driver.click(0, 0);
    for (int i = 1; i < 10; ++i) {
        katana::cad::tools::sketchMoveForTests(driver.tool(), Point2(i, 0.001 * (i % 2)));
    }
    driver.click(10, 0);
    driver.enter();
    const auto* stroke = only<Polyline2>(driver);
    ASSERT_NE(stroke, nullptr);
    EXPECT_EQ(stroke->vertices.size(), 2u) << "the wobble is within the tolerance";
}

TEST(DrawRevisionCloud, ARectangleCloudBulgesOutward)
{
    ToolDriver driver;
    driver.start("draw.revcloud");
    driver.type("A");
    driver.type("2");
    driver.type("R");
    driver.click(0, 0);
    driver.click(10, 4);
    const auto* cloud = only<CurvePolyline2>(driver);
    ASSERT_NE(cloud, nullptr);
    EXPECT_TRUE(cloud->closed);
    EXPECT_EQ(cloud->vertices.size(), 14u); // 5 + 2 + 5 + 2 arcs of 2 m
    EXPECT_GT(cloud->area(), 40.0) << "outward, so more than the rectangle";
    const auto box = cloud->boundingBox();
    EXPECT_LT(box.min.y, 0.0);
}

// ---- the constructions the Circle and Arc families lacked -------------------------------------

TEST(DrawCircleTangents, TheCircleTouchingThreeLinesNearestThePicks)
{
    // The 3-4-5 triangle (0,0) (4,0) (0,3): its incircle has radius
    // (3 + 4 - 5) / 2 = 1 about (1, 1).
    ToolDriver driver;
    const auto a = driver.add(katana::commands::createLine(Point2(0, 0), Point2(4, 0)));
    const auto b = driver.add(katana::commands::createLine(Point2(0, 0), Point2(0, 3)));
    const auto c = driver.add(katana::commands::createLine(Point2(4, 0), Point2(0, 3)));
    driver.start("draw.circle.ttt");
    driver.pick(a, 1, 0);
    driver.pick(b, 0, 1);
    driver.pick(c, 1.6, 1.8);
    const auto* circle = only<katana::geometry::Circle2>(driver);
    ASSERT_NE(circle, nullptr);
    EXPECT_NEAR(circle->radius, 1.0, 1e-9);
    expectNear(circle->center, Point2(1, 1));
}

TEST(DrawArcDirection, AnArcLeavingItsStartInTheGivenDirection)
{
    // From (0,0) to (4,4) leaving east: the quarter circle about (0,4).
    ToolDriver driver;
    driver.start("draw.arc.sed");
    driver.click(0, 0);
    driver.click(4, 4);
    driver.type("0");
    const auto* arc = only<Arc2>(driver);
    ASSERT_NE(arc, nullptr);
    expectNear(arc->center, Point2(0, 4));
    EXPECT_NEAR(arc->radius, 4.0, 1e-9);
}
