// Grips in the plan view (src/katana_qt/drawing/grip_controller.*), driven
// with real mouse and key events: the view draws the selection's grips, a
// press on one and a drag moves it, every drag is one undo step, several hot
// grips move together, Delete removes hot vertices and Esc drops a grip.
// What each grip does to the geometry is katana_cad's and tested there
// (tests/cad/drawing/test_grips.cpp); these test the gesture.

#include <gtest/gtest.h>

#include <cmath>

#include <QImage>
#include <QKeyEvent>
#include <QMouseEvent>

#include "katana/cad/drawing/vertex_editing.hpp"
#include "katana/cad/view_set.hpp"
#include "drawing/feedback_painter.hpp"
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

TEST(PlanViewGrips, AGripPickedUpBeforeAToolIsDroppedNotCommittedLater)
{
    // Click a vertex, then choose a tool - the owner's way in. The grip the
    // click picked up used to stay attached under the tool, and the first
    // click after the tool ended put it down: a GRIP_EDIT nobody asked for.
    Fixture f;
    f.select();
    f.mouse(QEvent::MouseButtonPress, Point2(20, 0), Qt::LeftButton);
    f.mouse(QEvent::MouseButtonRelease, Point2(20, 0), Qt::LeftButton);
    ASSERT_TRUE(f.view->gripController().active()) << "picked up";
    const std::size_t before = f.document.history().undoCount();
    ASSERT_TRUE(f.view->startTool("draw.vertex.insert").ok());
    EXPECT_FALSE(f.view->gripController().active()) << "the tool took the grips";
    f.key(Qt::Key_Escape);
    ASSERT_FALSE(f.view->toolActive());
    f.mouse(QEvent::MouseMove, Point2(25, 5), Qt::NoButton);
    f.mouse(QEvent::MouseButtonPress, Point2(25, 5), Qt::LeftButton);
    f.mouse(QEvent::MouseButtonRelease, Point2(25, 5), Qt::LeftButton);
    EXPECT_EQ(f.document.history().undoCount(), before);
    EXPECT_EQ(f.geometry().vertices[2], Point2(20, 0));
}

TEST(PlanViewGrips, DeleteWithAHotVertexIsTheViewsKeyNotTheWindowsErase)
{
    // The window's Erase holds Delete as a shortcut. Qt offers the key to
    // the view first (ShortcutOverride, ignored until someone claims it); a
    // view that let it go had the whole polyline erased instead of the vertex.
    Fixture f;
    f.select();
    const auto claimed = [&f] {
        QKeyEvent offer(QEvent::ShortcutOverride, Qt::Key_Delete, Qt::NoModifier);
        offer.ignore();
        QCoreApplication::sendEvent(f.view.get(), &offer);
        return offer.isAccepted();
    };
    EXPECT_FALSE(claimed()) << "nothing hot: Delete is the window's Erase";
    f.mouse(QEvent::MouseButtonPress, Point2(10, 0), Qt::LeftButton, Qt::ShiftModifier);
    f.mouse(QEvent::MouseButtonRelease, Point2(10, 0), Qt::LeftButton, Qt::ShiftModifier);
    ASSERT_EQ(f.view->gripController().hot().size(), 1u);
    EXPECT_TRUE(claimed()) << "a hot vertex: Delete is the view's";
    f.key(Qt::Key_Delete);
    EXPECT_EQ(f.geometry().vertices, (std::vector<Point2>{Point2(0, 0), Point2(20, 0)}));
    EXPECT_NE(f.document.model().entities.find(f.polyline), nullptr) << "the polyline stays";
}

