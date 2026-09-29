// The catalogue's tools hosted in a real plan view (src/katana_qt/
// viewport_widget with tools/tool_host): clicks, typing, Enter, Esc and a
// right-click sent to the widget as Qt events, and the result read back from
// the Document - its entities and its undo history.
//
// Every view here is 400 x 300 pixels, centred on the model origin at 10
// pixels a unit, and never framed, so a pixel's model point is worked by
// hand from ViewTransform::screenToWorld:
//     x = (px - 200) / 10        y = -(py - 150) / 10
// e.g. pixel (300, 50) is model (10, 10) and pixel (250, 100) is (5, 5).

#include <gtest/gtest.h>

#include <string>
#include <variant>
#include <vector>

#include <QKeyEvent>
#include <QMouseEvent>

#include "katana/cad/view_set.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::cad::ViewKind;
using katana::cad::ViewSet;
using katana::cad::ViewState;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::geometry::Segment2;
using katana::qt::Tool;
using katana::qt::ViewportWidget;
using katana::qt::test::paint;

namespace {

// A plan view on a fresh document at the scale the header works through.
// Snapping is off unless a test turns it on, so a click lands exactly where
// the hand-worked sum says.
struct PlanFixture {
    Document document;
    ViewSet views;
    ViewState& state;
    ViewportWidget view;
    std::vector<QString> errors;
    std::vector<QString> typedOut;

    PlanFixture() : state(views.add(ViewKind::Plan)), view(document, state)
    {
        state.planFramed = true; // never framed: the centre and scale stay put
        state.plan.center = Point2(0.0, 0.0);
        state.plan.scale = 10.0;
        state.plan.resize(400.0, 300.0);
        view.resize(400, 300);
        view.setSnapEnabled(false);
        view.onError = [this](const QString& text) { errors.push_back(text); };
        view.onTextTyped = [this](const QString& text) { typedOut.push_back(text); };
        paint(view);
    }

    void press(double x, double y, Qt::MouseButton button = Qt::LeftButton,
               Qt::KeyboardModifiers modifiers = Qt::NoModifier)
    {
        const QPointF at(x, y);
        QMouseEvent down(QEvent::MouseButtonPress, at, view.mapToGlobal(at), button, button,
                         modifiers);
        QCoreApplication::sendEvent(&view, &down);
        QMouseEvent up(QEvent::MouseButtonRelease, at, view.mapToGlobal(at), button,
                       Qt::NoButton, modifiers);
        QCoreApplication::sendEvent(&view, &up);
    }
    // A press at one pixel and the release at another: a selection box.
    void drag(QPointF from, QPointF to, Qt::KeyboardModifiers modifiers = Qt::NoModifier)
    {
        QMouseEvent down(QEvent::MouseButtonPress, from, view.mapToGlobal(from), Qt::LeftButton,
                         Qt::LeftButton, modifiers);
        QCoreApplication::sendEvent(&view, &down);
        QMouseEvent up(QEvent::MouseButtonRelease, to, view.mapToGlobal(to), Qt::LeftButton,
                       Qt::NoButton, modifiers);
        QCoreApplication::sendEvent(&view, &up);
    }
    void move(double x, double y)
    {
        const QPointF at(x, y);
        QMouseEvent moved(QEvent::MouseMove, at, view.mapToGlobal(at), Qt::NoButton,
                          Qt::NoButton, Qt::NoModifier);
        QCoreApplication::sendEvent(&view, &moved);
    }
    void key(int code, Qt::KeyboardModifiers modifiers = Qt::NoModifier, const QString& text = {})
    {
        // A shortcut is offered to the widget first, as Qt does, so the
        // view's ShortcutOverride handling is exercised too.
        QKeyEvent override(QEvent::ShortcutOverride, code, modifiers, text);
        QCoreApplication::sendEvent(&view, &override);
        QKeyEvent down(QEvent::KeyPress, code, modifiers, text);
        QCoreApplication::sendEvent(&view, &down);
    }
    // Types `text` a character at a time, as a keyboard does.
    void type(const QString& text)
    {
        for (const QChar c : text) {
            key(c.toUpper().unicode(), Qt::NoModifier, QString(c));
        }
    }
    void enter() { key(Qt::Key_Return); }
    void escape() { key(Qt::Key_Escape); }

