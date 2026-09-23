// The plan view (src/katana_qt/viewport_widget): how it frames the drawing and
// what it culls. Positions are checked through the view's state, which is
// what the widget draws through (ViewState::plan).

#include <gtest/gtest.h>

#include <QWheelEvent>

#include "katana/cad/view_set.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/interop/reference_data.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::cad::ViewKind;
using katana::cad::ViewSet;
using katana::cad::ViewState;
using katana::geometry::Point2;
using katana::qt::ViewportWidget;
using katana::qt::test::paint;

namespace {

// True when `world` lands inside a `width` x `height` view.
bool onScreen(const ViewState& state, const Point2& world, double width, double height)
{
    const Point2 pixel = state.plan.worldToScreen(world);
    return pixel.x >= 0.0 && pixel.x <= width && pixel.y >= 0.0 && pixel.y <= height;
}

} // namespace

TEST(PlanView, AViewStillAsFramedKeepsTheWholeDrawingInViewWhenItShrinks)
{
    // The line (0, 0)-(100, 50) framed in a 400 x 300 view. Shrunk to 200 x
    // 300 - a 3D view opened beside it - keeping the scale would leave
    // 200 / 400 of the width it had, about 46 of its 100 units about x = 50,
    // with both ends off the edges. Framed again it shows the whole line.
    Document document;
    ASSERT_TRUE(
        document.execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(100.0, 50.0)))
            .ok());
    ViewSet views;
    ViewState& state = views.add(ViewKind::Plan);
    ViewportWidget view(document, state);
    view.resize(400, 300);
    paint(view);
    ASSERT_TRUE(state.planFramed);
    EXPECT_TRUE(onScreen(state, Point2(0.0, 0.0), 400.0, 300.0));
    EXPECT_TRUE(onScreen(state, Point2(100.0, 50.0), 400.0, 300.0));

    view.resize(200, 300);
    paint(view);
    EXPECT_TRUE(onScreen(state, Point2(0.0, 0.0), 200.0, 300.0));
    EXPECT_TRUE(onScreen(state, Point2(100.0, 50.0), 200.0, 300.0));
}

TEST(PlanView, AViewTheUserZoomedKeepsItsScaleWhenResized)
{
    Document document;
    ASSERT_TRUE(
        document.execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(100.0, 50.0)))
            .ok());
    ViewSet views;
    ViewState& state = views.add(ViewKind::Plan);
    ViewportWidget view(document, state);
    view.resize(400, 300);
    paint(view);
    const QPointF middle(200.0, 150.0);
    QWheelEvent wheel(middle, view.mapToGlobal(middle), QPoint(), QPoint(0, 120), Qt::NoButton,
                      Qt::NoModifier, Qt::NoScrollPhase, false);
    QCoreApplication::sendEvent(&view, &wheel);
    const double zoomed = state.plan.scale;

    view.resize(200, 300);
    paint(view);
    EXPECT_EQ(state.plan.scale, zoomed);
}

TEST(PlanView, ADimensionWhoseLabelAloneIsOnScreenIsDrawn)
{
    // Audit QT-25. The dimension from (0, 0) to (10, 0) with its line 2
    // above: the Standard style puts the label textGap 0.625 above that line
    // and textHeight 2.5 tall, centred on x = 5, so it spans y 2.625 to
    // 5.125. A 400 x 300 view at 100 px a unit centred on (5, 4.5) shows
    // x 3..7, y 3..6: the label's middle, but not the dimension line (y = 2),
    // the extension lines (x = 0 and 10) or the arrows (2.5 long, so x <= 2.5
    // and x >= 7.5). The geometry's own box, (0, 0)-(10, 2), is off screen,
    // and culling on it left the dimension out.
    Document document;
    katana::entity::DimensionGeometry dimension;
    dimension.start = Point2(0.0, 0.0);
    dimension.end = Point2(10.0, 0.0);
    dimension.offset = 2.0;
    ASSERT_TRUE(document.execute(katana::commands::createDimension(dimension)).ok());
    ViewSet views;
    ViewState& state = views.add(ViewKind::Plan);
    state.planFramed = true; // this test's own view, not a frame of the drawing
    state.plan.center = Point2(5.0, 4.5);
    state.plan.scale = 100.0;
    ViewportWidget view(document, state);
    view.resize(400, 300);
    paint(view);
    EXPECT_EQ(view.lastDrawnEntityCount(), 1u);
}

TEST(PlanView, AnImageHiddenInTheViewIsLeftOutOfItsExtents)
{
    // A 2 x 2 pixel image placed by the geotransform (1000, 1, 0, 2000, 0,
    // -1): x = 1000 + column, y = 2000 - row, so its corners span (1000,
    // 1998)-(1002, 2000). With the line (0, 0)-(100, 50) the view draws
    // (0, 0)-(1002, 2000); with the image hidden in this view only the line,
    // (0, 0)-(100, 50).
    Document document;
    ASSERT_TRUE(
        document.execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(100.0, 50.0)))
            .ok());
    katana::interop::ReferenceData reference;
    katana::interop::RasterOverlay image;
    image.name = "ortho";
    image.width = 2;
    image.height = 2;
    image.rgba.assign(2 * 2 * 4, 255);
    image.geotransform = {1000.0, 1.0, 0.0, 2000.0, 0.0, -1.0};
    image.hasGeotransform = true;
    const katana::interop::ReferenceId id = reference.add(image);
    ViewSet views;
    ViewState& state = views.add(ViewKind::Plan);
    ViewportWidget view(document, state);
    view.setReferenceData(&reference);

    katana::geometry::Box2 drawn = view.drawnBounds();
    EXPECT_EQ(drawn.min, Point2(0.0, 0.0));
    EXPECT_EQ(drawn.max, Point2(1002.0, 2000.0));

    state.hiddenReferences.insert(id);
    drawn = view.drawnBounds();
    EXPECT_EQ(drawn.min, Point2(0.0, 0.0));
    EXPECT_EQ(drawn.max, Point2(100.0, 50.0));
}
