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