    [[nodiscard]] std::vector<const katana::entity::Entity*> entities() const
    {
        std::vector<const katana::entity::Entity*> out;
        document.model().entities.forEach(
            [&](const katana::entity::Entity& entity) { out.push_back(&entity); });
        return out;
    }
    [[nodiscard]] std::size_t undoSteps() const { return document.history().undoCount(); }
};

void expectSegment(const katana::entity::Entity* entity, Point2 start, Point2 end)
{
    ASSERT_NE(entity, nullptr);
    const auto* segment = std::get_if<Segment2>(&entity->geometry);
    ASSERT_NE(segment, nullptr) << "not a line";
    EXPECT_NEAR(segment->start.x, start.x, 1e-9);
    EXPECT_NEAR(segment->start.y, start.y, 1e-9);
    EXPECT_NEAR(segment->end.x, end.x, 1e-9);
    EXPECT_NEAR(segment->end.y, end.y, 1e-9);
}

EntityId addLine(Document& document, Point2 start, Point2 end)
{
    EXPECT_TRUE(document.execute(katana::commands::createLine(start, end)).ok());
    return document.lastCreatedEntities().front();
}

} // namespace

TEST(PlanViewTools, ALineFromTheCatalogueIsTwoClicksAndEnterAndOneUndoStep)
{
    PlanFixture plan;
    ASSERT_TRUE(plan.view.startTool("draw.line").ok());
    EXPECT_EQ(plan.view.activeToolId(), "draw.line");

    plan.press(200, 150); // model (0, 0)
    plan.press(300, 50);  // model (10, 10)
    EXPECT_TRUE(plan.entities().empty()) << "the chain is the tool's until Enter";
    plan.enter();

    const auto made = plan.entities();
    ASSERT_EQ(made.size(), 1u);
    expectSegment(made.front(), Point2(0, 0), Point2(10, 10));
    EXPECT_EQ(plan.undoSteps(), 1u);
    EXPECT_TRUE(plan.errors.empty()) << plan.errors.front().toStdString();
}

TEST(PlanViewTools, ATypedRelativePointIsMeasuredFromTheLastClick)
{
    PlanFixture plan;
    ASSERT_TRUE(plan.view.startTool("draw.line").ok());
    plan.press(250, 100); // model (5, 5)
    plan.type("@10,0");   // (5 + 10, 5 + 0) = (15, 5)
    EXPECT_EQ(plan.view.typedInput(), "@10,0") << "shown after the prompt until Enter";
    EXPECT_TRUE(plan.typedOut.empty()) << "a tool's input is not a command for the window";
    plan.enter(); // sends the typed point
    plan.enter(); // finishes the chain

    const auto made = plan.entities();
    ASSERT_EQ(made.size(), 1u);
    expectSegment(made.front(), Point2(5, 5), Point2(15, 5));
    EXPECT_EQ(plan.undoSteps(), 1u);
}

TEST(PlanViewTools, APolarPointTypedAfterAClickGoesThatFarAtThatAngle)
{
    PlanFixture plan;
    ASSERT_TRUE(plan.view.startTool("draw.line").ok());
    plan.press(200, 150); // model (0, 0)
    // @5<90: 5 units at 90 degrees counter-clockwise from east, due north.
    plan.type("@5<90");
    plan.enter();
    plan.enter();

    const auto made = plan.entities();
    ASSERT_EQ(made.size(), 1u);
    expectSegment(made.front(), Point2(0, 0), Point2(0, 5));
}

TEST(PlanViewTools, EscAfterOneClickCreatesNothingAndEndsTheTool)
{
    PlanFixture plan;
    ASSERT_TRUE(plan.view.startTool("draw.line").ok());
    plan.press(200, 150);
    plan.escape();

    EXPECT_TRUE(plan.entities().empty());
    EXPECT_EQ(plan.undoSteps(), 0u);
    EXPECT_FALSE(plan.view.toolActive());
}

TEST(PlanViewTools, EscAfterASegmentKeepsTheLinesDrawnAsAutoCadDoes)
{
    // A LINE's segments are finished work once both ends are given; Esc
    // ends the tool, and keeps them (ToolHost::cancel, the Line tool's own
    // InteractiveTool::cancel).
    PlanFixture plan;
    ASSERT_TRUE(plan.view.startTool("draw.line").ok());
    plan.press(200, 150); // (0, 0)
    plan.press(300, 150); // (10, 0)
    plan.escape();

    const auto made = plan.entities();
    ASSERT_EQ(made.size(), 1u);
    expectSegment(made.front(), Point2(0, 0), Point2(10, 0));
    EXPECT_EQ(plan.undoSteps(), 1u);
    EXPECT_FALSE(plan.view.toolActive());
}

TEST(PlanViewTools, TheFirstEscTakesBackWhatWasTypedAndLeavesTheToolRunning)
{
    PlanFixture plan;
    ASSERT_TRUE(plan.view.startTool("draw.line").ok());
    plan.press(200, 150);
    plan.type("@3");
    plan.escape();
    EXPECT_TRUE(plan.view.typedInput().isEmpty());
    EXPECT_TRUE(plan.view.toolActive());
}