TEST(PlanViewGrips, InsideAVertexToolDeleteNeverErasesThePolylineItIsEditing)
{
    // A vertex tool works on a SELECTED polyline - a vertex's grips are
    // there only while it is, and the tool selects what it has just edited -
    // and the window's Erase holds Delete, so Delete inside the tool erased
    // the whole string: "Press Enter to delete vertex 1", Delete pressed
    // instead, and the polyline was gone. The view claims Delete while a
    // vertex tool runs: Delete Vertex takes it as its Enter, and the others
    // do nothing with it and say so.
    Fixture f;
    f.select();
    const auto claimed = [&f] {
        QKeyEvent offer(QEvent::ShortcutOverride, Qt::Key_Delete, Qt::NoModifier);
        offer.ignore();
        QCoreApplication::sendEvent(f.view.get(), &offer);
        return offer.isAccepted();
    };
    f.mouse(QEvent::MouseButtonPress, Point2(10, 0), Qt::LeftButton, Qt::ShiftModifier);
    f.mouse(QEvent::MouseButtonRelease, Point2(10, 0), Qt::LeftButton, Qt::ShiftModifier);
    ASSERT_EQ(f.view->gripController().hot().size(), 1u);
    ASSERT_TRUE(f.view->startTool("draw.vertex.delete").ok());
    EXPECT_TRUE(claimed()) << "Delete Vertex's, not the window's Erase";
    f.key(Qt::Key_Delete);
    EXPECT_EQ(f.geometry().vertices, (std::vector<Point2>{Point2(0, 0), Point2(20, 0)}))
        << "the chosen vertex, as Enter deletes it";
    ASSERT_NE(f.document.model().entities.find(f.polyline), nullptr) << "the polyline stays";
    ASSERT_TRUE(f.view->toolActive()) << "and the tool runs on";
    EXPECT_TRUE(claimed()) << "with nothing chosen now, still the tool's";
    const std::size_t undos = f.document.history().undoCount();
    f.key(Qt::Key_Delete);
    EXPECT_EQ(f.document.history().undoCount(), undos) << "nothing chosen, nothing deleted";
    f.key(Qt::Key_Escape);

    // Insert Vertex on the selected polyline: claimed, and refused.
    ASSERT_TRUE(f.view->startTool("draw.vertex.insert").ok());
    EXPECT_TRUE(claimed());
    f.key(Qt::Key_Delete);
    EXPECT_NE(f.document.model().entities.find(f.polyline), nullptr);
    EXPECT_EQ(f.document.history().undoCount(), undos);
    EXPECT_TRUE(f.view->toolActive());
    f.key(Qt::Key_Escape);
    ASSERT_FALSE(f.view->toolActive());
    // No tool and nothing hot: Delete is the window's Erase again.
    EXPECT_FALSE(claimed());
}

TEST(PlanViewGrips, AHotVertexWhoseVertexWentInTwoEditsBetweenPaintsGoesToo)
{
    // Chosen, then two edits before the view paints again - a SCRIPT, lines
    // pasted, an agent: the chosen vertex deleted, and another inserted
    // ahead of it, leaving as many vertices as before. Its index then named
    // (10,0), a vertex nobody chose, and Delete removed it. Its point is
    // gone and the vertices before it have moved, so it goes too.
    Fixture f;
    f.select();
    f.mouse(QEvent::MouseButtonPress, Point2(20, 0), Qt::LeftButton, Qt::ShiftModifier);
    f.mouse(QEvent::MouseButtonRelease, Point2(20, 0), Qt::LeftButton, Qt::ShiftModifier);
    ASSERT_EQ(f.view->gripController().hot().size(), 1u);
    ASSERT_TRUE(f.document
                    .execute(katana::cad::editPolyline(
                        f.polyline, "VERTEX_DELETE",
                        [](const katana::geometry::CurvePolyline2& shape) {
                            return katana::geometry::deleteVertex(shape, 2);
                        }))
                    .ok());
    ASSERT_TRUE(f.document
                    .execute(katana::cad::editPolyline(
                        f.polyline, "VERTEX_INSERT",
                        [](const katana::geometry::CurvePolyline2& shape) {
                            return katana::geometry::insertVertex(shape, 0, Point2(5, 0));
                        }))
                    .ok());
    paint(*f.view);
    EXPECT_TRUE(f.view->gripController().hot().empty());
    // Delete Vertex chosen next, as the review ran it: nothing is offered to
    // Enter, and Enter deletes nothing. (With no vertex hot, the Delete key
    // itself is Erase's, of the whole selection, as it always was.)
    ASSERT_TRUE(f.view->startTool("draw.vertex.delete").ok());
    f.key(Qt::Key_Return);
    EXPECT_EQ(f.geometry().vertices,
              (std::vector<Point2>{Point2(0, 0), Point2(5, 0), Point2(10, 0)}))
        << "nothing nobody chose is deleted";
}

