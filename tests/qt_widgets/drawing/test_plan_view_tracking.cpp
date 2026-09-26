// Object snap tracking in the plan view (docs/drawing.md, "Precision
// input"): with tracking on, an object snap the cursor lands on is acquired,
// and a point on the horizontal or vertical through it is taken exactly -
// driven with real mouse events. The paths themselves are katana_cad's
// (cad::trackAcquired, tests/cad/drawing/test_drafting.cpp).
//
// The view is 400 x 300 pixels centred on the model origin at 10 pixels a
// unit: x = (px - 200) / 10, y = -(py - 150) / 10.

#include <gtest/gtest.h>

#include <QMouseEvent>

#include "katana/cad/view_set.hpp"
#include "katana/commands/entity_commands.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::cad::SnapMode;
using katana::cad::ViewKind;
using katana::cad::ViewSet;
using katana::cad::ViewState;
using katana::entity::PointGeometry;
using katana::geometry::Point2;
using katana::qt::ViewportWidget;
using katana::qt::test::paint;

namespace {

struct Fixture {
    Document document;
    ViewSet views;
    ViewState& state;
    ViewportWidget view;

    Fixture() : state(views.add(ViewKind::Plan)), view(document, state)
    {
        state.planFramed = true;
        state.plan.center = Point2(0.0, 0.0);
        state.plan.scale = 10.0;
        state.plan.resize(400.0, 300.0);
        view.resize(400, 300);
        EXPECT_TRUE(
            document.execute(katana::commands::createLine(Point2(0, 0), Point2(10, 10))).ok());
        document.drafting().snapEnabled = true;
        document.drafting().snapModes = static_cast<katana::cad::SnapModes>(SnapMode::Endpoint);
        paint(view);
    }

    void move(double x, double y)
    {
        const QPointF at(x, y);
        QMouseEvent moved(QEvent::MouseMove, at, view.mapToGlobal(at), Qt::NoButton, Qt::NoButton,
                          Qt::NoModifier);
        QCoreApplication::sendEvent(&view, &moved);
    }
    void click(double x, double y)
    {
        const QPointF at(x, y);
        QMouseEvent down(QEvent::MouseButtonPress, at, view.mapToGlobal(at), Qt::LeftButton,
                         Qt::LeftButton, Qt::NoModifier);
        QCoreApplication::sendEvent(&view, &down);
        QMouseEvent up(QEvent::MouseButtonRelease, at, view.mapToGlobal(at), Qt::LeftButton,
                       Qt::NoButton, Qt::NoModifier);
        QCoreApplication::sendEvent(&view, &up);
    }
    // The point the last Point tool click made.
    [[nodiscard]] Point2 lastPoint() const
    {
        const auto made = document.lastCreatedEntities();
        EXPECT_EQ(made.size(), 1u);
        if (made.empty()) {
            return Point2{};
        }
        return std::get<PointGeometry>(document.model().entities.find(made[0])->geometry).position;
    }
};

} // namespace

TEST(PlanViewTracking, ASnappedEndIsAcquiredAndItsHorizontalTakesThePoint)
{
    Fixture f;
    f.document.drafting().objectTracking = true;
    ASSERT_TRUE(f.view.startTool("draw.point").ok());
    f.move(300, 50); // onto the line's end at (10, 10): acquired
    ASSERT_EQ(f.view.trackingPoints().size(), 1u);
    EXPECT_EQ(f.view.trackingPoints().front(), Point2(10, 10));
    // (3, 10.2) is two pixels off the horizontal through (10, 10).
    f.move(230, 48);
    f.click(230, 48);
    const Point2 made = f.lastPoint();
    EXPECT_DOUBLE_EQ(made.x, 3.0);
    EXPECT_DOUBLE_EQ(made.y, 10.0) << "on the tracking path exactly";
    paint(f.view); // the acquired cross and the dotted path draw
}

TEST(PlanViewTracking, WithTrackingOffNothingIsAcquiredAndThePointIsWhereClicked)
{
    Fixture f;
    ASSERT_TRUE(f.view.startTool("draw.point").ok());
    f.move(300, 50);
    EXPECT_TRUE(f.view.trackingPoints().empty());
    f.move(230, 48);
    f.click(230, 48);
    EXPECT_NEAR(f.lastPoint().y, 10.2, 1e-9);
}

TEST(PlanViewTracking, TwoAcquiredPointsTrackToWherePathsCross)
{
    Fixture f;
    f.document.drafting().objectTracking = true;
    ASSERT_TRUE(f.view.startTool("draw.point").ok());
    f.move(200, 150); // (0, 0)
    f.move(300, 50);  // (10, 10)
    ASSERT_EQ(f.view.trackingPoints().size(), 2u);
    // Near (10, 0): the vertical through (10, 10) crosses the horizontal
    // through (0, 0).
    f.move(301, 151);
    f.click(301, 151);
    EXPECT_EQ(f.lastPoint(), Point2(10, 0));
}

TEST(PlanViewTracking, TheAcquiredPointsAreForgottenWhenNoToolWantsAPoint)
{
    Fixture f;
    f.document.drafting().objectTracking = true;
    ASSERT_TRUE(f.view.startTool("draw.point").ok());
    f.move(300, 50);
    ASSERT_FALSE(f.view.trackingPoints().empty());
    f.view.setTool(katana::qt::Tool::Select);
    f.move(250, 100);
    EXPECT_TRUE(f.view.trackingPoints().empty());
}