TEST(PlanViewTools, TheRubberBandIsDrawnWhileHoveringAfterTheFirstPoint)
{
    PlanFixture plan;
    ASSERT_TRUE(plan.view.startTool("draw.line").ok());
    plan.move(300, 150);
    paint(plan.view);
    // Before the first point Line previews nothing: no segment, no marker.
    EXPECT_EQ(plan.view.lastPreviewCount(), 0u);

    plan.press(200, 150); // (0, 0)
    plan.move(300, 150);  // (10, 0)
    paint(plan.view);
    // The band from (0, 0) to the cursor, and a marker on (0, 0): 1 + 1.
    EXPECT_EQ(plan.view.lastPreviewCount(), 2u);
    const auto feedback = plan.view.toolHost().feedback(Point2(10, 0));
    ASSERT_EQ(feedback.shapes.size(), 1u);
    const auto* band = std::get_if<Segment2>(&feedback.shapes.front());
    ASSERT_NE(band, nullptr);
    EXPECT_EQ(band->start, Point2(0, 0));
    EXPECT_EQ(band->end, Point2(10, 0));
}

TEST(PlanViewTools, NoPreviewIsDrawnBeforeThePointerHasMoved)
{
    PlanFixture plan;
    ASSERT_TRUE(plan.view.startTool("draw.line").ok());
    plan.type("5,5");
    plan.enter();
    paint(plan.view);
    // The pointer has never been over the view: the cursor is the model's
    // origin, where nobody pointed, and a band to it would lead nowhere.
    EXPECT_EQ(plan.view.lastPreviewCount(), 0u);
    plan.move(300, 150); // (10, 0)
    paint(plan.view);
    // The band from (5, 5) to the cursor and the marker on (5, 5): 1 + 1.
    EXPECT_EQ(plan.view.lastPreviewCount(), 2u);
}

TEST(PlanViewTools, ARightClickIsEnterWhileAToolRuns)
{
    PlanFixture plan;
    bool menuOpened = false;
    plan.view.onContextMenu = [&](const QPoint&) { menuOpened = true; };
    ASSERT_TRUE(plan.view.startTool("draw.line").ok());
    plan.press(200, 150);                  // (0, 0)
    plan.press(200, 50);                   // (0, 10)
    plan.press(200, 50, Qt::RightButton);  // Enter

    const auto made = plan.entities();
    ASSERT_EQ(made.size(), 1u);
    expectSegment(made.front(), Point2(0, 0), Point2(0, 10));
    EXPECT_FALSE(menuOpened) << "the shortcut menu is for selecting, not for a tool";
}

TEST(PlanViewTools, CtrlZInsideAToolStepsBackAPointNotTheDrawing)
{
    PlanFixture plan;
    addLine(plan.document, Point2(-50, -50), Point2(-40, -50));
    ASSERT_TRUE(plan.view.startTool("draw.line").ok());
    plan.press(200, 150); // (0, 0)
    plan.press(300, 150); // (10, 0)
    plan.press(300, 50);  // (10, 10)
    plan.key(Qt::Key_Z, Qt::ControlModifier);
    plan.enter();

    // The existing line is still there, and the chain lost (10, 10):
    // (0, 0)-(10, 0) and the line from before.
    const auto made = plan.entities();
    ASSERT_EQ(made.size(), 2u);
    expectSegment(made.back(), Point2(0, 0), Point2(10, 0));
    EXPECT_EQ(plan.undoSteps(), 2u);
}

TEST(PlanViewTools, CtrlZWithNothingToStepBackLeavesTheDrawingAloneAndSaysSo)
{
    PlanFixture plan;
    addLine(plan.document, Point2(-50, -50), Point2(-40, -50));
    ASSERT_TRUE(plan.view.startTool("draw.line").ok());
    plan.key(Qt::Key_Z, Qt::ControlModifier);

    EXPECT_EQ(plan.entities().size(), 1u);
    EXPECT_EQ(plan.undoSteps(), 1u);
    EXPECT_EQ(plan.errors.size(), 1u) << "the tool's refusal, not silence";
    EXPECT_TRUE(plan.view.toolActive());
}

TEST(PlanViewTools, AClickSnapsToTheEndOfALineNearIt)
{
    PlanFixture plan;
    addLine(plan.document, Point2(0, 0), Point2(10, 0));
    plan.view.setSnapEnabled(true);
    plan.view.setSnapModes(static_cast<katana::cad::SnapModes>(katana::cad::SnapMode::Endpoint));
    ASSERT_TRUE(plan.view.startTool("draw.line").ok());
    // Pixel (302, 152) is model (10.2, -0.2), 0.28 from the end (10, 0) and
    // inside the 12-pixel aperture (1.2 units at this scale).
    plan.press(302, 152);
    plan.view.setSnapEnabled(false);
    plan.press(300, 50); // (10, 10)
    plan.enter();

    const auto made = plan.entities();
    ASSERT_EQ(made.size(), 2u);
    expectSegment(made.back(), Point2(10, 0), Point2(10, 10));
}

