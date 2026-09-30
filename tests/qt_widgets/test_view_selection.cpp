// The selection in every view (docs/desktop.md, "The selection in every
// view"): the owner's request of 2026-09-30, "when selecting a feature in 2d
// it gets highlighted in other views". A feature selected in the design view
// shows, faint, in the as-built view that hides its layer, in plan and in 3D,
// and each view's own switch (VIEWS SET <id> ghosts=on|off, the Layers
// popup's box) turns that off.
//
// The workspace is built as MainWindow builds it, with a CommandInterpreter
// as the command runner and the workspace as its view host, so the popup's
// box runs its line through the same interpreter as a typed one.

#include <gtest/gtest.h>

#include <string>

#include <QCheckBox>
#include <QMainWindow>
#include <QMouseEvent>

#include "command_runner.hpp"
#include "dock_chrome.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/section.hpp"
#include "katana/cad/selection_style.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"
#include "view_layers_popup.hpp"
#include "view_workspace.hpp"
#include "widget_harness.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::cad::ViewId;
using katana::cad::ViewKind;
using katana::cad::ViewState;
using katana::geometry::Point2;
using katana::qt::DockChrome;
using katana::qt::VerbOutcome;
using katana::qt::ViewWorkspace;
using katana::qt::test::paint;
using katana::qt::test::processEvents;

namespace {

// A design line from (10, 10) to (40, 30) and an as-built line from (10, 30)
// to (40, 10), the design one selected; view 1 hides the as-built layer and
// view 2 the design one - the owner's two views.
struct DesignAndAsBuilt {
    Document document;
    QMainWindow window;
    ViewWorkspace* views = nullptr;
    CommandInterpreter interpreter{document};
    QStringList ran;
    katana::entity::EntityId design = katana::entity::kInvalidEntityId;
    katana::entity::EntityId asBuilt = katana::entity::kInvalidEntityId;

    DesignAndAsBuilt()
    {
        views = new ViewWorkspace(document, &window);
        window.setCentralWidget(views);
        views->setChrome(new DockChrome(window));
        interpreter.setViewHost([this] { return &views->verbHost(); });
        views->setCommandRunner([this](const QString& line) {
            ran << line;
            const auto reply = interpreter.run(line.toStdString());
            VerbOutcome outcome;
            outcome.ok = reply.ok();
            if (reply.ok()) {
                outcome.reply = QString::fromStdString(*reply);
            } else {
                outcome.error = QString::fromStdString(reply.error().describe());
            }
            return outcome;
        });
        design = line("design", Point2(10, 10), Point2(40, 30));
        asBuilt = line("asbuilt", Point2(10, 30), Point2(40, 10));
        window.resize(900, 600);
        window.show();
        processEvents();
        (void)views->openView(ViewKind::Plan);
        processEvents();
        run("VIEWS HIDE 1 asbuilt");
        run("VIEWS HIDE 2 design");
        document.selection().add(design);
        document.notifySelectionChanged();
        processEvents();
    }

    katana::entity::EntityId line(const char* layerName, Point2 from, Point2 to)
    {
        katana::entity::Layer layer;
        layer.name = layerName;
        EXPECT_TRUE(document.execute(katana::commands::createLayer(layer)).ok());
        katana::commands::EntityAttributes attributes;
        attributes.layer = layerName;
        EXPECT_TRUE(document.execute(katana::commands::createLine(from, to, attributes)).ok());
        return document.model().entities.ids().back();
    }

    std::string run(const std::string& text)
    {
        const auto reply = interpreter.run(text);
        EXPECT_TRUE(reply.ok()) << text << ": " << (reply.ok() ? "" : reply.error().describe());
        processEvents();
        return reply.ok() ? *reply : std::string();
    }

    [[nodiscard]] ViewState& state(ViewId id) const { return *views->viewSet().find(id); }
};

} // namespace

