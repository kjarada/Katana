// Grips in the plan view (src/katana_qt/drawing/grip_controller.*), driven
// with real mouse and key events: the view draws the selection's grips, a
// press on one and a drag moves it, every drag is one undo step, several hot
// grips move together, Delete removes hot vertices and Esc drops a grip.
// What each grip does to the geometry is katana_cad's and tested there
// (tests/cad/drawing/test_grips.cpp); these test the gesture.

#include <gtest/gtest.h>

#include <QKeyEvent>
#include <QMouseEvent>

#include "katana/cad/view_set.hpp"
#include "katana/commands/entity_commands.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::cad::ViewKind;
using katana::cad::ViewSet;
using katana::cad::ViewState;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::qt::ViewportWidget;
using katana::qt::test::paint;

namespace {

// A 400 x 300 view at 10 px a unit centred on (10, 5), so model (0, 0) is at
// pixel (100, 200) and a unit is ten pixels.
struct Fixture {
    Document document;
    ViewSet views;
    ViewState* state = nullptr;
    std::unique_ptr<ViewportWidget> view;
    EntityId polyline = 0;

    Fixture()
    {
        EXPECT_TRUE(document
                        .execute(katana::commands::createPolyline(Polyline2{
                            {Point2(0, 0), Point2(10, 0), Point2(20, 0)}, false}))
                        .ok());
        polyline = document.lastCreatedEntities().front();
        state = &views.add(ViewKind::Plan);
        state->planFramed = true;
        state->plan.center = Point2(10.0, 5.0);
        state->plan.scale = 10.0;
        view = std::make_unique<ViewportWidget>(document, *state);
        view->resize(400, 300);
        paint(*view);
        view->state().plan.center = Point2(10.0, 5.0);
        view->state().plan.scale = 10.0;
        // Nothing snaps, so a drag lands exactly where the mouse is.
        view->setSnapEnabled(false);
    }

    [[nodiscard]] QPointF pixel(const Point2& world) const
    {
        const Point2 p = state->plan.worldToScreen(world);
        return QPointF(p.x, p.y);
    }

    void select()
    {
        document.selection().add(polyline);
        document.notifySelectionChanged();
        paint(*view);
    }

    void mouse(QEvent::Type type, const Point2& world, Qt::MouseButton button,
               Qt::KeyboardModifiers modifiers = Qt::NoModifier)
    {
        const QPointF at = pixel(world);
        const Qt::MouseButtons held = type == QEvent::MouseButtonRelease ? Qt::NoButton : button;
        QMouseEvent event(type, at, view->mapToGlobal(at), button, held, modifiers);
        QCoreApplication::sendEvent(view.get(), &event);
    }

    void drag(const Point2& from, const Point2& to, Qt::KeyboardModifiers modifiers = Qt::NoModifier)
    {
        mouse(QEvent::MouseButtonPress, from, Qt::LeftButton, modifiers);
        mouse(QEvent::MouseMove, (from + to) * 0.5, Qt::LeftButton, modifiers);
        mouse(QEvent::MouseMove, to, Qt::LeftButton, modifiers);
        mouse(QEvent::MouseButtonRelease, to, Qt::LeftButton, modifiers);
    }

    void key(int code, const QString& text = {})
    {
        QKeyEvent press(QEvent::KeyPress, code, Qt::NoModifier, text);
        QCoreApplication::sendEvent(view.get(), &press);
    }

    [[nodiscard]] const Polyline2& geometry() const
    {
        return std::get<Polyline2>(document.model().entities.find(polyline)->geometry);
    }
};

} // namespace

TEST(PlanViewGrips, TheSelectionShowsItsGripsAndNothingElseDoes)
{
    Fixture f;
    auto& grips = f.view->gripController();
    grips.refresh(0);
    EXPECT_TRUE(grips.grips().empty()) << "nothing selected, no grips";
    f.select();
    // Three vertices and two segment middles.
    EXPECT_EQ(grips.grips().size(), 5u);
}

TEST(PlanViewGrips, DraggingAVertexGripMovesItAsOneUndoStep)
{
    Fixture f;
    f.select();
    const std::size_t before = f.document.history().undoCount();
    f.drag(Point2(10, 0), Point2(10, 6));
    EXPECT_EQ(f.geometry().vertices[1], Point2(10, 6));
    EXPECT_EQ(f.document.history().undoCount(), before + 1);
    EXPECT_EQ(f.document.history().undoName(), "GRIP_EDIT");
    ASSERT_TRUE(f.document.undo().ok());
    EXPECT_EQ(f.geometry().vertices[1], Point2(10, 0));
}