TEST(PlanViewTools, MoveActsOnTheSelectionItStartedWith)
{
    PlanFixture plan;
    const EntityId id = addLine(plan.document, Point2(0, 0), Point2(10, 0));
    plan.document.selection().add(id);
    ASSERT_TRUE(plan.view.startTool("modify.move").ok());
    plan.press(200, 150); // base (0, 0)
    plan.press(250, 100); // to (5, 5): the displacement is (5, 5)

    const auto* moved = plan.document.model().entities.find(id);
    expectSegment(moved, Point2(5, 5), Point2(15, 5));
    EXPECT_EQ(plan.undoSteps(), 2u) << "the line, then the move";
    EXPECT_FALSE(plan.view.toolActive()) << "Move ends; its next use needs a new selection";
}

TEST(PlanViewTools, AToolsSelectionStepGathersClicksUntilEnter)
{
    // Erase always asks. Two plain clicks pick both lines, as AutoCAD's
    // "Select objects" gathers; the Select tool would keep only the last.
    PlanFixture plan;
    addLine(plan.document, Point2(0, 0), Point2(10, 0));   // through pixel (250, 150)
    addLine(plan.document, Point2(0, 10), Point2(10, 10)); // through pixel (250, 50)
    ASSERT_TRUE(plan.view.startTool("modify.erase").ok());
    plan.press(250, 150);
    plan.press(250, 50);
    EXPECT_EQ(plan.document.selection().ids().size(), 2u);
    plan.enter();

    EXPECT_TRUE(plan.entities().empty());
    EXPECT_EQ(plan.undoSteps(), 3u) << "two lines, then one erase";
}

TEST(PlanViewTools, AShiftWindowAtAToolsSelectionStepTakesWhatItEnclosesBackOut)
{
    // AutoCAD's "Select objects": Shift with a window removes. The line
    // (0, 0)-(10, 0) lies inside the window from pixel (180, 130) to (320,
    // 170), model (-2, 2) to (12, -2), dragged left to right.
    PlanFixture plan;
    const EntityId id = addLine(plan.document, Point2(0, 0), Point2(10, 0));
    ASSERT_TRUE(plan.view.startTool("modify.erase").ok());
    plan.press(250, 150); // (5, 0), on the line
    ASSERT_TRUE(plan.document.selection().contains(id));
    plan.drag(QPointF(180, 130), QPointF(320, 170), Qt::ShiftModifier);

    EXPECT_FALSE(plan.document.selection().contains(id));
    plan.enter(); // nothing to erase: refused, and the line stays
    EXPECT_NE(plan.document.model().entities.find(id), nullptr);
    EXPECT_EQ(plan.undoSteps(), 1u);
}

TEST(PlanViewTools, AShiftOrCtrlClickAtAToolsSelectionStepTakesAPickBackOut)
{
    // Line a through pixel (250, 150), line b through (250, 50). Both are
    // picked; Shift-clicking a takes it out, and Ctrl-clicking a again does
    // not put it back (Ctrl no longer toggles here, it removes), so Enter
    // erases b alone.
    PlanFixture plan;
    const EntityId a = addLine(plan.document, Point2(0, 0), Point2(10, 0));
    const EntityId b = addLine(plan.document, Point2(0, 10), Point2(10, 10));
    ASSERT_TRUE(plan.view.startTool("modify.erase").ok());
    plan.press(250, 150);
    plan.press(250, 50);
    plan.press(250, 150, Qt::LeftButton, Qt::ShiftModifier);
    EXPECT_FALSE(plan.document.selection().contains(a));
    EXPECT_TRUE(plan.document.selection().contains(b));
    plan.press(250, 150, Qt::LeftButton, Qt::ControlModifier);
    EXPECT_FALSE(plan.document.selection().contains(a));
    plan.enter();

    EXPECT_NE(plan.document.model().entities.find(a), nullptr);
    EXPECT_EQ(plan.document.model().entities.find(b), nullptr);
}

