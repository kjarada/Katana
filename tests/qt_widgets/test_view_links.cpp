// Linked plan views in the workspace (src/katana_qt/view_workspace.*,
// include/katana/cad/view_link.hpp): the owner's request of 2026-09-30 to
// zoom in a design view and have the as-built view zoom in too, with the
// control on each view's own bar.
//
// The workspace and the chrome are built as MainWindow builds them, with a
// CommandInterpreter as the command runner and the workspace as its view
// host - so the Link button's line runs through the same interpreter as a
// typed one. Every expected position is worked by hand from ViewTransform's
// definitions (a view W by H pixels centred on c at s pixels a unit puts
// screen (x, y) at world (c.x + (x - W/2)/s, c.y - (y - H/2)/s)), never read
// back from a run.

#include <gtest/gtest.h>

#include <cmath>
#include <string>

#include <QMainWindow>
#include <QToolButton>
#include <QWheelEvent>

#include "command_runner.hpp"
#include "dock_chrome.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"
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
using katana::qt::ViewportWidget;
using katana::qt::ViewWorkspace;
using katana::qt::test::paint;
using katana::qt::test::processEvents;

namespace {

// The window's arrangement: the workspace, the chrome it shares with the
// panels, and the one executor the view bars run their lines through.
struct LinkedWorkspace {
    Document document;
    QMainWindow window;
    ViewWorkspace* views = nullptr;
    DockChrome* chrome = nullptr;
    CommandInterpreter interpreter{document};
    // Every line the runner was handed, in order.
    QStringList ran;

    LinkedWorkspace()
    {
        views = new ViewWorkspace(document, &window);
        window.setCentralWidget(views);
        chrome = new DockChrome(window);
        views->setChrome(chrome);
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
        window.resize(800, 600);
        window.show();
        processEvents();
    }

    std::string run(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << ": " << (reply.ok() ? "" : reply.error().describe());
        processEvents();
        return reply.ok() ? *reply : std::string();
    }

    [[nodiscard]] ViewportWidget& plan(ViewId id) const { return *views->planView(id); }
    [[nodiscard]] ViewState& state(ViewId id) const { return *views->viewSet().find(id); }
    [[nodiscard]] QToolButton* button(ViewId id, const char* name) const
    {
        return views->dockFor(id)->findChild<QToolButton*>(name);
    }

    // A plan view pinned to `width` x `height` pixels, showing (x, y) at
    // `scale` as though the user had put it there: framed, and kept.
    void place(ViewId id, int width, int height, double x, double y, double scale)
    {
        plan(id).setFixedSize(width, height);
        processEvents();
        ViewState& view = state(id);
        view.plan.center = Point2(x, y);
        view.plan.scale = scale;
        view.planFramed = true;
        plan(id).holdView();
        processEvents();
    }

    void wheel(ViewId id, QPointF at, int notches = 1)
    {
        ViewportWidget& view = plan(id);
        QWheelEvent event(at, view.mapToGlobal(at), QPoint(), QPoint(0, 120 * notches),
                          Qt::NoButton, Qt::NoModifier, Qt::NoScrollPhase, false);
        QCoreApplication::sendEvent(&view, &event);
        processEvents();
    }
};

katana::entity::Layer layerNamed(const char* name)
{
    katana::entity::Layer layer;
    layer.name = name;
    return layer;
}

void expectSameView(const ViewState& follower, const ViewState& leader)
{
    // Bit for bit: the follower is given the leader's numbers, not a
    // recomputation of them, so there is nothing to drift.
    EXPECT_EQ(follower.plan.center.x, leader.plan.center.x);
    EXPECT_EQ(follower.plan.center.y, leader.plan.center.y);
    EXPECT_EQ(follower.plan.scale, leader.plan.scale);
}

} // namespace

