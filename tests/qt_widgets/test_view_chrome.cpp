// Which view is the active one once the dock chrome has minimised, restored,
// maximised or closed views of the workspace.
//
// The menus, Zoom Extents and the tools act on the active view, and only its
// title bar carries the accent, so the active view must be one the user can
// see whenever one is on screen. Minimise made an open but hidden view
// possible; and Qt does not hide a tab page behind the current one, it parks
// it off the window - so "open and not hidden" is not "on screen" either.
// Each test here is a way the active view was once left where nobody could
// see it, or a maximise outlived the layout it had hidden.
//
// The workspace and the chrome are built as MainWindow builds them, without
// the rest of the window: the rule belongs to those two.

#include <gtest/gtest.h>

#include <cstdlib>
#include <set>
#include <string>

#include <QMainWindow>
#include <QStyle>
#include <QToolButton>
#include <QTreeWidget>

#include "dock_chrome.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"
#include "view_layers_popup.hpp"
#include "view_workspace.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::cad::LayoutKind;
using katana::cad::ViewId;
using katana::cad::ViewKind;
using katana::qt::DockChrome;
using katana::qt::ViewLayersPopup;
using katana::qt::ViewWorkspace;
using katana::qt::test::processEvents;

namespace {

// A main window whose central widget is the workspace, with one chrome shared
// by both - the workspace made first, as MainWindow makes it, so that Qt
// deletes the views before the chrome that keeps their records.
struct ChromedWorkspace {
    Document document;
    QMainWindow window;
    ViewWorkspace* views = nullptr;
    DockChrome* chrome = nullptr;

    ChromedWorkspace()
    {
        views = new ViewWorkspace(document, &window);
        window.setCentralWidget(views);
        chrome = new DockChrome(window);
        views->setChrome(chrome);
        window.resize(800, 600);
        window.show();
        processEvents();
    }

    [[nodiscard]] ViewId active() const { return views->viewSet().activeId(); }
    // The gap QMainWindow leaves between two docks - the style's, as the
    // workspace's own splits read it.
    [[nodiscard]] int separator() const
    {
        return views->style()->pixelMetric(QStyle::PM_DockWidgetSeparatorExtent, nullptr, views);
    }
    [[nodiscard]] QDockWidget* dock(ViewId id) const { return views->dockFor(id); }

    // Where the user can see it: shown, and floating or within the
    // workspace. A parked tab page is outside the workspace's rectangle.
    [[nodiscard]] bool onScreen(ViewId id) const
    {
        const QDockWidget* view = dock(id);
        return view != nullptr && !view->isHidden() &&
               (view->isFloating() || views->rect().intersects(view->geometry()));
    }

    // What the user does: the view's own Minimise button, the tray button.
    void minimise(ViewId id)
    {
        chrome->titleBar(dock(id))->minimiseButton()->click();
        processEvents();
    }
    void restoreFromTray(ViewId id)
    {
        chrome->trayButton(dock(id))->click();
        processEvents();
    }
    // A click on a tab: raise() is what QTabBar's click does to the page.
    void bringToFront(ViewId id)
    {
        dock(id)->raise();
        processEvents();
    }
};

// A layer of that name, everything else the defaults.
katana::entity::Layer layerNamed(const char* name)
{
    katana::entity::Layer layer;
    layer.name = name;
    return layer;
}

} // namespace