TEST(PlanViewTools, AClickAtAToolsSelectionStepSendsOutThePromptThatCountsIt)
{
    // Trim's edge prompt counts the edges from the document's selection. The
    // command line and status bar hear the prompt only through onPrompt, so
    // a click there must send it again. Two lines picked, none by the tool
    // itself: "Select cutting edges" + " <use 2 edges>" (EdgeTool::prompt).
    PlanFixture plan;
    addLine(plan.document, Point2(0, 0), Point2(10, 0));   // through pixel (250, 150)
    addLine(plan.document, Point2(0, 10), Point2(10, 10)); // through pixel (250, 50)
    std::vector<QString> prompts;
    plan.view.onPrompt = [&](const QString& text) { prompts.push_back(text); };
    ASSERT_TRUE(plan.view.startTool("modify.trim").ok());
    plan.press(250, 150);
    plan.press(250, 50);

    ASSERT_FALSE(prompts.empty());
    EXPECT_EQ(prompts.back().toStdString(), "Trim: Select cutting edges <use 2 edges>");
}

TEST(PlanViewTools, CtrlZAtAToolsSelectionStepTakesBackTheLastClick)
{
    // The clicks changed the document's selection, which the tool reads at
    // Enter; the tool never saw them, so its own undo would say nothing was
    // picked while both edges stayed picked.
    PlanFixture plan;
    const EntityId a = addLine(plan.document, Point2(0, 0), Point2(10, 0));
    const EntityId b = addLine(plan.document, Point2(0, 10), Point2(10, 10));
    ASSERT_TRUE(plan.view.startTool("modify.trim").ok());
    plan.press(250, 150);
    plan.press(250, 50);

    plan.key(Qt::Key_Z, Qt::ControlModifier);
    EXPECT_TRUE(plan.document.selection().contains(a));
    EXPECT_FALSE(plan.document.selection().contains(b));
    plan.key(Qt::Key_Z, Qt::ControlModifier);
    EXPECT_TRUE(plan.document.selection().empty());
    EXPECT_TRUE(plan.errors.empty()) << plan.errors.front().toStdString();

    plan.key(Qt::Key_Z, Qt::ControlModifier); // nothing left: the tool says so
    EXPECT_EQ(plan.errors.size(), 1u);
    EXPECT_TRUE(plan.view.toolActive());
    EXPECT_EQ(plan.undoSteps(), 2u) << "the drawing is never undone under a tool";
}

TEST(PlanViewTools, UTypedAtAToolsSelectionStepTakesBackTheLastClickAsCtrlZDoes)
{
    PlanFixture plan;
    const EntityId a = addLine(plan.document, Point2(0, 0), Point2(10, 0));
    const EntityId b = addLine(plan.document, Point2(0, 10), Point2(10, 10));
    ASSERT_TRUE(plan.view.startTool("modify.erase").ok());
    plan.press(250, 150);
    plan.press(250, 50);
    plan.type("u");
    plan.enter();
    plan.enter(); // erases what is left picked: a

    EXPECT_EQ(plan.document.model().entities.find(a), nullptr);
    EXPECT_NE(plan.document.model().entities.find(b), nullptr);
}

TEST(PlanViewTools, CtrlZAfterATypedAllTakesBackTheAllBeforeTheClickBeforeIt)
{
    // Steps back in the order they were made: the click on a, then All
    // (which the tool holds itself), so Ctrl+Z takes back All and leaves a
    // picked, and Enter erases a alone.
    PlanFixture plan;
    const EntityId a = addLine(plan.document, Point2(0, 0), Point2(10, 0));
    const EntityId b = addLine(plan.document, Point2(0, 10), Point2(10, 10));
    ASSERT_TRUE(plan.view.startTool("modify.erase").ok());
    plan.press(250, 150);
    plan.type("ALL");
    plan.enter();
    plan.key(Qt::Key_Z, Qt::ControlModifier);
    plan.enter();

    EXPECT_EQ(plan.document.model().entities.find(a), nullptr);
    EXPECT_NE(plan.document.model().entities.find(b), nullptr);
}

TEST(PlanViewTools, TextTypedWithNoToolRunningStillGoesToTheWindow)
{
    PlanFixture plan;
    plan.type("L");
    ASSERT_EQ(plan.typedOut.size(), 1u);
    EXPECT_EQ(plan.typedOut.front(), "L");
}

TEST(PlanViewTools, TheLegacyToolsStartTheirCatalogueTools)
{
    PlanFixture plan;
    for (const Tool tool : {Tool::Point, Tool::Line, Tool::Polyline, Tool::Rectangle, Tool::Circle,
                            Tool::Arc, Tool::Move, Tool::Copy}) {
        ASSERT_NE(katana::cad::toolCatalog().find(katana::qt::toolId(tool)), nullptr)
            << katana::qt::toString(tool);
        plan.view.setTool(tool);
        EXPECT_EQ(plan.view.activeToolId(), katana::qt::toolId(tool));
        EXPECT_EQ(plan.view.tool(), tool);
    }
    plan.view.setTool(Tool::Select);
    EXPECT_FALSE(plan.view.toolActive());
    EXPECT_EQ(plan.view.tool(), Tool::Select);
}