TEST(PlanViewGrips, ADoubleClickOnAGripLeavesNothingPickedUp)
{
    Fixture f;
    f.select();
    int opened = 0;
    f.view->onEntityDoubleClicked = [&opened](EntityId) { ++opened; };
    const std::size_t before = f.document.history().undoCount();
    // Qt's double click: press, release, DOUBLE CLICK, release. The first
    // click picks the grip up.
    f.mouse(QEvent::MouseButtonPress, Point2(10, 0), Qt::LeftButton);
    f.mouse(QEvent::MouseButtonRelease, Point2(10, 0), Qt::LeftButton);
    f.mouse(QEvent::MouseButtonDblClick, Point2(10, 0), Qt::LeftButton);
    f.mouse(QEvent::MouseButtonRelease, Point2(10, 0), Qt::LeftButton);
    EXPECT_EQ(opened, 1) << "the double click still opens the polyline's editor";
    EXPECT_FALSE(f.view->gripController().active()) << "and leaves no grip following the cursor";
    f.mouse(QEvent::MouseMove, Point2(14, 6), Qt::NoButton);
    f.mouse(QEvent::MouseButtonPress, Point2(14, 6), Qt::LeftButton);
    f.mouse(QEvent::MouseButtonRelease, Point2(14, 6), Qt::LeftButton);
    EXPECT_EQ(f.document.history().undoCount(), before) << "no GRIP_EDIT";
    EXPECT_EQ(f.geometry().vertices[1], Point2(10, 0));
}

TEST(PlanViewGrips, AHotVertexFollowsItsVertexThroughAnInsertBeforeIt)
{
    Fixture f;
    f.select();
    f.mouse(QEvent::MouseButtonPress, Point2(20, 0), Qt::LeftButton, Qt::ShiftModifier);
    f.mouse(QEvent::MouseButtonRelease, Point2(20, 0), Qt::LeftButton, Qt::ShiftModifier);
    ASSERT_EQ(f.view->gripController().hot().size(), 1u);
    // Another edit - a tool, the panel, an agent - puts a vertex before it:
    // (20,0) is vertex 3 now, and vertex 2 is (10,0).
    ASSERT_TRUE(f.document
                    .execute(katana::cad::editPolyline(
                        f.polyline, "VERTEX_INSERT",
                        [](const katana::geometry::CurvePolyline2& shape) {
                            return katana::geometry::insertVertex(shape, 0, Point2(5, 0));
                        }))
                    .ok());
    paint(*f.view);
    f.key(Qt::Key_Delete);
    EXPECT_EQ(f.geometry().vertices,
              (std::vector<Point2>{Point2(0, 0), Point2(5, 0), Point2(10, 0)}))
        << "the vertex made hot goes, not the one now at its old index";
}

TEST(PlanViewGrips, AHoveredGripSaysWhatItIsAndWhatCanBeDone)
{
    Fixture f;
    f.select();
    auto& grips = f.view->gripController();
    const QString id = QString::number(f.polyline);
    f.mouse(QEvent::MouseMove, Point2(10, 0.1), Qt::NoButton);
    EXPECT_EQ(grips.hoverHint(),
              "Vertex 1 of polyline " + id +
                  ": drag to move · click to pick up · Shift+click to choose · Delete removes "
                  "the chosen · right-click for vertex tools");
    f.mouse(QEvent::MouseMove, Point2(5, 0.1), Qt::NoButton);
    EXPECT_EQ(grips.hoverHint(), "Segment 0 of polyline " + id +
                                     ": drag to stretch · Ctrl+drag to add a vertex · "
                                     "right-click for segment tools");
    f.mouse(QEvent::MouseMove, Point2(12, 6), Qt::NoButton);
    EXPECT_TRUE(grips.hoverHint().isEmpty()) << "no grip under the cursor, nothing to say";
    // A grip picked up says what to do with it instead (its prompt).
    f.mouse(QEvent::MouseButtonPress, Point2(20, 0), Qt::LeftButton);
    f.mouse(QEvent::MouseButtonRelease, Point2(20, 0), Qt::LeftButton);
    EXPECT_TRUE(grips.hoverHint().isEmpty());
    EXPECT_FALSE(grips.prompt().isEmpty());
}