TEST(ViewChrome, RestoringAViewFromTheTrayEndsAMaximiseMadeWhileItWasAway)
{
    ChromedWorkspace w;
    const ViewId plan = w.active();
    const ViewId model = w.views->openView(ViewKind::Model3D).id;
    processEvents();
    // openView split the plan into halves side by side: the two widths, with
    // the separator between them, make up the workspace's width, and differ
    // by at most the pixel an odd remainder leaves over.
    const int planWidth = w.dock(plan)->width();
    const int modelWidth = w.dock(model)->width();
    ASSERT_LE(std::abs(planWidth - modelWidth), 1);

    w.minimise(model);
    w.chrome->maximise(w.dock(plan));
    ASSERT_TRUE(w.chrome->isMaximised(w.dock(plan)));
    w.restoreFromTray(model);

    // The 3D view is back beside the plan, so the plan fills the workspace no
    // longer: its button must say Maximise again, or its Restore would give
    // back the sizes snapped while the 3D view was in the tray.
    EXPECT_FALSE(w.chrome->isMaximised(w.dock(plan)));
    EXPECT_TRUE(w.onScreen(plan));
    EXPECT_TRUE(w.onScreen(model));
    // Exactly where they were before the minimise.
    EXPECT_EQ(w.dock(plan)->width(), planWidth);
    EXPECT_EQ(w.dock(model)->width(), modelWidth);
}

TEST(ViewChrome, MinimisingTheActiveViewHandsOnToTheTabPageOnScreenNotOneParkedBehindIt)
{
    ChromedWorkspace w;
    const ViewId plan = w.active();
    const ViewId model = w.views->openView(ViewKind::Model3D).id;
    const ViewId section = w.views->openView(ViewKind::Section).id;
    // Two places for three views: the plan on the left, the 3D view and the
    // section tabbed on the right (cad::dockSplits(SplitVertical)).
    w.views->arrange(LayoutKind::SplitVertical);
    processEvents();
    w.bringToFront(section);
    ASSERT_TRUE(w.onScreen(section));
    ASSERT_FALSE(w.onScreen(model)) << "the 3D tab is behind the section, parked off the window";
    ASSERT_TRUE(w.views->activateView(plan).ok());

    w.minimise(plan);

    // The 3D view was opened first, but only the section is on screen.
    EXPECT_EQ(w.active(), section);
}

TEST(ViewChrome, MinimisingTheCurrentTabLeavesTheViewMadeActiveOnScreen)
{
    ChromedWorkspace w;
    (void)w.views->openView(ViewKind::Model3D);
    const ViewId section = w.views->openView(ViewKind::Section).id;
    // One place for three views: all three tabbed together.
    w.views->arrange(LayoutKind::Single);
    processEvents();
    w.bringToFront(section);
    ASSERT_TRUE(w.views->activateView(section).ok());
    ASSERT_TRUE(w.onScreen(section));

    w.minimise(section);

    // Qt makes another page current as the section goes, and the plan, the
    // first opened, stays parked behind it unless it is the one chosen:
    // whichever view was made active has to be the page that shows.
    EXPECT_NE(w.active(), section);
    EXPECT_TRUE(w.onScreen(w.active()));
}

TEST(ViewChrome, ClosingTheActiveViewNeverHandsOnToAMinimisedView)
{
    ChromedWorkspace w;
    const ViewId plan = w.active();
    const ViewId model = w.views->openView(ViewKind::Model3D).id;
    const ViewId section = w.views->openView(ViewKind::Section).id;
    processEvents();
    ASSERT_TRUE(w.views->activateView(model).ok());
    w.minimise(model);
    // The first view on screen in the order they were opened took over.
    ASSERT_EQ(w.active(), plan);

    ASSERT_TRUE(w.views->closeView(plan).ok());
    processEvents();

    // The 3D view was used more recently than the section, but it is in the
    // tray; the section is the one view left on screen.
    EXPECT_EQ(w.active(), section);
    EXPECT_TRUE(w.onScreen(section));
}