TEST(ViewSelection, AFeatureSelectedInTheDesignViewIsAGhostInTheAsBuiltPlanView)
{
    DesignAndAsBuilt w;
    auto& design = *w.views->planView(1);
    auto& asBuilt = *w.views->planView(2);
    paint(design);
    paint(asBuilt);
    // Each view draws its own line; the design line, selected, is drawn in
    // view 1 and ghosted in view 2.
    EXPECT_EQ(design.lastDrawnEntityCount(), 1u);
    EXPECT_EQ(design.lastGhostCount(), 0u);
    EXPECT_EQ(asBuilt.lastDrawnEntityCount(), 1u);
    EXPECT_EQ(asBuilt.lastGhostCount(), 1u);

    // The as-built view's switch off: nothing of the design line there.
    EXPECT_NE(w.run("VIEWS SET 2 ghosts=off").find(" ghosts=off"), std::string::npos);
    paint(asBuilt);
    EXPECT_EQ(asBuilt.lastGhostCount(), 0u);
    EXPECT_EQ(asBuilt.lastDrawnEntityCount(), 1u);

    // Deselected, no ghost even with the switch on again.
    w.run("VIEWS SET 2 ghosts=on");
    w.document.selection().clear();
    w.document.notifySelectionChanged();
    paint(asBuilt);
    EXPECT_EQ(asBuilt.lastGhostCount(), 0u);
}

TEST(ViewSelection, AFeatureSelectedInPlanIsAFaintLineInTheThreeDViewThatHidesItsLayer)
{
    DesignAndAsBuilt w;
    const ViewId model = w.views->openView(ViewKind::Model3D).id;
    processEvents();
    w.run("VIEWS HIDE " + std::to_string(model) + " design");
    auto& view = *w.views->renderView(model);
    paint(view);
    const katana::cad::SceneLayers& layers = view.sceneLayers();
    ASSERT_EQ(layers.selection.lines.size(), 1u);
    EXPECT_EQ(layers.selection.colors[layers.selection.lines[0].a], katana::cad::kGhostColor3d);
    EXPECT_TRUE(layers.selectionCasing.lines.empty()) << "a ghost has no casing";

    w.run("VIEWS SET " + std::to_string(model) + " ghosts=off");
    paint(view);
    EXPECT_TRUE(view.sceneLayers().selection.lines.empty());

    // Shown in that view again, the line is the selection's core and casing.
    w.run("VIEWS SHOW " + std::to_string(model) + " ALL");
    paint(view);
    ASSERT_EQ(view.sceneLayers().selection.lines.size(), 1u);
    EXPECT_EQ(view.sceneLayers().selection.colors[view.sceneLayers().selection.lines[0].a],
              katana::cad::kSelectionColor);
    EXPECT_EQ(view.sceneLayers().selectionCasing.lines.size(), 1u);
}

TEST(ViewSelection, TheLayersPopupsBoxRunsViewsSetThroughTheRunner)
{
    DesignAndAsBuilt w;
    katana::qt::ViewLayersPopup* popup = w.views->showLayersPopup(2);
    ASSERT_NE(popup, nullptr);
    QCheckBox* box = popup->findChild<QCheckBox*>("ViewLayersShowSelection");
    ASSERT_NE(box, nullptr);
    ASSERT_EQ(box, popup->ghostsBox());
    EXPECT_TRUE(box->isChecked()) << "a view ghosts the selection when it opens";

    box->click();
    processEvents();
    ASSERT_FALSE(w.ran.isEmpty());
    EXPECT_EQ(w.ran.back().toStdString(), "VIEWS SET 2 ghosts=off");
    EXPECT_FALSE(w.state(2).selectionGhosts);
    EXPECT_TRUE(w.state(1).selectionGhosts) << "only that view";
    EXPECT_FALSE(box->isChecked());
    paint(*w.views->planView(2));
    EXPECT_EQ(w.views->planView(2)->lastGhostCount(), 0u);

    box->click();
    processEvents();
    EXPECT_EQ(w.ran.back().toStdString(), "VIEWS SET 2 ghosts=on");
    EXPECT_TRUE(w.state(2).selectionGhosts);
    EXPECT_TRUE(box->isChecked());
    popup->close();
}

TEST(ViewSelection, TheLayersPopupsBoxFollowsAViewsSetTypedWhileItIsClosed)
{
    DesignAndAsBuilt w;
    w.run("VIEWS SET 2 ghosts=off");
    katana::qt::ViewLayersPopup* popup = w.views->showLayersPopup(2);
    ASSERT_NE(popup, nullptr);
    EXPECT_FALSE(popup->ghostsBox()->isChecked());
    popup->close();
}