TEST(ViewLinks, AWheelNotchInALinkedPlanViewGivesTheOtherTheSameCentreAndScale)
{
    LinkedWorkspace w;
    const ViewId a = w.views->viewSet().activeId();
    const ViewId b = w.views->openView(ViewKind::Plan).id;
    processEvents();
    w.place(a, 300, 200, 10, 20, 4);
    w.place(b, 360, 240, -50, -50, 1);
    EXPECT_EQ(w.run("VIEWS LINK " + std::to_string(a) + "," + std::to_string(b)),
              "leader=1 linked=1,2 moved=2");
    paint(w.plan(b));
    const std::size_t before = w.plan(b).drawingPaintCount();

    // One notch, x1.2, at A's top-left pixel. By hand the anchor there is
    // (10 - 150/4, 20 + 100/4) = (-27.5, 45); at 4.8 px a unit the centre
    // that keeps it there is (-27.5 + 150/4.8, 45 - 100/4.8) = (3.75,
    // 24.1666...). 150/4.8 rounds to 31.25 exactly, so x is exact; y is
    // 145/6 to the double's precision.
    w.wheel(a, QPointF(0, 0));
    const ViewState& leader = w.state(a);
    EXPECT_EQ(leader.plan.center.x, 3.75);
    EXPECT_NEAR(leader.plan.center.y, 145.0 / 6.0, 1e-12);
    EXPECT_NEAR(leader.plan.scale, 4.8, 1e-12);

    expectSameView(w.state(b), leader);
    // B keeps its own size, so it shows more about the same centre.
    EXPECT_EQ(w.state(b).plan.widthPixels, 360.0);
    EXPECT_EQ(w.state(b).plan.heightPixels, 240.0);
    // And it was drawn again, once, for it.
    paint(w.plan(b));
    EXPECT_EQ(w.plan(b).drawingPaintCount(), before + 1);
}

TEST(ViewLinks, AWheelInAnUnlinkedViewMovesNoOtherView)
{
    LinkedWorkspace w;
    const ViewId a = w.views->viewSet().activeId();
    const ViewId b = w.views->openView(ViewKind::Plan).id;
    processEvents();
    w.place(a, 300, 200, 10, 20, 4);
    w.place(b, 360, 240, -50, -50, 1);
    paint(w.plan(b));
    const std::size_t before = w.plan(b).drawingPaintCount();

    w.wheel(a, QPointF(0, 0));
    EXPECT_EQ(w.state(a).plan.center.x, 3.75);
    EXPECT_EQ(w.state(b).plan.center, Point2(-50, -50));
    EXPECT_EQ(w.state(b).plan.scale, 1.0);
    paint(w.plan(b));
    EXPECT_EQ(w.plan(b).drawingPaintCount(), before);
}

TEST(ViewLinks, ALinkedViewKeepsTheLinksViewWhenResized)
{
    // Both views framed by Zoom Extents, which refits a view on every resize
    // until the user moves it. Linked, neither may: a refit changes the
    // scale of the one resized, which no other view hears of.
    LinkedWorkspace w;
    ASSERT_TRUE(
        w.document.execute(katana::commands::createLine(Point2(0, 0), Point2(100, 50))).ok());
    const ViewId a = w.views->viewSet().activeId();
    const ViewId b = w.views->openView(ViewKind::Plan).id;
    processEvents();
    w.plan(a).setFixedSize(300, 200);
    w.plan(b).setFixedSize(360, 240);
    processEvents();
    ASSERT_TRUE(w.views->zoomExtents(a).ok());
    ASSERT_TRUE(w.views->zoomExtents(b).ok());
    (void)w.run("VIEWS LINK " + std::to_string(a) + "," + std::to_string(b));
    const Point2 centre = w.state(a).plan.center;
    const double scale = w.state(a).plan.scale;
    expectSameView(w.state(b), w.state(a));

    // Each resized: the one that led and the one that followed.
    w.plan(a).setFixedSize(200, 300);
    w.plan(b).setFixedSize(500, 150);
    processEvents();
    EXPECT_EQ(w.state(a).plan.center, centre);
    EXPECT_EQ(w.state(a).plan.scale, scale);
    expectSameView(w.state(b), w.state(a));
}