TEST(ViewChrome, ClosingTheActiveTabHandsOnToTheViewUsedBeforeItAndBringsItToTheFront)
{
    ChromedWorkspace w;
    const ViewId plan = w.active();
    (void)w.views->openView(ViewKind::Model3D);
    const ViewId section = w.views->openView(ViewKind::Section).id;
    // All three tabbed, in the order plan, 3D, section.
    w.views->arrange(LayoutKind::Single);
    processEvents();
    // The plan used, then the section: the plan is the one used before it.
    w.bringToFront(plan);
    ASSERT_TRUE(w.views->activateView(plan).ok());
    w.bringToFront(section);
    ASSERT_TRUE(w.views->activateView(section).ok());

    ASSERT_TRUE(w.views->closeView(section).ok());
    processEvents();

    // ViewSet hands on to the plan. The tab bar would show the section's
    // neighbour, the 3D view, and leave the plan parked behind it.
    EXPECT_EQ(w.active(), plan);
    EXPECT_TRUE(w.onScreen(plan));
}

TEST(ViewChrome, AViewBroughtBackFromTheTrayIsActiveWhenTheActiveViewIsStillInIt)
{
    ChromedWorkspace w;
    const ViewId plan = w.active();
    const ViewId model = w.views->openView(ViewKind::Model3D).id;
    processEvents();
    w.minimise(model);
    ASSERT_EQ(w.active(), plan);
    // No view left to hand on to: the plan stays active, in the tray.
    w.minimise(plan);
    ASSERT_EQ(w.active(), plan);

    w.restoreFromTray(model);

    EXPECT_EQ(w.active(), model);
    EXPECT_TRUE(w.onScreen(model));
}

TEST(ViewChrome, AViewOpenedWhileEveryViewIsMinimisedIsActive)
{
    ChromedWorkspace w;
    const ViewId plan = w.active();
    w.minimise(plan);
    ASSERT_EQ(w.active(), plan);

    // What "show the 3D view" does: it opens one without asking for it to be
    // active, which is right while the active view is one the user can see.
    const ViewId model = w.views->ensureView(ViewKind::Model3D).id;
    processEvents();

    EXPECT_EQ(w.active(), model);
    EXPECT_TRUE(w.onScreen(model));
}

TEST(ViewChrome, PressingMaximiseOnAViewThatIsNotActiveMakesItTheActiveView)
{
    ChromedWorkspace w;
    const ViewId plan = w.active();
    const ViewId model = w.views->openView(ViewKind::Model3D).id;
    processEvents();
    ASSERT_EQ(w.active(), model);

    // A button takes its own press, so the bar's press-to-activate never
    // sees it: the maximise would hide the active 3D view and leave the
    // menus acting on it.
    w.chrome->titleBar(w.dock(plan))->maximiseButton()->click();
    processEvents();

    ASSERT_TRUE(w.chrome->isMaximised(w.dock(plan)));
    EXPECT_FALSE(w.onScreen(model));
    EXPECT_EQ(w.active(), plan);
}

TEST(ViewChrome, PressingAViewsZoomExtentsMakesItTheActiveView)
{
    ChromedWorkspace w;
    const ViewId plan = w.active();
    (void)w.views->openView(ViewKind::Model3D);
    processEvents();

    auto* extents = w.dock(plan)->findChild<QToolButton*>("ViewZoomExtentsButton");
    ASSERT_NE(extents, nullptr);
    extents->click();
    processEvents();

    EXPECT_EQ(w.active(), plan);
}

namespace {

// The buttons a bar shows, by name, with their rectangles in the bar.
std::vector<std::pair<QString, QRect>> shownButtons(const QWidget& bar)
{
    std::vector<std::pair<QString, QRect>> shown;
    for (const QToolButton* button : bar.findChildren<QToolButton*>()) {
        if (button->isVisible()) {
            shown.emplace_back(button->objectName(), button->geometry());
        }
    }
    return shown;
}

} // namespace