TEST(PlanViewGrips, AVertexWithAHeightSaysItsHeight)
{
    Fixture f;
    // Heights 100, none, 101.25: vertex 2's hint gives its own.
    ASSERT_TRUE(f.document
                    .execute(katana::cad::editPolyline(
                        f.polyline, "VERTEX_Z",
                        [](const katana::geometry::CurvePolyline2& shape) {
                            return katana::geometry::setVertexHeight(shape, 2, 101.25);
                        }))
                    .ok());
    f.select();
    f.mouse(QEvent::MouseMove, Point2(20, 0.1), Qt::NoButton);
    EXPECT_TRUE(f.view->gripController().hoverHint().startsWith(
        "Vertex 2 of polyline " + QString::number(f.polyline) + ", z 101.250: "))
        << f.view->gripController().hoverHint().toStdString();
    // A height a hair under zero reads as the vertex tools' labels read it
    // (cad::heightText): "z 0.000". The band wrote it with its own
    // formatting, "z -0.000", beside a tool's "z 0.000" for the same vertex.
    ASSERT_TRUE(f.document
                    .execute(katana::cad::editPolyline(
                        f.polyline, "VERTEX_Z",
                        [](const katana::geometry::CurvePolyline2& shape) {
                            return katana::geometry::setVertexHeight(shape, 1, -0.0004);
                        }))
                    .ok());
    paint(*f.view);
    f.mouse(QEvent::MouseMove, Point2(10, 0.1), Qt::NoButton);
    EXPECT_TRUE(f.view->gripController().hoverHint().startsWith(
        "Vertex 1 of polyline " + QString::number(f.polyline) + ", z 0.000: "))
        << f.view->gripController().hoverHint().toStdString();
    EXPECT_EQ(QString::fromStdString(katana::cad::heightText(-0.0004)), "z 0.000");
}

TEST(PlanViewGrips, CtrlOverASegmentMiddleShowsTheVertexADragWouldAdd)
{
    Fixture f;
    f.select();
    // Without Ctrl the middle is the hovered grip's green diamond; with it,
    // the new vertex's cyan disc: more cyan round the middle's pixel.
    const QPointF middle = f.pixel(Point2(5, 0));
    const QColor cyan = katana::qt::drawing::overlay::preview();
    const auto cyanNear = [&](const QImage& image) {
        int count = 0;
        for (int y = int(middle.y()) - 6; y <= int(middle.y()) + 6; ++y) {
            for (int x = int(middle.x()) - 6; x <= int(middle.x()) + 6; ++x) {
                const QColor pixel = image.pixelColor(x, y);
                count += std::abs(pixel.red() - cyan.red()) <= 24 &&
                                 std::abs(pixel.green() - cyan.green()) <= 24 &&
                                 std::abs(pixel.blue() - cyan.blue()) <= 24
                             ? 1
                             : 0;
            }
        }
        return count;
    };
    f.mouse(QEvent::MouseMove, Point2(5, 0.1), Qt::NoButton);
    const QImage plain = f.view->grab().toImage();
    f.mouse(QEvent::MouseMove, Point2(5, 0.1), Qt::NoButton, Qt::ControlModifier);
    ASSERT_TRUE(f.view->gripController().ctrl());
    const QImage held = f.view->grab().toImage();
    EXPECT_GT(cyanNear(held), cyanNear(plain));
    // Let go of Ctrl: the diamond again.
    QKeyEvent release(QEvent::KeyRelease, Qt::Key_Control, Qt::NoModifier);
    QCoreApplication::sendEvent(f.view.get(), &release);
    EXPECT_FALSE(f.view->gripController().ctrl());
}

TEST(PlanViewGrips, AHotVertexStaysChosenWhenItIsItselfMoved)
{
    // Chosen, then moved by another edit (VERTEX MOVE, a Vertices panel
    // cell, another view): matched again by its position alone, the chosen
    // vertex was silently unchosen. As many vertices as before means none
    // was inserted or deleted ahead of it, so its index still names it.
    Fixture f;
    f.select();
    f.mouse(QEvent::MouseButtonPress, Point2(20, 0), Qt::LeftButton, Qt::ShiftModifier);
    f.mouse(QEvent::MouseButtonRelease, Point2(20, 0), Qt::LeftButton, Qt::ShiftModifier);
    ASSERT_EQ(f.view->gripController().hot().size(), 1u);
    ASSERT_TRUE(f.document
                    .execute(katana::cad::editPolyline(
                        f.polyline, "VERTEX_MOVE",
                        [](const katana::geometry::CurvePolyline2& shape) {
                            return katana::geometry::moveVertex(shape, 2, Point2(22, 3));
                        }))
                    .ok());
    paint(*f.view);
    ASSERT_EQ(f.view->gripController().hot().size(), 1u) << "still chosen";
    EXPECT_EQ(f.view->gripController().hot().front().index, 2u);
    EXPECT_EQ(f.view->gripController().hot().front().position, Point2(22, 3));
    f.key(Qt::Key_Delete);
    EXPECT_EQ(f.geometry().vertices, (std::vector<Point2>{Point2(0, 0), Point2(10, 0)}));
}