TEST(ViewLinks, AFollowersFirstPaintDoesNotReframeIt)
{
    LinkedWorkspace w;
    // Something to frame far from where the link looks, so a frame at the
    // first paint would be seen.
    ASSERT_TRUE(
        w.document.execute(katana::commands::createLine(Point2(1000, 1000), Point2(1100, 1050)))
            .ok());
    const ViewId a = w.views->viewSet().activeId();
    w.place(a, 300, 200, 10, 20, 4);
    // Opened and linked before it is ever painted.
    const ViewId b = w.views->openView(ViewKind::Plan).id;
    ASSERT_FALSE(w.state(b).planFramed);
    (void)w.run("VIEWS LINK " + std::to_string(a) + "," + std::to_string(b));
    paint(w.plan(b));

    EXPECT_EQ(w.state(b).plan.center, Point2(10, 20));
    EXPECT_EQ(w.state(b).plan.scale, 4.0);
}

TEST(ViewLinks, PressingTheLinkButtonRunsTheLineAndChecksItFromTheModel)
{
    LinkedWorkspace w;
    const ViewId a = w.views->viewSet().activeId();
    const ViewId b = w.views->openView(ViewKind::Plan).id;
    processEvents();
    w.place(a, 300, 200, 10, 20, 4);
    w.place(b, 300, 200, 70, 80, 2);
    QToolButton* linkA = w.button(a, "ViewLinkButton");
    QToolButton* linkB = w.button(b, "ViewLinkButton");
    ASSERT_NE(linkA, nullptr);
    ASSERT_NE(linkB, nullptr);
    EXPECT_FALSE(linkA->isChecked());
    EXPECT_TRUE(linkA->toolTip().contains("Link this view"));

    // The user zooms the as-built view B, then clicks A's Link first and
    // B's second: B's zoom is what both show.
    w.wheel(b, QPointF(150, 100));
    const Point2 zoomedCentre = w.state(b).plan.center;
    const double zoomedScale = w.state(b).plan.scale;
    ASSERT_NEAR(zoomedScale, 2.4, 1e-12);

    linkA->click();
    processEvents();
    ASSERT_EQ(w.ran.size(), 1);
    EXPECT_EQ(w.ran.back().toStdString(), "VIEWS LINK 1 TO 1");
    EXPECT_TRUE(linkA->isChecked());
    EXPECT_TRUE(linkA->toolTip().contains("Linked, waiting")) << linkA->toolTip().toStdString();
    EXPECT_FALSE(linkB->isChecked());

    linkB->click();
    processEvents();
    EXPECT_EQ(w.ran.back().toStdString(), "VIEWS LINK 2 TO 2") << "B was moved last, so it leads";
    EXPECT_TRUE(linkA->isChecked());
    EXPECT_TRUE(linkB->isChecked());
    EXPECT_EQ(w.state(a).plan.center, zoomedCentre);
    EXPECT_EQ(w.state(a).plan.scale, zoomedScale);
    EXPECT_TRUE(linkA->toolTip().contains("Pans and zooms with Plan 2"))
        << linkA->toolTip().toStdString();
    EXPECT_EQ(linkA->property("linked").toBool(), true);

    // A's Link again: out of the link, and B, alone, with it.
    linkA->click();
    processEvents();
    EXPECT_EQ(w.ran.back().toStdString(), "VIEWS UNLINK 1");
    EXPECT_FALSE(linkA->isChecked());
    EXPECT_FALSE(linkB->isChecked());
    EXPECT_TRUE(w.views->viewSet().linkedViews().empty());
}

