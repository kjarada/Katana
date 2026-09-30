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

#include <algorithm>
#include <cmath>
#include <string>

#include <QLabel>
#include <QMainWindow>
#include <QToolButton>
#include <QMouseEvent>
#include <QWheelEvent>

#include "command_runner.hpp"
#include "dock_chrome.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"
#include "theme.hpp"
#include "view_workspace.hpp"
#include "widget_harness.hpp"
#include "zoom_to_dialog.hpp"

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

    // A middle-button drag in view `id` from `from` by `by` pixels, in four
    // moves, as a person pans.
    void pan(ViewId id, QPointF from, QPointF by)
    {
        ViewportWidget& view = plan(id);
        const auto send = [&view](QEvent::Type type, QPointF at, Qt::MouseButtons held) {
            QMouseEvent event(type, at, view.mapToGlobal(at),
                              type == QEvent::MouseMove ? Qt::NoButton : Qt::MiddleButton, held,
                              Qt::NoModifier);
            QCoreApplication::sendEvent(&view, &event);
        };
        send(QEvent::MouseButtonPress, from, Qt::MiddleButton);
        for (int step = 1; step <= 4; ++step) {
            send(QEvent::MouseMove, from + by * (step / 4.0), Qt::MiddleButton);
        }
        send(QEvent::MouseButtonRelease, from + by, Qt::NoButton);
        processEvents();
    }

    // Counts the moves view `id` reports to the workspace, passing each on.
    void countMoves(ViewId id, int& reports)
    {
        ViewportWidget& view = plan(id);
        view.onViewMoved = [previous = view.onViewMoved, &reports](bool byUser) {
            ++reports;
            if (previous) {
                previous(byUser);
            }
        };
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

TEST(ViewLinks, APanInALinkedViewMovesTheOtherBitForBitAndTheOtherNeverReportsIt)
{
    LinkedWorkspace w;
    const ViewId a = w.views->viewSet().activeId();
    const ViewId b = w.views->openView(ViewKind::Plan).id;
    processEvents();
    w.place(a, 300, 200, 10, 20, 4);
    w.place(b, 380, 260, -50, -50, 1);
    (void)w.run("VIEWS LINK " + std::to_string(a) + "," + std::to_string(b));
    int reportsA = 0;
    int reportsB = 0;
    w.countMoves(a, reportsA);
    w.countMoves(b, reportsB);

    // A dragged (+60, -30) px from its middle: the point that was under the
    // press, (10, 20), is under the release, so by hand the centre is
    // (10 - 60/4, 20 - 30/4) = (-5, 12.5) at the same 4 px a unit.
    w.pan(a, QPointF(150, 100), QPointF(60, -30));

    EXPECT_EQ(w.state(a).plan.center, Point2(-5, 12.5));
    EXPECT_EQ(w.state(a).plan.scale, 4.0);
    expectSameView(w.state(b), w.state(a));
    EXPECT_GT(reportsA, 0) << "each move of the drag is the user's";
    EXPECT_EQ(reportsB, 0) << "moved by the link, B reports nothing: no echo";
}

TEST(ViewLinks, AViewLinkedBeforeItWasEverSeenLeadsFromItsFirstPaint)
{
    // Linked while never painted and named the one the others come to, a
    // plan view moves nobody - it has nothing framed to give them - until
    // its first paint frames the drawing; that frame is then the link's.
    LinkedWorkspace w;
    ASSERT_TRUE(
        w.document.execute(katana::commands::createLine(Point2(1000, 1000), Point2(1100, 1050)))
            .ok());
    const ViewId a = w.views->viewSet().activeId();
    w.place(a, 300, 200, 10, 20, 4);
    const ViewId b = w.views->openView(ViewKind::Plan).id;
    ASSERT_FALSE(w.state(b).planFramed);
    // Straight to the interpreter: the event loop would paint B at once.
    const auto linked = w.interpreter.run("VIEWS LINK " + std::to_string(a) + "," +
                                          std::to_string(b) + " TO " + std::to_string(b));
    ASSERT_TRUE(linked.ok());
    EXPECT_EQ(*linked, "leader=2 linked=1,2 moved=none");
    ASSERT_EQ(w.state(a).plan.center, Point2(10, 20)) << "nothing framed to follow yet";

    paint(w.plan(b));

    // B framed the line about its middle, and A shows exactly that.
    ASSERT_TRUE(w.state(b).planFramed);
    EXPECT_EQ(w.state(b).plan.center, Point2(1050, 1025));
    expectSameView(w.state(a), w.state(b));
}

TEST(ViewLinks, ALinkLedByAViewThatFramesNothingAtItsFirstPaintTakesTheOthersPlace)
{
    // As above, but B hides the drawing's one layer: its first paint frames
    // nothing and leaves it where every new view starts, the origin at 10 px
    // a unit. That frame led nobody, and the link showed two places - A on
    // the line, B at the origin - with both views saying linked=yes. B has no
    // place of its own worth leading to; it takes A's, the place the user
    // put A: the line's middle (1050, 1025) at 2 px a unit.
    LinkedWorkspace w;
    ASSERT_TRUE(w.document.execute(katana::commands::createLayer(layerNamed("design"))).ok());
    katana::commands::EntityAttributes design = w.document.currentAttributes();
    design.layer = "design";
    ASSERT_TRUE(w.document
                    .execute(katana::commands::createLine(Point2(1000, 1000),
                                                          Point2(1100, 1050), design))
                    .ok());
    const ViewId a = w.views->viewSet().activeId();
    w.place(a, 300, 200, 1050, 1025, 2);
    const ViewId b = w.views->openView(ViewKind::Plan).id;
    // Straight to the interpreter, as a script runs its lines: the event
    // loop would paint B at once.
    const std::string ids = std::to_string(a) + "," + std::to_string(b);
    ASSERT_TRUE(w.interpreter.run("VIEWS HIDE " + std::to_string(b) + " design").ok());
    const auto linked = w.interpreter.run("VIEWS LINK " + ids + " TO " + std::to_string(b));
    ASSERT_TRUE(linked.ok());
    EXPECT_EQ(*linked, "leader=2 linked=1,2 moved=none");

    paint(w.plan(b));
    processEvents();

    ASSERT_TRUE(w.plan(b).drawnBounds().empty()) << "B draws nothing";
    EXPECT_EQ(w.state(a).plan.center, Point2(1050, 1025)) << "A stays where it was put";
    EXPECT_EQ(w.state(a).plan.scale, 2.0);
    expectSameView(w.state(b), w.state(a));
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

    // Z, as a person types it at the second point: the view's, not a point -
    // the view the tool runs in, which the line names.
    const std::string inView = " view=" + std::to_string(plan);
    ASSERT_TRUE(w.views->typeIntoTool("Z"));
    processEvents();
    ASSERT_FALSE(w.ran.isEmpty());
    EXPECT_EQ(w.ran.back().toStdString(), "ZOOM" + inView);
    EXPECT_EQ(w.state(plan).plan.center, Point2(150, 125));
    // The tool is where it was: still Line, still asking for the next point.
    EXPECT_EQ(w.views->activeToolId(), "draw.line");
    EXPECT_EQ(w.plan(plan).toolExpects(), katana::cad::ToolInput::Point);
    EXPECT_EQ(w.plan(plan).toolHost().prompt(), prompt);

    // 'ZOOM IN with AutoCAD's apostrophe is the same; PAN has no verb and is
    // refused by name, the tool untouched.
    ASSERT_TRUE(w.views->typeIntoTool("'ZOOM IN"));
    EXPECT_EQ(w.ran.back().toStdString(), "ZOOM IN" + inView);
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

TEST(ViewLinks, AZoomTypedAtAToolsPromptZoomsTheViewTheToolRunsInNotTheActiveOne)
{
    // Line runs in view 1 when view 2 is made the active one - clicked to be
    // looked at. The Z typed at Line's prompt zoomed view 2, and view 1, where
    // Line picks its next point, stayed where it was. It is view 1's now,
    // framed on the line's middle (150, 125); a view= typed is kept.
    LinkedWorkspace w;
    ASSERT_TRUE(
        w.document.execute(katana::commands::createLine(Point2(100, 100), Point2(200, 150)))
            .ok());
    const ViewId drawing = w.views->viewSet().activeId();
    const ViewId other = w.views->openView(ViewKind::Plan).id;
    processEvents();
    w.place(drawing, 300, 200, 0, 0, 10);
    w.place(other, 300, 200, -40, -40, 3);
    (void)w.run("VIEWS ACTIVATE " + std::to_string(drawing));
    ASSERT_TRUE(w.views->startTool("draw.line").ok());
    ASSERT_NE(w.plan(drawing).activeToolId(), "") << "Line runs in view 1";
    ASSERT_TRUE(w.views->typeIntoTool("0,0"));
    (void)w.run("VIEWS ACTIVATE " + std::to_string(other));
    ASSERT_EQ(w.views->viewSet().activeId(), other);
    ASSERT_EQ(w.views->activeToolId(), "draw.line");

    ASSERT_TRUE(w.views->typeIntoTool("'Z"));
    processEvents();
    EXPECT_EQ(w.ran.back().toStdString(), "ZOOM view=" + std::to_string(drawing));
    EXPECT_EQ(w.state(drawing).plan.center, Point2(150, 125));
    EXPECT_EQ(w.state(other).plan.center, Point2(-40, -40)) << "the view looked at stays put";
    EXPECT_EQ(w.plan(drawing).toolExpects(), katana::cad::ToolInput::Point);

    ASSERT_TRUE(w.views->typeIntoTool("ZOOM IN view=" + QString::number(other)));
    EXPECT_EQ(w.ran.back().toStdString(), "ZOOM IN view=" + std::to_string(other));
    EXPECT_EQ(w.state(other).plan.scale, 6.0) << "twice as close, where it was asked";
    w.views->stopTool();
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

namespace {

// A survey line at MGA coordinates, far from the origin every new view starts
// at: (300000, 6200000) to (300100, 6200050), its middle (300050, 6200025).
void surveyLine(Document& document, const char* layerName = nullptr)
{
    katana::commands::EntityAttributes attributes = document.currentAttributes();
    if (layerName != nullptr) {
        katana::entity::Layer layer;
        layer.name = layerName;
        ASSERT_TRUE(document.execute(katana::commands::createLayer(layer)).ok());
        attributes.layer = layerName;
    }
    ASSERT_TRUE(document
                    .execute(katana::commands::createLine(Point2(300000, 6200000),
                                                          Point2(300100, 6200050), attributes))
                    .ok());
}

// The scale Zoom Extents frames a w x h box at in `view`: 8 % a side
// (ViewportWidget's kFrameMargin, ViewTransform::fit), the tighter axis.
double framedScale(const ViewState& view, double w, double h)
{
    return std::min(0.84 * view.plan.widthPixels / w, 0.84 * view.plan.heightPixels / h);
}

} // namespace

TEST(ViewLinks, AZoomOfAViewNotYetPaintedStartsFromWhatItDraws)
{
    // A script runs VIEWS OPEN plan and ZOOM IN back to back, with no paint
    // between. The view zoomed about the place a new view holds until its
    // first paint - the origin at 10 px a unit - and, marked framed then,
    // never framed the drawing: the survey line was nowhere in it. It frames
    // what it draws first now, as its first paint would, so the zoom is about
    // the line's middle at twice the scale that frames it.
    LinkedWorkspace w;
    surveyLine(w.document);
    const auto opened = w.interpreter.run("VIEWS OPEN plan");
    ASSERT_TRUE(opened.ok());
    EXPECT_NE(opened->find(" framed=no "), std::string::npos) << *opened;
    const ViewId b = w.views->viewSet().activeId();
    ASSERT_FALSE(w.state(b).planFramed);

    // Straight on, as a script runs its lines.
    const auto zoomed = w.interpreter.run("ZOOM IN view=" + std::to_string(b));
    ASSERT_TRUE(zoomed.ok()) << zoomed.error().describe();
    const ViewState& view = w.state(b);
    EXPECT_EQ(view.plan.center, Point2(300050, 6200025));
    const double fitted = framedScale(view, 100, 50);
    EXPECT_NEAR(view.plan.scale, 2.0 * fitted, 1e-12 * fitted);
    EXPECT_TRUE(zoomed->starts_with("view=2 kind=plan centre=300050,6200025 scale=")) << *zoomed;

    // Its first paint leaves it there: the zoom is the user's view now.
    paint(w.plan(b));
    EXPECT_EQ(w.state(b).plan.center, Point2(300050, 6200025));
    EXPECT_NEAR(w.state(b).plan.scale, 2.0 * fitted, 1e-12 * fitted);
    EXPECT_EQ(w.plan(b).lastDrawnEntityCount(), 1u);
    EXPECT_EQ(w.run("VIEWS").find("framed="), std::string::npos) << "framed, it says nothing";
}

TEST(ViewLinks, AZoomStraightAfterAViewOpensIsFramedAtTheWidthTheViewIsSeenAt)
{
    // A script's VIEWS OPEN plan then ZOOM IN, with no event loop between.
    // The new view's bar fits its tools to the width the split gave it, and
    // a tool it drops queues one more layout, which moves the split by a
    // pixel or two. The zoom framed the view - and its reply said - at the
    // width before that layout: in the window, 330 px where the view is seen
    // at 328. The workspace does the layout Qt has queued before it zooms.
    // Over the window widths at which the tools come and go, each view about
    // 250 to 380 px (as ViewChrome's tests sweep them).
    int differed = 0;
    for (int windowWidth = 500; windowWidth <= 760; windowWidth += 5) {
        LinkedWorkspace w;
        w.window.resize(windowWidth, 600);
        processEvents();
        surveyLine(w.document);
        ASSERT_TRUE(w.interpreter.run("VIEWS OPEN plan").ok());
        const ViewId b = w.views->viewSet().activeId();
        const double opened = w.plan(b).width();
        ASSERT_TRUE(w.interpreter.run("ZOOM IN view=" + std::to_string(b)).ok());
        const double framed = w.state(b).plan.widthPixels;
        processEvents();
        const double seen = w.plan(b).width();
        EXPECT_EQ(framed, seen) << "window " << windowWidth << " px";
        differed += opened != seen ? 1 : 0;
    }
    // The sweep reaches the case it is about: a width the layout still had
    // to change when the view had just opened.
    EXPECT_GT(differed, 0);
}

TEST(ViewLinks, AViewNotYetPaintedThatLeadsTheLinkTakesItToTheDrawingWhenZoomed)
{
    // The same script with a link made in it, the new view leading: the link
    // went to the origin with it. The view linked with it follows the frame
    // and then the zoom, bit for bit.
    LinkedWorkspace w;
    surveyLine(w.document);
    const ViewId a = w.views->viewSet().activeId();
    w.place(a, 300, 200, 10, 20, 4);
    ASSERT_TRUE(w.interpreter.run("VIEWS OPEN plan").ok());
    const ViewId b = w.views->viewSet().activeId();
    const auto linked = w.interpreter.run("VIEWS LINK " + std::to_string(a) + "," +
                                          std::to_string(b) + " TO " + std::to_string(b));
    ASSERT_TRUE(linked.ok());
    EXPECT_EQ(*linked, "leader=2 linked=1,2 moved=none");

    const auto zoomed = w.interpreter.run("ZOOM IN view=" + std::to_string(b));
    ASSERT_TRUE(zoomed.ok()) << zoomed.error().describe();
    EXPECT_EQ(w.state(b).plan.center, Point2(300050, 6200025));
    expectSameView(w.state(a), w.state(b));
    EXPECT_NE(zoomed->find("\nview=1 kind=plan followed=2 centre=300050,6200025 "),
              std::string::npos)
        << *zoomed;
}

TEST(ViewLinks, AThreeDViewsZoomInIsTwiceAsCloseAboutItsMiddle)
{
    // ZOOM IN on a 3D view is its wheel turned over the middle of it: the
    // notches that make 2 at the wheel's 1.15 a notch, over the pixel the
    // view's axis passes through - so the eye is half as far from the
    // target, and the target, the middle of the view, stays where it was.
    // ZOOM OUT is the way back.
    LinkedWorkspace w;
    ASSERT_TRUE(
        w.document.execute(katana::commands::createLine(Point2(0, 0), Point2(100, 50))).ok());
    const ViewId model = w.views->openView(ViewKind::Model3D).id;
    processEvents();
    paint(*w.views->renderView(model));
    ASSERT_TRUE(w.state(model).cameraFramed);
    const katana::render::Camera before = w.state(model).camera;

    const std::string reply = w.run("ZOOM IN view=" + std::to_string(model));
    EXPECT_TRUE(reply.starts_with("view=" + std::to_string(model) + " kind=3d target=")) << reply;
    const katana::render::Camera& after = w.state(model).camera;
    EXPECT_NEAR(after.distance(), 0.5 * before.distance(), 1e-12 * before.distance());
    const double near = 1e-9 * before.distance();
    EXPECT_NEAR(after.target().x, before.target().x, near);
    EXPECT_NEAR(after.target().y, before.target().y, near);
    EXPECT_NEAR(after.target().z, before.target().z, near);
    EXPECT_EQ(after.azimuth(), before.azimuth()) << "a zoom turns nothing";

    (void)w.run("ZOOM OUT view=" + std::to_string(model));
    EXPECT_NEAR(w.state(model).camera.distance(), before.distance(), 1e-12 * before.distance());
}

TEST(ViewLinks, AnEnormousZoomOutOfAThreeDViewStopsWhereTheSceneIsAPixelAcross)
{
    // ZOOM 1e-300 took the eye 4.7e302 units off, where the camera's
    // arithmetic had no digits left for where it looked: the target, the
    // middle of the view, moved from (87.5, 68.69) to (175, 137.38). It goes
    // out no farther than where the scene is a pixel across. The line
    // (0,0)-(100,50) on the datum 0 is the whole scene, whose diagonal,
    // sqrt(100^2 + 50^2), is a pixel across at that times (H / 2) /
    // tan(fov / 2), H the camera's height in pixels and fov its 45 degrees.
    // Out there, a further zoom out does nothing, and a zoom in halves the
    // distance as ever.
    LinkedWorkspace w;
    ASSERT_TRUE(
        w.document.execute(katana::commands::createLine(Point2(0, 0), Point2(100, 50))).ok());
    const ViewId model = w.views->openView(ViewKind::Model3D).id;
    processEvents();
    paint(*w.views->renderView(model));
    const katana::render::Camera before = w.state(model).camera;
    const double farthest = std::hypot(100.0, 50.0) * 0.5 * before.viewportHeight() /
                            std::tan(0.5 * before.fieldOfView());
    ASSERT_LT(before.distance(), farthest);

    const std::string line = " view=" + std::to_string(model);
    (void)w.run("ZOOM 1e-300" + line);
    const katana::render::Camera& after = w.state(model).camera;
    EXPECT_NEAR(after.distance(), farthest, 1e-9 * farthest);
    EXPECT_NEAR(after.target().x, before.target().x, 1e-9 * farthest);
    EXPECT_NEAR(after.target().y, before.target().y, 1e-9 * farthest);
    EXPECT_NEAR(after.target().z, before.target().z, 1e-9 * farthest);

    (void)w.run("ZOOM OUT" + line);
    EXPECT_NEAR(w.state(model).camera.distance(), farthest, 1e-9 * farthest);
    (void)w.run("ZOOM IN" + line);
    EXPECT_NEAR(w.state(model).camera.distance(), 0.5 * farthest, 1e-9 * farthest);
}

TEST(ViewLinks, AThreeDViewNotYetPaintedFramesTheSceneBeforeItZooms)
{
    // As a plan view: VIEWS OPEN 3d and ZOOM IN back to back. The zoom was
    // made on the camera a new 3D view starts with and the first paint then
    // framed the scene over it; the view frames first now, so the zoom is
    // half the distance of the frame, about the line's middle, and its first
    // paint keeps it.
    LinkedWorkspace w;
    ASSERT_TRUE(
        w.document.execute(katana::commands::createLine(Point2(1000, 1000), Point2(1100, 1050)))
            .ok());
    const auto opened = w.interpreter.run("VIEWS OPEN 3d");
    ASSERT_TRUE(opened.ok());
    EXPECT_NE(opened->find(" framed=no "), std::string::npos) << *opened;
    const ViewId model = w.views->viewSet().activeId();
    ASSERT_FALSE(w.state(model).cameraFramed);

    ASSERT_TRUE(w.interpreter.run("ZOOM IN view=" + std::to_string(model)).ok());
    const katana::render::Camera zoomed = w.state(model).camera;
    // The frame's target is the middle of the line's box, on the datum 0.
    EXPECT_NEAR(zoomed.target().x, 1050.0, 1e-9);
    EXPECT_NEAR(zoomed.target().y, 1025.0, 1e-9);
    EXPECT_NEAR(zoomed.target().z, 0.0, 1e-9);

    paint(*w.views->renderView(model));
    EXPECT_EQ(w.state(model).camera.distance(), zoomed.distance()) << "the paint kept the zoom";

    // Framed afresh at the same size, the distance is twice the zoomed one.
    (void)w.run("ZOOM EXTENTS view=" + std::to_string(model));
    EXPECT_NEAR(zoomed.distance(), 0.5 * w.state(model).camera.distance(),
                1e-9 * zoomed.distance());
}

TEST(ViewLinks, ZoomToSelectionInAThreeDViewFramesWhatItDrawsOfTheSelection)
{
    // The 3D view's Zoom to Selection, ZOOM SELECTION view=<id>: the selected
    // line framed where the scene draws it - the camera's target the middle
    // of its box, (510, 320) on the datum 0. Nothing selected, or none of it
    // drawn in the view, moves nothing, and the reply is the scope's record.
    LinkedWorkspace w;
    ASSERT_TRUE(
        w.document.execute(katana::commands::createLine(Point2(0, 0), Point2(10, 0))).ok());
    katana::commands::EntityAttributes far = w.document.currentAttributes();
    ASSERT_TRUE(w.document.execute(katana::commands::createLayer(layerNamed("far"))).ok());
    far.layer = "far";
    ASSERT_TRUE(
        w.document.execute(katana::commands::createLine(Point2(500, 300), Point2(520, 340), far))
            .ok());
    const katana::entity::EntityId farLine = w.document.model().entities.ids().back();
    const ViewId model = w.views->openView(ViewKind::Model3D).id;
    processEvents();
    paint(*w.views->renderView(model));
    const std::string line = "ZOOM SELECTION view=" + std::to_string(model);
    const katana::render::Camera start = w.state(model).camera;

    EXPECT_EQ(w.run(line), "scope=selection matched=0");
    EXPECT_EQ(w.state(model).camera.target(), start.target());
    EXPECT_EQ(w.state(model).camera.distance(), start.distance());

    w.document.selection().set({farLine});
    w.document.notifySelectionChanged();
    const std::string framed = w.run(line);
    EXPECT_TRUE(framed.starts_with("scope=selection matched=1\nview=" + std::to_string(model) +
                                   " kind=3d target=510,320,0 "))
        << framed;
    EXPECT_EQ(w.state(model).camera.target(), katana::geometry::Point3(510, 320, 0));
    EXPECT_LT(w.state(model).camera.distance(), start.distance()) << "closer: 45 units, not 500";

    // Hidden in that view, and not ghosted there: nothing of it is drawn,
    // nothing to frame, and the view's record says so.
    (void)w.run("VIEWS HIDE " + std::to_string(model) + " far");
    (void)w.run("VIEWS SET " + std::to_string(model) + " ghosts=off");
    const katana::render::Camera shown = w.state(model).camera;
    EXPECT_EQ(w.run(line), "scope=selection matched=1\nview=" + std::to_string(model) +
                               " kind=3d shown=0 moved=no");
    EXPECT_EQ(w.state(model).camera.target(), shown.target());
    EXPECT_EQ(w.state(model).camera.distance(), shown.distance());
}

TEST(ViewLinks, APlanViewsZoomToSelectionMovesNothingWhereItShowsNoneOfTheSelection)
{
    // The as-built view hides the design layer with its ghosts off, and the
    // design line is selected: its bar's Zoom to Selection framed the line
    // it does not show - empty ground there - and took the design view
    // along, where its tip says nothing moves. Its ghosts on, it shows the
    // line faintly and frames it: by hand (10,10)-(40,30) in 300 x 200 at
    // 0.84 x 300 / 30 = 8.4 px a unit, about (25, 20).
    LinkedWorkspace w;
    ASSERT_TRUE(w.document.execute(katana::commands::createLayer(layerNamed("design"))).ok());
    katana::commands::EntityAttributes design = w.document.currentAttributes();
    design.layer = "design";
    ASSERT_TRUE(w.document
                    .execute(katana::commands::createLine(Point2(10, 10), Point2(40, 30), design))
                    .ok());
    const ViewId a = w.views->viewSet().activeId();
    const ViewId b = w.views->openView(ViewKind::Plan).id;
    processEvents();
    w.place(a, 300, 200, -500, -500, 2);
    w.place(b, 300, 200, -500, -500, 2);
    (void)w.run("VIEWS LINK " + std::to_string(a) + "," + std::to_string(b));
    (void)w.run("VIEWS HIDE " + std::to_string(b) + " design");
    (void)w.run("VIEWS SET " + std::to_string(b) + " ghosts=off");
    (void)w.run("SELECT ALL");
    ASSERT_EQ(w.document.selection().size(), 1U);

    QToolButton* zoomSelection = w.button(b, "ViewZoomSelectionButton");
    ASSERT_NE(zoomSelection, nullptr);
    zoomSelection->click();
    processEvents();
    EXPECT_EQ(w.ran.back().toStdString(), "ZOOM SELECTION view=2");
    EXPECT_EQ(w.state(b).plan.center, Point2(-500, -500)) << "nothing of it shown there";
    EXPECT_EQ(w.state(b).plan.scale, 2.0);
    EXPECT_EQ(w.state(a).plan.center, Point2(-500, -500)) << "and the link stayed put";

    (void)w.run("VIEWS SET " + std::to_string(b) + " ghosts=on");
    zoomSelection->click();
    processEvents();
    EXPECT_EQ(w.state(b).plan.center, Point2(25, 20));
    EXPECT_NEAR(w.state(b).plan.scale, 8.4, 1e-12);
    expectSameView(w.state(a), w.state(b));
}

TEST(ViewLinks, TheBarsZoomInAndOutRunTheirLinesAndZoomTheView)
{
    // A view bar's Zoom In and Zoom Out: each runs its line through the
    // command runner, ZOOM IN view=<id> and ZOOM OUT view=<id>, which halves
    // and doubles what the view shows - a plan view about its centre, by 2,
    // a 3D view as its wheel zooms, the eye half as far and back.
    LinkedWorkspace w;
    w.window.resize(1200, 600);
    ASSERT_TRUE(
        w.document.execute(katana::commands::createLine(Point2(0, 0), Point2(100, 50))).ok());
    const ViewId plan = w.views->viewSet().activeId();
    const ViewId model = w.views->openView(ViewKind::Model3D).id;
    processEvents();
    w.state(plan).plan.center = Point2(10, 20);
    w.state(plan).plan.scale = 4;
    w.state(plan).planFramed = true;
    w.plan(plan).holdView();
    paint(*w.views->renderView(model));

    for (const ViewId id : {plan, model}) {
        for (const char* name : {"ViewZoomInButton", "ViewZoomOutButton"}) {
            ASSERT_NE(w.button(id, name), nullptr) << name;
            ASSERT_TRUE(w.button(id, name)->isVisible()) << name << " on view " << id;
        }
    }

    w.button(plan, "ViewZoomInButton")->click();
    processEvents();
    EXPECT_EQ(w.ran.back().toStdString(), "ZOOM IN view=1");
    EXPECT_EQ(w.state(plan).plan.scale, 8.0);
    EXPECT_EQ(w.state(plan).plan.center, Point2(10, 20));
    w.button(plan, "ViewZoomOutButton")->click();
    processEvents();
    EXPECT_EQ(w.ran.back().toStdString(), "ZOOM OUT view=1");
    EXPECT_EQ(w.state(plan).plan.scale, 4.0);

    const double distance = w.state(model).camera.distance();
    w.button(model, "ViewZoomInButton")->click();
    processEvents();
    EXPECT_EQ(w.ran.back().toStdString(), "ZOOM IN view=" + std::to_string(model));
    EXPECT_NEAR(w.state(model).camera.distance(), 0.5 * distance, 1e-12 * distance);
    w.button(model, "ViewZoomOutButton")->click();
    processEvents();
    EXPECT_EQ(w.ran.back().toStdString(), "ZOOM OUT view=" + std::to_string(model));
    EXPECT_NEAR(w.state(model).camera.distance(), distance, 1e-12 * distance);
    EXPECT_EQ(w.views->viewSet().activeId(), model) << "pressing a view's tool makes it active";
}

TEST(ViewLinks, ZoomExtentsInALinkedViewThatDrawsNothingFramesWhatTheLinkDraws)
{
    // The design view hides the drawing's one layer. Its Zoom Extents framed
    // nothing - the origin at a pixel a unit, as Zoom Extents on nothing has
    // always gone - and took the as-built view there, 6.2 million units off
    // the line it shows. Linked, it frames what the link draws: the line.
    LinkedWorkspace w;
    surveyLine(w.document, "survey");
    const ViewId a = w.views->viewSet().activeId();
    const ViewId b = w.views->openView(ViewKind::Plan).id;
    processEvents();
    w.place(a, 300, 200, 10, 20, 4);
    w.place(b, 300, 200, 300050, 6200025, 2);
    (void)w.run("VIEWS HIDE " + std::to_string(a) + " survey");
    ASSERT_TRUE(w.plan(a).drawnBounds().empty());
    (void)w.run("VIEWS LINK " + std::to_string(a) + "," + std::to_string(b));

    const std::string reply = w.run("ZOOM EXTENTS view=" + std::to_string(a));
    EXPECT_EQ(w.state(a).plan.center, Point2(300050, 6200025));
    EXPECT_NEAR(w.state(a).plan.scale, framedScale(w.state(a), 100, 50), 1e-12);
    expectSameView(w.state(b), w.state(a));
    EXPECT_NE(reply.find("\nview=2 kind=plan followed=1 centre=300050,6200025 "),
              std::string::npos)
        << reply;

    // Unlinked, a view that draws nothing still frames nothing, as always.
    (void)w.run("VIEWS UNLINK ALL");
    (void)w.run("ZOOM EXTENTS view=" + std::to_string(a));
    EXPECT_EQ(w.state(a).plan.center, Point2(0, 0));
    EXPECT_EQ(w.state(a).plan.scale, 1.0);
    EXPECT_EQ(w.state(b).plan.center, Point2(300050, 6200025)) << "and moves nothing else";
}

TEST(ViewLinks, AMiddleDoubleClickInALinkedViewThatDrawsNothingFramesWhatTheLinkDraws)
{
    // A plan view's middle double-click is its Zoom Extents, and went
    // straight to the widget's own: in the design view, which draws nothing,
    // it took the as-built view to the origin at a pixel a unit, around the
    // rule ZOOM EXTENTS keeps. It is the workspace's Zoom Extents now
    // (ViewportWidget::onZoomExtents), the one every Zoom Extents of a view
    // goes through, so it frames what the link draws: the line, its middle
    // (300050, 6200025), 100 x 50 at the framing margin.
    LinkedWorkspace w;
    surveyLine(w.document, "survey");
    const ViewId a = w.views->viewSet().activeId();
    const ViewId b = w.views->openView(ViewKind::Plan).id;
    processEvents();
    w.place(a, 300, 200, 10, 20, 4);
    w.place(b, 300, 200, 300050, 6200025, 2);
    (void)w.run("VIEWS HIDE " + std::to_string(a) + " survey");
    ASSERT_TRUE(w.plan(a).drawnBounds().empty());
    (void)w.run("VIEWS LINK " + std::to_string(a) + "," + std::to_string(b));

    ViewportWidget& design = w.plan(a);
    const QPointF at(150, 100);
    QMouseEvent click(QEvent::MouseButtonDblClick, at, design.mapToGlobal(at), Qt::MiddleButton,
                      Qt::MiddleButton, Qt::NoModifier);
    QCoreApplication::sendEvent(&design, &click);
    processEvents();
    EXPECT_EQ(w.state(a).plan.center, Point2(300050, 6200025));
    EXPECT_NEAR(w.state(a).plan.scale, framedScale(w.state(a), 100, 50), 1e-12);
    expectSameView(w.state(b), w.state(a));
}

TEST(ViewLinks, AZoomToolsTipSaysLinkedViewsFollowOnlyWhereTheViewCanBeLinked)
{
    // One tip per tool was set for every kind of view, so a section's and a
    // 3D view's promised "linked views follow", which their kinds cannot.
    LinkedWorkspace w;
    const ViewId plan = w.views->viewSet().activeId();
    const ViewId model = w.views->openView(ViewKind::Model3D).id;
    const ViewId section = w.views->openView(ViewKind::Section).id;
    processEvents();
    const auto tip = [&w](ViewId id, const char* name) {
        return w.button(id, name)->toolTip().toStdString();
    };
    for (const char* name : {"ViewZoomInButton", "ViewZoomOutButton", "ViewZoomSelectionButton",
                             "ViewZoomExtentsButton"}) {
        EXPECT_NE(tip(plan, name).find("linked views follow"), std::string::npos) << name;
        EXPECT_EQ(tip(model, name).find("linked views follow"), std::string::npos) << name;
        EXPECT_EQ(tip(section, name).find("linked views follow"), std::string::npos) << name;
    }
    EXPECT_NE(tip(model, "ViewZoomInButton").find("as the wheel zooms there"), std::string::npos);
    EXPECT_NE(tip(section, "ViewZoomInButton").find("the section's plot"), std::string::npos);

    // The tips follow the kind: the 3D view made a plan says so.
    ASSERT_TRUE(w.views->setViewKind(model, ViewKind::Plan).ok());
    processEvents();
    EXPECT_NE(tip(model, "ViewZoomInButton").find("linked views follow"), std::string::npos);
}

TEST(ZoomTo, TheStatusSaysWhatZoomAnsweredAsASentenceAndARefusalInTheErrorColour)
{
    // The raw records, long reals among them, wrapped over five lines and
    // pressed the dialog's type grid until its captions were clipped.
    using katana::qt::ZoomToDialog;
    EXPECT_EQ(ZoomToDialog::spokenReply("scope=selection matched=0").toStdString(),
              "Nothing matched, so no view moved.");
    EXPECT_EQ(ZoomToDialog::spokenReply(
                  "scope=drawing where=\"LAYER=design\" matched=3\n"
                  "view=2 kind=plan centre=33.333333333333336,-3.135162601626014 scale=4 "
                  "area=1,2,3,4")
                  .toStdString(),
              "3 matched, framed in view 2.");
    EXPECT_EQ(ZoomToDialog::spokenReply("scope=selection matched=1\n"
                                        "view=2 kind=plan centre=1,2 scale=3 area=0,0,1,1\n"
                                        "view=1 kind=plan followed=2 centre=1,2 scale=3 "
                                        "area=0,0,1,1")
                  .toStdString(),
              "1 matched, framed in view 2; view 1 followed.");
    EXPECT_EQ(ZoomToDialog::spokenReply("scope=selection matched=5\n"
                                        "view=2 kind=plan centre=1,2 scale=3 area=0,0,1,1\n"
                                        "view=1 kind=plan followed=2 centre=1,2 scale=3\n"
                                        "view=3 kind=plan followed=2 centre=1,2 scale=3")
                  .toStdString(),
              "5 matched, framed in view 2; views 1 and 3 followed.");
    // A view that shows none of what the scope took: it said "framed in
    // view 2" of a plan view showing none of it.
    EXPECT_EQ(ZoomToDialog::spokenReply("scope=selection matched=2\n"
                                        "view=2 kind=plan shown=0 moved=no")
                  .toStdString(),
              "2 matched, but view 2 shows none of them: nothing moved.");
    // One that shows some of them, which it frames.
    EXPECT_EQ(ZoomToDialog::spokenReply("scope=layers layers=a,b sublayers=yes matched=5\n"
                                        "view=3 kind=3d target=1,2,3 distance=4 shown=2")
                  .toStdString(),
              "5 matched, 2 of them shown and framed in view 3.");

    katana::qt::ZoomToContext context;
    context.run = [](const QString&) {
        return VerbOutcome{false, {}, QStringLiteral("refused, as asked")};
    };
    ZoomToDialog dialog(std::move(context));
    EXPECT_FALSE(dialog.zoom());
    const auto* status = dialog.findChild<QLabel*>("zoomToStatus");
    ASSERT_NE(status, nullptr);
    EXPECT_EQ(status->text().toStdString(), "refused, as asked");
    EXPECT_TRUE(status->styleSheet().contains(katana::qt::theme::error().name()))
        << status->styleSheet().toStdString();
}