TEST(PlanViewGrips, AClickPicksAGripUpAndTheNextClickPutsItDown)
{
    Fixture f;
    f.select();
    f.mouse(QEvent::MouseButtonPress, Point2(20, 0), Qt::LeftButton);
    f.mouse(QEvent::MouseButtonRelease, Point2(20, 0), Qt::LeftButton);
    EXPECT_TRUE(f.view->gripController().active()) << "picked up";
    f.mouse(QEvent::MouseMove, Point2(22, 3), Qt::NoButton);
    f.mouse(QEvent::MouseButtonPress, Point2(22, 3), Qt::LeftButton);
    f.mouse(QEvent::MouseButtonRelease, Point2(22, 3), Qt::LeftButton);
    EXPECT_EQ(f.geometry().vertices[2], Point2(22, 3));
    EXPECT_FALSE(f.view->gripController().active());
}

TEST(PlanViewGrips, AGripPickedUpTakesATypedPoint)
{
    Fixture f;
    f.select();
    f.mouse(QEvent::MouseButtonPress, Point2(0, 0), Qt::LeftButton);
    f.mouse(QEvent::MouseButtonRelease, Point2(0, 0), Qt::LeftButton);
    for (const QChar c : QString("@3,4")) {
        f.key(0, QString(c));
    }
    f.key(Qt::Key_Return);
    EXPECT_EQ(f.geometry().vertices[0], Point2(3, 4));
}

TEST(PlanViewGrips, ShiftClickedGripsMoveTogether)
{
    Fixture f;
    f.select();
    f.mouse(QEvent::MouseButtonPress, Point2(10, 0), Qt::LeftButton, Qt::ShiftModifier);
    f.mouse(QEvent::MouseButtonRelease, Point2(10, 0), Qt::LeftButton, Qt::ShiftModifier);
    f.mouse(QEvent::MouseButtonPress, Point2(20, 0), Qt::LeftButton, Qt::ShiftModifier);
    f.mouse(QEvent::MouseButtonRelease, Point2(20, 0), Qt::LeftButton, Qt::ShiftModifier);
    EXPECT_EQ(f.view->gripController().hot().size(), 2u);
    // Grabbing one of the hot grips moves both.
    f.drag(Point2(20, 0), Point2(20, 5));
    EXPECT_EQ(f.geometry().vertices[1], Point2(10, 5));
    EXPECT_EQ(f.geometry().vertices[2], Point2(20, 5));
    EXPECT_EQ(f.geometry().vertices[0], Point2(0, 0));
}

TEST(PlanViewGrips, CtrlDraggingASegmentMiddleInsertsAVertex)
{
    Fixture f;
    f.select();
    f.drag(Point2(5, 0), Point2(5, -3), Qt::ControlModifier);
    ASSERT_EQ(f.geometry().vertices.size(), 4u);
    EXPECT_EQ(f.geometry().vertices[1], Point2(5, -3));
}

TEST(PlanViewGrips, DeleteRemovesTheHotVertexAndEscDropsAGrip)
{
    Fixture f;
    f.select();
    // A click makes the middle vertex hot (and picks it up); Delete removes it.
    f.mouse(QEvent::MouseButtonPress, Point2(10, 0), Qt::LeftButton);
    f.mouse(QEvent::MouseButtonRelease, Point2(10, 0), Qt::LeftButton);
    f.key(Qt::Key_Delete);
    EXPECT_EQ(f.geometry().vertices.size(), 2u);
    EXPECT_EQ(f.document.model().entities.size(), 1u) << "the vertex, not the polyline";

    // Esc drops a picked-up grip without changing anything.
    const std::size_t before = f.document.history().undoCount();
    f.mouse(QEvent::MouseButtonPress, Point2(20, 0), Qt::LeftButton);
    f.mouse(QEvent::MouseButtonRelease, Point2(20, 0), Qt::LeftButton);
    f.mouse(QEvent::MouseMove, Point2(25, 5), Qt::NoButton);
    f.key(Qt::Key_Escape);
    EXPECT_FALSE(f.view->gripController().active());
    EXPECT_EQ(f.document.history().undoCount(), before);
    EXPECT_EQ(f.geometry().vertices[1], Point2(20, 0));
}

TEST(PlanViewGrips, AGripDragSnapsToAnEndpoint)
{
    Fixture f;
    ASSERT_TRUE(f.document.execute(katana::commands::createPoint(Point2(15.2, 7.1))).ok());
    f.view->setSnapEnabled(true);
    f.select();
    // Dropped half a unit (5 px) from the point: the endpoint snap takes it.
    f.drag(Point2(10, 0), Point2(15.6, 7.4));
    EXPECT_EQ(f.geometry().vertices[1], Point2(15.2, 7.1));
}

TEST(PlanViewGrips, OrthoHoldsAGripDragSquareToItsBase)
{
    Fixture f;
    f.document.drafting().ortho = true;
    f.select();
    f.drag(Point2(10, 0), Point2(11, 8));
    EXPECT_EQ(f.geometry().vertices[1], Point2(10, 8));
}