TEST(ViewLinks, ARefusedLinkLeavesTheButtonAsItWas)
{
    LinkedWorkspace w;
    const ViewId a = w.views->viewSet().activeId();
    (void)w.views->openView(ViewKind::Plan);
    processEvents();
    // An executor that refuses every line, as the window's does a line it
    // cannot run: the click toggled the button, and nothing else happened.
    w.views->setCommandRunner([&w](const QString& line) {
        w.ran << line;
        return VerbOutcome{false, {}, "refused"};
    });
    QToolButton* link = w.button(a, "ViewLinkButton");
    ASSERT_NE(link, nullptr);
    link->click();
    processEvents();
    ASSERT_EQ(w.ran.size(), 1);
    EXPECT_FALSE(link->isChecked());
    EXPECT_FALSE(w.state(a).linked);
}

TEST(ViewLinks, TheLinkButtonIsOnPlanViewsOnly)
{
    LinkedWorkspace w;
    const ViewId plan = w.views->viewSet().activeId();
    const ViewId model = w.views->openView(ViewKind::Model3D).id;
    const ViewId section = w.views->openView(ViewKind::Section).id;
    processEvents();
    for (const ViewId id : {plan, model, section}) {
        ASSERT_NE(w.button(id, "ViewLinkButton"), nullptr) << id;
    }
    EXPECT_TRUE(w.button(plan, "ViewLinkButton")->isVisible());
    EXPECT_FALSE(w.button(model, "ViewLinkButton")->isVisible());
    EXPECT_FALSE(w.button(section, "ViewLinkButton")->isVisible());

    // It follows the kind: the plan turned into 3D loses it, and the 3D
    // view turned into a plan gains it.
    ASSERT_TRUE(w.views->setViewKind(plan, ViewKind::Model3D).ok());
    ASSERT_TRUE(w.views->setViewKind(model, ViewKind::Plan).ok());
    processEvents();
    EXPECT_FALSE(w.button(plan, "ViewLinkButton")->isVisible());
    EXPECT_TRUE(w.button(model, "ViewLinkButton")->isVisible());
}

TEST(ViewLinks, ThePlanDocksMinimumWidthGrowsByTheLinkButtonAlone)
{
    // Every tool always shown on a bar adds its 22 px and the bar's 1 px
    // between tools (dock_chrome.cpp) to the least a view can be narrowed
    // to. A plan view's bar has the Link button; a 3D view's has not, and is
    // otherwise the same.
    LinkedWorkspace w;
    const ViewId plan = w.views->viewSet().activeId();
    const ViewId model = w.views->openView(ViewKind::Model3D).id;
    processEvents();
    const int planBar = w.chrome->titleBar(w.views->dockFor(plan))->minimumSizeHint().width();
    const int modelBar = w.chrome->titleBar(w.views->dockFor(model))->minimumSizeHint().width();
    EXPECT_EQ(planBar - modelBar, 23);
}

TEST(ViewLinks, ZoomExtentsAllFramesTheLinkOnceOnTheUnionOfWhatItsViewsDraw)
{
    // The design view hides the as-built layer and the as-built view the
    // design one. Framed one by one, each would centre on its own line -
    // (5, 5) and (105, 105) - and the link would come apart.
    LinkedWorkspace w;
    for (const char* name : {"design", "asbuilt"}) {
        ASSERT_TRUE(w.document.execute(katana::commands::createLayer(layerNamed(name))).ok());
    }
    katana::commands::EntityAttributes design;
    design.layer = "design";
    katana::commands::EntityAttributes asBuilt;
    asBuilt.layer = "asbuilt";
    ASSERT_TRUE(
        w.document.execute(katana::commands::createLine(Point2(0, 0), Point2(10, 10), design))
            .ok());
    ASSERT_TRUE(
        w.document
            .execute(katana::commands::createLine(Point2(100, 100), Point2(110, 110), asBuilt))
            .ok());
    const ViewId a = w.views->viewSet().activeId();
    const ViewId b = w.views->openView(ViewKind::Plan).id;
    processEvents();
    w.plan(a).setFixedSize(300, 200);
    w.plan(b).setFixedSize(360, 240);
    processEvents();
    w.state(a).layers.hide("asbuilt");
    w.state(b).layers.hide("design");
    (void)w.run("VIEWS LINK " + std::to_string(a) + "," + std::to_string(b));

    w.views->zoomExtentsAll();
    processEvents();

    // Framed in A, the lowest id, nobody having moved either: the union
    // (0, 0)-(110, 110) about (55, 55), fitted into A's 300 x 200 with 8% a
    // side, 0.84 x 200 / 110 px a unit on the tighter axis.
    const ViewState& leader = w.state(a);
    EXPECT_EQ(leader.plan.center, Point2(55, 55));
    EXPECT_NEAR(leader.plan.scale, 0.84 * 200.0 / 110.0, 1e-12);
    expectSameView(w.state(b), leader);
}