TEST(ViewChrome, TheTitleBarShowsOptionalToolsOnlyWhereTheyFitAndNoTwoOverlap)
{
    // A plan view's bar has three optional tools: Zoom to Selection
    // (priority 2), and Zoom In and Out (1, a pair). Each shows only while
    // it fits with DockTitleBar::kTitleRoom left for the title, the highest
    // first: each is 22 px and the row's 1 px spacing. Worked from the bar's
    // least width, which leaves the optional tools out.
    ChromedWorkspace w;
    const ViewId plan = w.active();
    katana::qt::DockTitleBar* bar = w.chrome->titleBar(w.dock(plan));
    ASSERT_NE(bar, nullptr);
    for (const int windowWidth : {300, 600}) {
        w.window.resize(windowWidth, 400);
        processEvents();
        const int width = bar->width();
        const int least = bar->minimumSizeHint().width();
        const int room =
            width - least - (katana::qt::DockTitleBar::kTitleRoom - 24 /* the title's least */);
        const bool selection = room >= 23;
        const bool inAndOut = selection && room - 23 >= 2 * 23;

        const auto* zoomIn = bar->findChild<QToolButton*>("ViewZoomInButton");
        const auto* zoomOut = bar->findChild<QToolButton*>("ViewZoomOutButton");
        const auto* zoomSelection = bar->findChild<QToolButton*>("ViewZoomSelectionButton");
        ASSERT_NE(zoomIn, nullptr);
        ASSERT_NE(zoomOut, nullptr);
        ASSERT_NE(zoomSelection, nullptr);
        EXPECT_EQ(zoomSelection->isVisible(), selection) << "bar " << width << " px";
        EXPECT_EQ(zoomIn->isVisible(), inAndOut) << "bar " << width << " px";
        EXPECT_EQ(zoomOut->isVisible(), inAndOut) << "bar " << width << " px";
        if (windowWidth == 600) {
            EXPECT_TRUE(inAndOut) << "at 600 px every tool fits";
        } else {
            EXPECT_FALSE(inAndOut) << "at 300 px In and Out go first";
        }

        // Every button shown lies in the bar and clear of every other.
        const auto shown = shownButtons(*bar);
        ASSERT_GE(shown.size(), 7U);
        for (std::size_t i = 0; i < shown.size(); ++i) {
            EXPECT_TRUE(bar->rect().contains(shown[i].second))
                << shown[i].first.toStdString() << " leaves the " << width << " px bar";
            for (std::size_t j = i + 1; j < shown.size(); ++j) {
                EXPECT_FALSE(shown[i].second.intersects(shown[j].second))
                    << shown[i].first.toStdString() << " overlaps "
                    << shown[j].first.toStdString() << " at " << width << " px";
            }
        }
    }
}

TEST(ViewChrome, OptionalToolsNeverWidenTheDocksMinimum)
{
    ChromedWorkspace w;
    const ViewId plan = w.active();
    katana::qt::DockTitleBar* bar = w.chrome->titleBar(w.dock(plan));
    ASSERT_NE(bar, nullptr);
    w.window.resize(900, 400);
    processEvents();
    ASSERT_TRUE(bar->findChild<QToolButton*>("ViewZoomInButton")->isVisible());
    const int wide = bar->minimumSizeHint().width();
    const int dockWide = w.dock(plan)->minimumSizeHint().width();
    w.window.resize(300, 400);
    processEvents();
    ASSERT_FALSE(bar->findChild<QToolButton*>("ViewZoomInButton")->isVisible());
    // The least width does not depend on which optional tools are shown, so
    // a view can always be narrowed past them.
    EXPECT_EQ(bar->minimumSizeHint().width(), wide);
    EXPECT_EQ(w.dock(plan)->minimumSizeHint().width(), dockWide);
}