TEST(PlanViewGrips, AWordTypedAfterAClickOnAGripGoesToTheCommandLineAndTheGripStaysChosen)
{
    // A plain click on a vertex is how it is chosen, and it also picks the
    // grip up to take a typed point. The tool's name typed next - "click the
    // vertex, then type INSERTVERTEX" - went into the grip's point and was
    // refused there. Nothing a grip takes starts with a letter: the grip is
    // put down, still chosen, and the word goes to the command line.
    Fixture f;
    std::vector<QString> typed;
    f.view->onTextTyped = [&typed](const QString& text) { typed.push_back(text); };
    f.select();
    f.mouse(QEvent::MouseButtonPress, Point2(10, 0), Qt::LeftButton);
    f.mouse(QEvent::MouseButtonRelease, Point2(10, 0), Qt::LeftButton);
    ASSERT_TRUE(f.view->gripController().active()) << "picked up";
    f.key(Qt::Key_I, "I");
    f.key(Qt::Key_N, "N");
    EXPECT_EQ(typed, (std::vector<QString>{"I", "N"}));
    EXPECT_FALSE(f.view->gripController().active()) << "put down";
    ASSERT_EQ(f.view->gripController().hot().size(), 1u) << "and still chosen";
    EXPECT_EQ(f.view->gripController().hot().front().index, 1u);
    EXPECT_EQ(f.geometry().vertices[1], Point2(10, 0)) << "nothing moved";
    // A point typed after the click still goes to the grip.
    f.mouse(QEvent::MouseButtonPress, Point2(0, 0), Qt::LeftButton);
    f.mouse(QEvent::MouseButtonRelease, Point2(0, 0), Qt::LeftButton);
    for (const QChar c : QString("@3,4")) {
        f.key(0, QString(c));
    }
    f.key(Qt::Key_Return);
    EXPECT_EQ(f.geometry().vertices[0], Point2(3, 4));
}

TEST(PlanViewGrips, AnArcsMiddleSaysADragBendsIt)
{
    // An arc segment's middle keeps both ends and bends the arc; its hint
    // said "drag to stretch", a straight segment's gesture.
    Fixture f;
    ASSERT_TRUE(f.document
                    .execute(katana::cad::editPolyline(
                        f.polyline, "SEGMENT_ARC",
                        [](const katana::geometry::CurvePolyline2& shape) {
                            return katana::geometry::segmentToArc(shape, 0, Point2(5, 3));
                        }))
                    .ok());
    f.select();
    auto& grips = f.view->gripController();
    const QString id = QString::number(f.polyline);
    // The arc through (5,3) from (0,0) to (10,0) has its middle there.
    f.mouse(QEvent::MouseMove, Point2(5, 3.1), Qt::NoButton);
    EXPECT_EQ(grips.hoverHint(), "Segment 0 of polyline " + id +
                                     ": drag to bend the arc · Ctrl+drag to add a vertex · "
                                     "right-click for segment tools");
    f.mouse(QEvent::MouseMove, Point2(15, 0.1), Qt::NoButton);
    EXPECT_TRUE(grips.hoverHint().contains("drag to stretch")) << grips.hoverHint().toStdString();
}

TEST(PlanViewGrips, ACtrlPressOnASegmentMiddleSaysItAddsAVertex)
{
    // A Ctrl-press on a middle adds a vertex where it is put down; the band
    // said what a stretch says, word for word.
    Fixture f;
    f.select();
    auto& grips = f.view->gripController();
    f.mouse(QEvent::MouseButtonPress, Point2(15, 0), Qt::LeftButton, Qt::ControlModifier);
    f.mouse(QEvent::MouseButtonRelease, Point2(15, 0), Qt::LeftButton, Qt::ControlModifier);
    ASSERT_TRUE(grips.active());
    EXPECT_TRUE(grips.prompt().startsWith("Grip segment middle, adding a vertex: "))
        << grips.prompt().toStdString();
    f.key(Qt::Key_Escape);
    f.mouse(QEvent::MouseButtonPress, Point2(15, 0), Qt::LeftButton);
    f.mouse(QEvent::MouseButtonRelease, Point2(15, 0), Qt::LeftButton);
    ASSERT_TRUE(grips.active());
    EXPECT_FALSE(grips.prompt().contains("adding a vertex")) << grips.prompt().toStdString();
}