TEST(ViewSelection, ASectionMarksTheCrossingOfTheFeatureSelectedInPlan)
{
    // A cut along y = 20 from x = 0 to 50 crosses the design line - (10, 10)
    // to (40, 30), at y = 20 half way, x = 25 - at station 25, and the
    // as-built line at station 25 too; each crossing names its entity.
    DesignAndAsBuilt w;
    katana::geometry::Polyline2 cut;
    cut.vertices = {Point2(0, 20), Point2(50, 20)};
    auto section = katana::cad::extractSection(cut, {}, &w.document.model());
    ASSERT_TRUE(section.ok());
    ASSERT_EQ(section->crossings.size(), 2u);
    ASSERT_TRUE(w.views->showSection(std::move(*section)));
    processEvents();
    const ViewId id = w.views->viewSet().views().back()->id;
    auto* view = w.views->sectionView(id);
    ASSERT_NE(view, nullptr);
    paint(*view);
    EXPECT_EQ(view->lastSelectedCrossingCount(), 1u) << "the design line's, selected in plan";
    EXPECT_EQ(view->lastDrawnCrossingCount(), 2u);

    // The section hiding the design layer: its crossing is a ghost.
    w.run("VIEWS HIDE " + std::to_string(id) + " design");
    paint(*view);
    EXPECT_EQ(view->lastSelectedCrossingCount(), 0u);
    EXPECT_EQ(view->lastGhostCrossingCount(), 1u);
    EXPECT_EQ(view->lastDrawnCrossingCount(), 1u);

    // A selection change reaches the section without a cut.
    w.document.selection().clear();
    w.document.notifySelectionChanged();
    paint(*view);
    EXPECT_EQ(view->lastGhostCrossingCount(), 0u);
}

TEST(ViewSelection, ASelectedFeatureTheDrawingHidesIsMarkedInNoView)
{
    // One rule for every view (cad::isGhost beside cad::isDrawn): an entity
    // the drawing hides - made invisible, or its layer switched off - stays
    // hidden in every view, selected or not. The section asked a rule of its
    // own, which read the layer the cut found and not the entity, and went on
    // marking such a crossing as selected, or as a ghost.
    DesignAndAsBuilt w;
    katana::geometry::Polyline2 cut;
    cut.vertices = {Point2(0, 20), Point2(50, 20)};
    auto cutSection = katana::cad::extractSection(cut, {}, &w.document.model());
    ASSERT_TRUE(cutSection.ok());
    ASSERT_TRUE(w.views->showSection(std::move(*cutSection)));
    processEvents();
    const ViewId id = w.views->viewSet().views().back()->id;
    auto* section = w.views->sectionView(id);
    ASSERT_NE(section, nullptr);
    auto& asBuilt = *w.views->planView(2);
    paint(*section);
    paint(asBuilt);
    ASSERT_EQ(section->lastSelectedCrossingCount(), 1u);
    ASSERT_EQ(asBuilt.lastGhostCount(), 1u);

    // The design line made invisible, and selected still.
    ASSERT_TRUE(
        w.document.execute(katana::commands::setEntityVisible({w.design}, false)).ok());
    ASSERT_TRUE(w.document.selection().contains(w.design));
    paint(*section);
    paint(asBuilt);
    EXPECT_EQ(section->lastSelectedCrossingCount(), 0u);
    EXPECT_EQ(section->lastGhostCrossingCount(), 0u);
    EXPECT_EQ(asBuilt.lastGhostCount(), 0u);
    // Nor a ghost in a section that hides its layer.
    w.run("VIEWS HIDE " + std::to_string(id) + " design");
    paint(*section);
    EXPECT_EQ(section->lastGhostCrossingCount(), 0u);
    w.run("VIEWS SHOW " + std::to_string(id) + " ALL");

    // Visible again; then its layer switched off in the drawing.
    ASSERT_TRUE(w.document.undo().ok());
    paint(*section);
    ASSERT_EQ(section->lastSelectedCrossingCount(), 1u);
    w.run("LAYER HIDE design");
    ASSERT_TRUE(w.document.selection().contains(w.design));
    paint(*section);
    paint(asBuilt);
    EXPECT_EQ(section->lastSelectedCrossingCount(), 0u);
    EXPECT_EQ(section->lastGhostCrossingCount(), 0u);
    EXPECT_EQ(asBuilt.lastGhostCount(), 0u);
}