TEST(PlanViewTools, AnUnknownToolIdIsRefusedAndTheRunningToolRunsOn)
{
    PlanFixture plan;
    ASSERT_TRUE(plan.view.startTool("draw.line").ok());
    const auto refused = plan.view.startTool("draw.nothing");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, katana::core::ErrorCode::NotFound);
    EXPECT_EQ(plan.view.activeToolId(), "draw.line");
}

TEST(PlanViewTools, TheLayerNewWorkIsDrawnOnIsTheDocumentsCurrentLayer)
{
    PlanFixture plan;
    ASSERT_TRUE(plan.document
                    .execute(katana::commands::createLayer(
                        katana::entity::Layer{.name = "Kerbs"}))
                    .ok());
    ASSERT_TRUE(plan.document.setCurrentLayer("Kerbs").ok());
    ASSERT_TRUE(plan.view.startTool("draw.line").ok());
    plan.press(200, 150);
    plan.press(300, 150);
    plan.enter();

    const auto made = plan.entities();
    ASSERT_EQ(made.size(), 1u);
    EXPECT_EQ(made.front()->layer, "Kerbs");
}

TEST(PlanViewTools, AToolRunningAcrossANewDrawingEndsAndItsNextStartDrawsOnTheNewDrawing)
{
    // MainWindow::newDocument calls resetInteraction BEFORE it replaces the
    // document. A tool rebuilt at that moment read the old drawing's current
    // layer, Kerbs, and its first chain in the new drawing was refused (no
    // such layer); Move kept ids from the discarded drawing. The tool ends
    // instead, so the order of the two calls no longer matters.
    PlanFixture plan;
    ASSERT_TRUE(plan.document
                    .execute(katana::commands::createLayer(
                        katana::entity::Layer{.name = "Kerbs"}))
                    .ok());
    ASSERT_TRUE(plan.document.setCurrentLayer("Kerbs").ok());
    std::vector<std::string> changes;
    plan.view.onActiveToolChanged = [&](const std::string& id) { changes.push_back(id); };
    ASSERT_TRUE(plan.view.startTool("draw.line").ok());
    plan.press(200, 150);

    plan.view.resetInteraction(); // in MainWindow::newDocument's order
    plan.document.newDocument();
    EXPECT_FALSE(plan.view.toolActive());
    ASSERT_FALSE(changes.empty());
    EXPECT_EQ(changes.back(), "") << "the toolbar hears that the tool ended";

    ASSERT_TRUE(plan.view.startTool("draw.line").ok());
    plan.press(200, 150); // (0, 0)
    plan.press(300, 150); // (10, 0)
    plan.enter();
    const auto made = plan.entities();
    ASSERT_EQ(made.size(), 1u);
    EXPECT_EQ(made.front()->layer, std::string(katana::entity::kDefaultLayerName));
    expectSegment(made.front(), Point2(0, 0), Point2(10, 0));
    EXPECT_TRUE(plan.errors.empty()) << plan.errors.front().toStdString();
}

TEST(PlanViewTools, EnterAtNoPromptRepeatsTheLastTool)
{
    PlanFixture plan;
    ASSERT_TRUE(plan.view.startTool("draw.line").ok());
    plan.press(200, 150);
    plan.press(300, 150);
    plan.enter(); // the line; Line starts again
    plan.enter(); // Enter at its first prompt ends it
    ASSERT_FALSE(plan.view.toolActive());

    plan.enter();
    EXPECT_EQ(plan.view.activeToolId(), "draw.line");
    EXPECT_EQ(plan.entities().size(), 1u) << "repeating a tool draws nothing by itself";
}

TEST(PlanViewTools, EscAfterATrimKeepsTheCutAsOneUndoStep)
{
    // The Modify Edit report's case: Trim holds its cuts in a session that
    // only Enter commits, so an Esc that dropped the tool would lose them.
    // The horizontal line (0, 0)-(20, 0) is crossed by the vertical one at
    // x = 10; picking it at (15, 0) - pixel (350, 150) - takes away the part
    // beyond the crossing, leaving (0, 0)-(10, 0).
    PlanFixture plan;
    addLine(plan.document, Point2(0, 0), Point2(20, 0));
    addLine(plan.document, Point2(10, -5), Point2(10, 5));
    ASSERT_TRUE(plan.view.startTool("modify.trim").ok());
    plan.enter();         // every object is a cutting edge
    plan.press(350, 150); // the part to take away
    ASSERT_EQ(plan.undoSteps(), 2u) << "the cut is the tool's until Enter or Esc";
    plan.escape();

    EXPECT_FALSE(plan.view.toolActive());
    EXPECT_EQ(plan.undoSteps(), 3u);
    const katana::entity::Entity* kept = nullptr;
    for (const auto* entity : plan.entities()) {
        const auto* segment = std::get_if<Segment2>(&entity->geometry);
        if (segment != nullptr && segment->start.y == 0.0 && segment->end.y == 0.0) {
            kept = entity;
        }
    }
    expectSegment(kept, Point2(0, 0), Point2(10, 0));
}