TEST(ViewChrome, OpeningAViewSplitsTheActiveViewIntoEqualHalvesAlongItsLongerSide)
{
    ChromedWorkspace w;
    const ViewId plan = w.active();
    const QRect whole = w.dock(plan)->geometry();
    // The lone plan fills the workspace, which fills the 800 x 600 window
    // while nothing is in the tray: wider than it is tall.
    ASSERT_EQ(whole, w.views->rect());
    ASSERT_GT(whole.width(), whole.height());
    const int gap = w.separator();

    const ViewId model = w.views->openView(ViewKind::Model3D).id;
    processEvents();
    // Side by side: the two halves and the gap make up the plan's width, and
    // an odd remainder leaves them one pixel apart at most.
    const QRect left = w.dock(plan)->geometry();
    const QRect right = w.dock(model)->geometry();
    EXPECT_EQ(left.topLeft(), whole.topLeft());
    EXPECT_EQ(left.width() + gap + right.width(), whole.width());
    EXPECT_LE(std::abs(left.width() - right.width()), 1);
    EXPECT_EQ(right.x(), left.width() + gap);
    EXPECT_EQ(left.height(), whole.height());
    EXPECT_EQ(right.height(), whole.height());

    // The 3D view, now active, is about 400 wide by nearly 600 tall: taller
    // than it is wide, so the section goes under it.
    ASSERT_GT(right.height(), right.width());
    const ViewId section = w.views->openView(ViewKind::Section).id;
    processEvents();
    // A split inside the side-by-side one. Offscreen, with the plain style's
    // 6 px separator, the halves of 600 - 6 are 297 each; before the fix they
    // were 397 and 197, the new view having been left out of resizeDocks' row.
    const QRect top = w.dock(model)->geometry();
    const QRect bottom = w.dock(section)->geometry();
    EXPECT_EQ(top.topLeft(), right.topLeft());
    EXPECT_EQ(top.height() + gap + bottom.height(), right.height());
    EXPECT_LE(std::abs(top.height() - bottom.height()), 1)
        << "3D " << top.height() << " px, section " << bottom.height() << " px";
    EXPECT_EQ(bottom.y(), top.height() + gap);
    EXPECT_EQ(top.width(), right.width());
    EXPECT_EQ(bottom.width(), right.width());
    // The plan beside them is untouched.
    EXPECT_EQ(w.dock(plan)->geometry(), left);
}

TEST(ViewChrome, AMaximisedViewFillsTheWorkspaceAndRestoreGivesEveryViewBackItsPlace)
{
    ChromedWorkspace w;
    const ViewId plan = w.active();
    const ViewId model = w.views->openView(ViewKind::Model3D).id;
    const ViewId section = w.views->openView(ViewKind::Section).id;
    processEvents();
    const QRect planBefore = w.dock(plan)->geometry();
    const QRect modelBefore = w.dock(model)->geometry();
    const QRect sectionBefore = w.dock(section)->geometry();

    QToolButton* maximise = w.chrome->titleBar(w.dock(model))->maximiseButton();
    maximise->click();
    processEvents();
    EXPECT_EQ(w.dock(model)->geometry(), w.views->rect());
    EXPECT_FALSE(w.onScreen(plan));
    EXPECT_FALSE(w.onScreen(section));

    // The same button, now Restore.
    maximise->click();
    processEvents();
    EXPECT_FALSE(w.chrome->isMaximised(w.dock(model)));
    EXPECT_EQ(w.dock(plan)->geometry(), planBefore);
    EXPECT_EQ(w.dock(model)->geometry(), modelBefore);
    EXPECT_EQ(w.dock(section)->geometry(), sectionBefore);
}