namespace {

// A mouse event as Qt delivers one to `widget`: `button` pressed, the mouse
// moved with it held, or `button` released.
void mouse(QWidget& widget, QEvent::Type type, QPointF at, Qt::MouseButton button)
{
    const Qt::MouseButtons held =
        type == QEvent::MouseButtonRelease ? Qt::NoButton : Qt::MouseButtons(button);
    QMouseEvent event(type, at, widget.mapToGlobal(at),
                      type == QEvent::MouseMove ? Qt::NoButton : button, held, Qt::NoModifier);
    QCoreApplication::sendEvent(&widget, &event);
}

// Presses on `from`, moves in four steps to `to` and lets go.
void drag(QWidget& widget, QPointF from, QPointF to)
{
    mouse(widget, QEvent::MouseButtonPress, from, Qt::LeftButton);
    for (int step = 1; step <= 4; ++step) {
        mouse(widget, QEvent::MouseMove, from + (to - from) * (step / 4.0), Qt::LeftButton);
    }
    mouse(widget, QEvent::MouseButtonRelease, to, Qt::LeftButton);
}

} // namespace

TEST(ViewSelection, AGhostOffersNoGripSoADragWhereItCrossesTheAsBuiltLineEditsNothing)
{
    // Where the two lines cross, (25, 20), both have their middles; the
    // design line, selected, offers its grips in the design view. The
    // as-built view hides its layer and showed its grips all the same: a
    // drag there from the crossing moved the design line that view does not
    // show (40 px right took it to (15.93, 10)-(45.93, 30)). It offers none
    // of them now - a ghost is never picked - and follows its own layers,
    // which change with no document notification.
    DesignAndAsBuilt w;
    w.run("ZOOM WINDOW 0,0,50,40 view=2");
    auto& design = *w.views->planView(1);
    auto& asBuilt = *w.views->planView(2);
    paint(design);
    paint(asBuilt);
    EXPECT_EQ(design.gripCount(), 3u) << "the design line's two ends and middle";
    EXPECT_EQ(asBuilt.gripCount(), 0u);
    EXPECT_EQ(asBuilt.lastGhostCount(), 1u) << "drawn as a ghost there, all the same";

    w.run("VIEWS SHOW 2 design");
    paint(asBuilt);
    EXPECT_EQ(asBuilt.gripCount(), 3u) << "shown there again, it is edited there again";
    w.run("VIEWS HIDE 2 design");
    paint(asBuilt);
    EXPECT_EQ(asBuilt.gripCount(), 0u);

    const std::size_t undoable = w.document.history().undoCount();
    const auto before =
        std::get<katana::geometry::Segment2>(w.document.model().entities.find(w.design)->geometry);
    const Point2 crossing = w.state(2).plan.worldToScreen(Point2(25, 20));
    drag(asBuilt, QPointF(crossing.x, crossing.y), QPointF(crossing.x + 40, crossing.y));
    processEvents();

    const auto after =
        std::get<katana::geometry::Segment2>(w.document.model().entities.find(w.design)->geometry);
    EXPECT_EQ(after.start, before.start);
    EXPECT_EQ(after.end, before.end);
    EXPECT_EQ(w.document.history().undoCount(), undoable) << "no edit was made";
}

TEST(ViewSelection, AGhostSwitchChangedWithoutANotificationIsStillDrawn)
{
    // As a selection changed and only repainted (the kept drawing's key holds
    // a fingerprint of it), a view's switch set directly and repainted is
    // drawn: the key holds it too.
    DesignAndAsBuilt w;
    auto& asBuilt = *w.views->planView(2);
    paint(asBuilt);
    ASSERT_EQ(asBuilt.lastGhostCount(), 1u);
    w.state(2).selectionGhosts = false;
    asBuilt.update();
    paint(asBuilt);
    EXPECT_EQ(asBuilt.lastGhostCount(), 0u);
}