TEST(ViewLinks, AZoomLineOnALinkedViewMovesItAndTheViewsLinkedWithIt)
{
    LinkedWorkspace w;
    const ViewId a = w.views->viewSet().activeId();
    const ViewId b = w.views->openView(ViewKind::Plan).id;
    const ViewId c = w.views->openView(ViewKind::Plan).id;
    processEvents();
    w.place(a, 300, 200, 10, 20, 4);
    w.place(b, 300, 200, 0, 0, 1);
    w.place(c, 300, 200, 7, 7, 7);
    (void)w.run("VIEWS LINK " + std::to_string(a) + "," + std::to_string(b));

    const std::string reply = w.run("ZOOM CENTRE 50,40 SCALE 8 view=" + std::to_string(b));
    // 300 x 200 at 8 px a unit about (50, 40): x 50 -+ 18.75, y 40 -+ 12.5.
    EXPECT_EQ(reply, "view=2 kind=plan centre=50,40 scale=8 area=31.25,27.5,68.75,52.5\n"
                     "view=1 kind=plan followed=2 centre=50,40 scale=8 area=31.25,27.5,68.75,52.5");
    EXPECT_EQ(w.state(a).plan.center, Point2(50, 40));
    EXPECT_EQ(w.state(c).plan.center, Point2(7, 7)) << "not linked";
}

TEST(ViewLinks, ZoomToSelectionInOneLinkedViewFramesItInBoth)
{
    // The as-built view's Zoom to Selection: its line, ZOOM SELECTION view=2,
    // frames the selected line there and the design view follows. By hand the
    // line (10,10)-(40,30) fits a 300 x 200 view at 0.84 x 300 / 30 = 8.4 px a
    // unit, about (25, 20).
    LinkedWorkspace w;
    ASSERT_TRUE(
        w.document.execute(katana::commands::createLine(Point2(10, 10), Point2(40, 30))).ok());
    const ViewId a = w.views->viewSet().activeId();
    const ViewId b = w.views->openView(ViewKind::Plan).id;
    processEvents();
    w.place(a, 360, 240, 0, 0, 1);
    w.place(b, 300, 200, -50, -50, 2);
    (void)w.run("SELECT ALL");
    (void)w.run("VIEWS LINK " + std::to_string(a) + "," + std::to_string(b));

    QToolButton* zoomSelection = w.button(b, "ViewZoomSelectionButton");
    ASSERT_NE(zoomSelection, nullptr);
    zoomSelection->click();
    processEvents();

    EXPECT_EQ(w.ran.back().toStdString(), "ZOOM SELECTION view=2");
    EXPECT_EQ(w.state(b).plan.center, Point2(25, 20));
    EXPECT_NEAR(w.state(b).plan.scale, 8.4, 1e-12);
    expectSameView(w.state(a), w.state(b));
    EXPECT_EQ(w.views->viewSet().activeId(), b) << "pressing a view's tool makes it active";
}