TEST(ViewChrome, QuadPutsTheFourViewsInTheQuartersOfTheWorkspace)
{
    ChromedWorkspace w;
    const ViewId plan = w.active();
    w.views->arrange(LayoutKind::Quad);
    processEvents();
    // Quad opens the kinds it expects in the preset's order - 3D, section,
    // elevation, ids 2, 3 and 4 after the plan's - and splits them as
    // cad::dockSplits(Quad) says: 3D right of the plan, the section under the
    // plan, the elevation under the 3D view.
    ASSERT_EQ(w.views->viewSet().size(), 4U);
    const ViewId model = plan + 1;
    const ViewId section = plan + 2;
    const ViewId elevation = plan + 3;
    const QRect area = w.views->rect();
    const int gap = w.separator();
    // Each half is (extent - gap) / 2, give or take the odd pixel.
    const int halfWidth = (area.width() - gap) / 2;
    const int halfHeight = (area.height() - gap) / 2;
    const auto near = [](const QRect& actual, int x, int y, int width, int height) {
        return std::abs(actual.x() - x) <= 1 && std::abs(actual.y() - y) <= 1 &&
               std::abs(actual.width() - width) <= 1 && std::abs(actual.height() - height) <= 1;
    };
    EXPECT_TRUE(near(w.dock(plan)->geometry(), 0, 0, halfWidth, halfHeight));
    EXPECT_TRUE(near(w.dock(model)->geometry(), halfWidth + gap, 0, halfWidth, halfHeight));
    EXPECT_TRUE(near(w.dock(section)->geometry(), 0, halfHeight + gap, halfWidth, halfHeight));
    EXPECT_TRUE(
        near(w.dock(elevation)->geometry(), halfWidth + gap, halfHeight + gap, halfWidth, halfHeight));
}

TEST(ViewChrome, HidingALayerInAViewsPopupHidesItInThatViewAloneAndIsNoEdit)
{
    ChromedWorkspace w;
    for (const char* name : {"BUILDING", "ROAD"}) {
        ASSERT_TRUE(w.document.execute(katana::commands::createLayer(layerNamed(name))).ok());
    }
    const ViewId plan = w.active();
    const ViewId model = w.views->openView(ViewKind::Model3D).id;
    processEvents();
    const bool modified = w.document.isModified();
    const std::size_t undoable = w.document.history().undoCount();

    ViewLayersPopup* popup = w.views->showLayersPopup(plan);
    ASSERT_NE(popup, nullptr);
    QTreeWidgetItem* building = popup->itemFor("BUILDING");
    ASSERT_NE(building, nullptr);
    // What a click on its box does.
    building->setCheckState(0, Qt::Unchecked);

    EXPECT_TRUE(w.views->viewSet().find(plan)->layers.hides("BUILDING"));
    EXPECT_FALSE(w.views->viewSet().find(plan)->layers.hides("ROAD"));
    EXPECT_FALSE(w.views->viewSet().find(model)->layers.hides("BUILDING"));
    EXPECT_EQ(w.views->hiddenCount(plan), 1U);
    EXPECT_EQ(w.views->hiddenCount(model), 0U);
    // A view's own filter: no command ran, so nothing to save or undo.
    EXPECT_EQ(w.document.isModified(), modified);
    EXPECT_EQ(w.document.history().undoCount(), undoable);
    popup->close();
}

TEST(ViewChrome, IsolatingANodeThatIsNotALayerHidesTheSiblingsOfItAndOfEachAncestor)
{
    ChromedWorkspace w;
    for (const char* name : {"ANNOT", "design/road", "design/surface/tin1", "survey/points"}) {
        ASSERT_TRUE(w.document.execute(katana::commands::createLayer(layerNamed(name))).ok());
    }
    const ViewId plan = w.active();

    ViewLayersPopup* popup = w.views->showLayersPopup(plan);
    ASSERT_NE(popup, nullptr);
    // design/surface is a node of the tree only: no layer has that name.
    QTreeWidgetItem* surface = popup->itemFor("design/surface");
    ASSERT_NE(surface, nullptr);
    popup->layerTree()->setCurrentItem(surface);
    ASSERT_TRUE(popup->isolateSelected().ok());

    // Kept: design/surface, what is under it (tin1) and its ancestor design.
    // Hidden: design's siblings at the root - the default layer 0, ANNOT and
    // survey (which takes survey/points with it) - and surface's sibling
    // under design, design/road.
    const std::set<std::string, std::less<>> expected{"0", "ANNOT", "design/road", "survey"};
    EXPECT_EQ(w.views->viewSet().find(plan)->layers.hidden(), expected);
    popup->close();
}