TEST(PlanViewTools, EscAtTheRadiusPromptKeepsTheCornersAMultipleFilletMade)
{
    // Enter at Fillet's radius prompt only returns to the lines, so an Esc
    // that sent Enter there and then dropped the tool lost the corners the
    // Multiple session had made. Line a (0, 0)-(12, 0) and line b (10, -2)-
    // (10, 10) meet at (10, 0). Picking a at (5, 0) - pixel (250, 150) - and b
    // at (10, 5) - pixel (300, 100) - keeps the picked sides, and radius 0
    // makes a sharp corner: a becomes (0, 0)-(10, 0), b (10, 0)-(10, 10).
    PlanFixture plan;
    const EntityId a = addLine(plan.document, Point2(0, 0), Point2(12, 0));
    const EntityId b = addLine(plan.document, Point2(10, -2), Point2(10, 10));
    ASSERT_TRUE(plan.view.startTool("modify.fillet").ok());
    plan.type("R"); // the radius is remembered for the program: set it here
    plan.enter();
    plan.type("0");
    plan.enter();
    plan.type("M");
    plan.enter();
    plan.press(250, 150);
    plan.press(300, 100);
    ASSERT_EQ(plan.undoSteps(), 2u) << "the corner is the tool's until Enter or Esc";
    plan.type("R");
    plan.enter();
    ASSERT_NE(plan.view.toolHost().prompt().find("radius <"), std::string::npos)
        << plan.view.toolHost().prompt();
    plan.escape();

    EXPECT_FALSE(plan.view.toolActive());
    EXPECT_EQ(plan.undoSteps(), 3u) << "the two lines, then one FILLET";
    expectSegment(plan.document.model().entities.find(a), Point2(0, 0), Point2(10, 0));
    expectSegment(plan.document.model().entities.find(b), Point2(10, 0), Point2(10, 10));
}

TEST(PlanViewTools, EscAtTheSecondChamferDistanceLeavesTheRememberedDistancesAlone)
{
    // Enter at the second distance takes the first as the second and stores
    // both for every later Chamfer; Esc must never apply a default. The
    // distances are remembered for the program, so the test compares the
    // prompt before with the prompt after rather than assuming what it says.
    PlanFixture plan;
    ASSERT_TRUE(plan.view.startTool("modify.chamfer").ok());
    const std::string before = plan.view.toolHost().prompt();
    plan.type("D");
    plan.enter();
    plan.type("3.25");
    plan.enter();
    ASSERT_NE(plan.view.toolHost().prompt().find("second chamfer distance <3.25>"),
              std::string::npos)
        << plan.view.toolHost().prompt();
    plan.escape();
    EXPECT_FALSE(plan.view.toolActive());

    ASSERT_TRUE(plan.view.startTool("modify.chamfer").ok());
    EXPECT_EQ(plan.view.toolHost().prompt(), before);
    EXPECT_EQ(plan.undoSteps(), 0u);
}

TEST(PlanViewTools, EscFromAMoveAtItsSecondPointMovesNothing)
{
    // Move's Enter at the second point moves by the base point as a
    // displacement (AutoCAD's default); Esc must never apply it.
    PlanFixture plan;
    const EntityId id = addLine(plan.document, Point2(0, 0), Point2(10, 0));
    plan.document.selection().add(id);
    ASSERT_TRUE(plan.view.startTool("modify.move").ok());
    plan.press(250, 100); // base (5, 5)
    plan.escape();

    expectSegment(plan.document.model().entities.find(id), Point2(0, 0), Point2(10, 0));
    EXPECT_EQ(plan.undoSteps(), 1u);
}

TEST(PlanViewTools, SelectIsReportedToTheWindowEvenWithNothingRunning)
{
    // The window checks its Select button from onToolChanged at start-up,
    // when no tool has run yet.
    PlanFixture plan;
    std::vector<Tool> reported;
    plan.view.onToolChanged = [&](Tool tool) { reported.push_back(tool); };
    plan.view.setTool(Tool::Select);
    ASSERT_EQ(reported.size(), 1u);
    EXPECT_EQ(reported.front(), Tool::Select);

    plan.view.setTool(Tool::Line);
    plan.view.setTool(Tool::Select);
    ASSERT_EQ(reported.size(), 3u);
    EXPECT_EQ(reported[1], Tool::Line);
    EXPECT_EQ(reported[2], Tool::Select);
}