TEST(ViewLinks, ATransparentZoomTypedInsideLineZoomsAndLeavesLineAtItsStep)
{
    // Something far off to frame, so the zoom is seen: its middle (150, 125).
    LinkedWorkspace w;
    ASSERT_TRUE(
        w.document.execute(katana::commands::createLine(Point2(100, 100), Point2(200, 150)))
            .ok());
    const ViewId plan = w.views->viewSet().activeId();
    w.place(plan, 300, 200, 0, 0, 10);
    ASSERT_TRUE(w.views->startTool("draw.line").ok());
    ASSERT_TRUE(w.views->typeIntoTool("0,0"));
    const std::string prompt = w.plan(plan).toolHost().prompt();

    // Z, as a person types it at the second point: the view's, not a point.
    ASSERT_TRUE(w.views->typeIntoTool("Z"));
    processEvents();
    ASSERT_FALSE(w.ran.isEmpty());
    EXPECT_EQ(w.ran.back().toStdString(), "ZOOM");
    EXPECT_EQ(w.state(plan).plan.center, Point2(150, 125));
    // The tool is where it was: still Line, still asking for the next point.
    EXPECT_EQ(w.views->activeToolId(), "draw.line");
    EXPECT_EQ(w.plan(plan).toolExpects(), katana::cad::ToolInput::Point);
    EXPECT_EQ(w.plan(plan).toolHost().prompt(), prompt);

    // 'ZOOM IN with AutoCAD's apostrophe is the same; PAN has no verb and is
    // refused by name, the tool untouched.
    ASSERT_TRUE(w.views->typeIntoTool("'ZOOM IN"));
    EXPECT_EQ(w.ran.back().toStdString(), "ZOOM IN");
    const qsizetype before = w.ran.size();
    ASSERT_TRUE(w.views->typeIntoTool("PAN"));
    EXPECT_EQ(w.ran.size(), before) << "PAN ran nothing";
    EXPECT_TRUE(w.views->runsTransparently("Z"));
    EXPECT_FALSE(w.views->runsTransparently("PAN"));

    // And the line goes on from 0,0 as if nothing had been typed between: the
    // chain is drawn at Enter, as LINE draws it, from 0,0 to 10,0.
    const std::size_t entities = w.document.model().entities.size();
    ASSERT_TRUE(w.views->typeIntoTool("10,0"));
    ASSERT_TRUE(w.views->pressEnter());
    processEvents();
    ASSERT_EQ(w.document.model().entities.size(), entities + 1);
    const katana::entity::Entity* drawn = nullptr;
    w.document.model().entities.forEach([&drawn](const katana::entity::Entity& entity) {
        drawn = &entity; // ascending ids: the last is the newest
    });
    ASSERT_NE(drawn, nullptr);
    const auto* segment = std::get_if<katana::geometry::Segment2>(&drawn->geometry);
    ASSERT_NE(segment, nullptr);
    EXPECT_EQ(segment->start, Point2(0, 0));
    EXPECT_EQ(segment->end, Point2(10, 0));
    w.views->stopTool();
    EXPECT_FALSE(w.views->runsTransparently("Z")) << "no tool runs: Z is a command";
}

TEST(ViewLinks, ASectionsZoomInIsTwiceAsCloseAboutTheMiddleOfItsPlot)
{
    // A section's pan and zoom are its widget's: ZOOM IN view=<section> keeps
    // the station and elevation at the middle of the plot where they were.
    LinkedWorkspace w;
    const ViewId section = w.views->openView(ViewKind::Section).id;
    processEvents();
    katana::qt::SectionViewWidget* view = w.views->sectionView(section);
    ASSERT_NE(view, nullptr);
    const QPointF middle = view->plotCentre();
    const QPointF before = view->stationElevationAt(middle);
    const QPointF edge = view->stationElevationAt(middle + QPointF(100, 0));
    (void)w.run("ZOOM IN view=" + std::to_string(section));
    const QPointF after = view->stationElevationAt(middle);
    EXPECT_NEAR(after.x(), before.x(), 1e-9);
    EXPECT_NEAR(after.y(), before.y(), 1e-9);
    // Twice as close: 100 px from the middle now spans half the stations.
    const QPointF edgeAfter = view->stationElevationAt(middle + QPointF(100, 0));
    EXPECT_NEAR(edgeAfter.x() - after.x(), 0.5 * (edge.x() - before.x()), 1e-9);
}
