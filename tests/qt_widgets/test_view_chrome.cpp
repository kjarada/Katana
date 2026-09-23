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

#include <QMainWindow>
#include <QToolButton>

#include "dock_chrome.hpp"
#include "view_workspace.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::cad::LayoutKind;
using katana::cad::ViewId;
using katana::cad::ViewKind;
using katana::qt::DockChrome;
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