// ---- the everyday tools in a real view -----------------------------------------------

TEST(PlanViewTools, EscAfterTwoCopiesKeepsBothAsOneUndoStep)
{
    // Copy's placed copies are work the user clicked for; Esc keeps them
    // now, where the host's old list dropped them.
    PlanFixture plan;
    ASSERT_TRUE(plan.document.execute(katana::commands::createLine(Point2(0, 0), Point2(1, 0)))
                    .ok());
    plan.document.selection().add(plan.document.lastCreatedEntities().front());
    ASSERT_TRUE(plan.view.startTool("modify.copy").ok());
    plan.press(200, 150); // base (0, 0)
    plan.press(250, 150); // (5, 0)
    plan.press(300, 150); // (10, 0)
    plan.escape();
    EXPECT_FALSE(plan.view.toolActive());
    EXPECT_EQ(plan.entities().size(), 3u);
    EXPECT_EQ(plan.undoSteps(), 2u) << "the line, then one step for both copies";
}

TEST(PlanViewTools, DistanceBetweenTwoClicksReachesTheLogAsAnInverse)
{
    PlanFixture plan;
    std::vector<QString> messages;
    plan.view.onToolMessage = [&](const QString& text) { messages.push_back(text); };
    ASSERT_TRUE(plan.view.startTool("inquiry.distance").ok());
    plan.press(200, 150); // (0, 0)
    plan.press(230, 110); // (3, 4)
    ASSERT_EQ(messages.size(), 1u);
    // sqrt(3^2 + 4^2) = 5, and nothing drawn.
    EXPECT_TRUE(messages.front().contains("Horizontal distance 5.000")) << messages.front().toStdString();
    EXPECT_TRUE(plan.entities().empty());
    EXPECT_TRUE(plan.view.toolActive()) << "ready for the next measurement";
}

TEST(PlanViewTools, MatchPropertiesTakesTheSourceByAClickAndTheTargetsBySelection)
{
    PlanFixture plan;
    katana::entity::Layer road;
    road.name = "ROAD";
    ASSERT_TRUE(plan.document.execute(katana::commands::createLayer(road)).ok());
    katana::entity::Entity source;
    source.geometry = Segment2{Point2(0, 0), Point2(10, 0)};
    source.layer = "ROAD";
    ASSERT_TRUE(plan.document.execute(katana::commands::createEntities({source})).ok());
    ASSERT_TRUE(plan.document.execute(katana::commands::createLine(Point2(0, 5), Point2(10, 5)))
                    .ok());
    const EntityId target = plan.document.lastCreatedEntities().front();
    ASSERT_TRUE(plan.view.startTool("modify.match_properties").ok());
    plan.press(250, 150); // on the source, (5, 0)
    plan.press(250, 100); // on the target, (5, 5)
    plan.enter();
    EXPECT_FALSE(plan.view.toolActive());
    EXPECT_EQ(plan.document.model().entities.find(target)->layer, "ROAD");
    EXPECT_TRUE(plan.errors.empty()) << plan.errors.front().toStdString();
}


TEST(PlanViewTools, ADimensionSnappedToALinesEndsKeepsTheEndsItWasSnappedTo)
{
    // The view hands the tool the entity point its snap found, not only the
    // point: pixel (202, 152) is model (0.2, -0.2), 0.28 from the line's
    // start and inside the aperture, and (298, 148) is (9.8, 0.2), as near
    // its end. The dimension line goes through (250, 100), model (5, 5).
    PlanFixture plan;
    const EntityId line = addLine(plan.document, Point2(0, 0), Point2(10, 0));
    plan.view.setSnapEnabled(true);
    plan.view.setSnapModes(static_cast<katana::cad::SnapModes>(katana::cad::SnapMode::Endpoint));
    ASSERT_TRUE(plan.view.startTool("annotate.dimaligned").ok());
    plan.press(202, 152);
    plan.press(298, 148);
    plan.view.setSnapEnabled(false);
    plan.press(250, 100);

    const auto made = plan.entities();
    ASSERT_EQ(made.size(), 2u);
    const auto* dimension = std::get_if<katana::entity::DimensionGeometry>(&made.back()->geometry);
    ASSERT_NE(dimension, nullptr);
    EXPECT_EQ(dimension->start, Point2(0, 0));
    EXPECT_EQ(dimension->end, Point2(10, 0));
    EXPECT_EQ(dimension->startRef,
              (katana::entity::AnchorRef{line, katana::entity::AnchorPoint::Start, 0}));
    EXPECT_EQ(dimension->endRef,
              (katana::entity::AnchorRef{line, katana::entity::AnchorPoint::End, 0}));
}
