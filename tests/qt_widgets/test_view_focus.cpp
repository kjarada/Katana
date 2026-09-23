// When the keyboard focus arriving in a view makes it the active view.
//
// Each view raises onActivated when the user clicks into it or moves the
// focus into it, and the workspace makes that view the one the menus, Zoom
// Extents and F9 act on. The user moving the focus is a choice of view. Qt
// moving it is not: when the focused widget is hidden, Qt hands the focus to
// the next widget in the tab chain, and the workspace hides a view whenever
// it changes the view's kind (QDockWidget::setWidget), closes it or rearranges
// the docks (removeDockWidget). A window coming back to the front gives its
// focus back to whichever widget last held it. Before the fix every one of
// those made some unrelated view active.
//
// The workspace itself is not built here: these tests put the views in a
// QMainWindow's docks and do to them what ViewWorkspace does, so that they
// test the widgets' rule and not the workspace's current code.

#include <gtest/gtest.h>

#include <QDockWidget>
#include <QHBoxLayout>
#include <QKeyEvent>
#include <QMainWindow>

#include "widget_harness.hpp"

using katana::cad::Document;
using katana::cad::ViewKind;
using katana::cad::ViewSet;
using katana::qt::test::makeView;
using katana::qt::test::processEvents;
using katana::qt::test::showActive;

namespace {

const char* kindName(ViewKind kind)
{
    switch (kind) {
    case ViewKind::Plan:
        return "Plan";
    case ViewKind::Model3D:
        return "Model3D";
    case ViewKind::Section:
        return "Section";
    case ViewKind::Elevation:
        return "Elevation";
    }
    return "Unknown";
}

// Two views side by side in one window: the one that has the focus, then the
// one the tab chain reaches next - of the kind under test, since each of the
// three widgets carries the rule.
struct TwoViewsInAWindow {
    Document document;
    ViewSet views;
    QWidget window;
    QWidget* holder = nullptr;
    QWidget* next = nullptr;
    int holderActivations = 0;
    int nextActivations = 0;

    explicit TwoViewsInAWindow(ViewKind nextKind)
    {
        auto* layout = new QHBoxLayout(&window);
        holder = makeView(ViewKind::Plan, document, views.add(ViewKind::Plan), &window,
                          [this] { ++holderActivations; });
        next = makeView(nextKind, document, views.add(nextKind), &window,
                        [this] { ++nextActivations; });
        layout->addWidget(holder);
        layout->addWidget(next);
        window.resize(640, 320);
    }
};

class ViewFocusByKind : public testing::TestWithParam<ViewKind> {};

} // namespace

TEST_P(ViewFocusByKind, AViewGivenTheFocusBecauseTheFocusedViewWasHiddenIsNotActivated)
{
    TwoViewsInAWindow views(GetParam());
    ASSERT_TRUE(showActive(views.window));
    views.holder->setFocus(Qt::MouseFocusReason);
    processEvents();
    ASSERT_TRUE(views.holder->hasFocus());
    views.holderActivations = 0;
    views.nextActivations = 0;

    views.holder->hide();

    // The situation under test: Qt itself moved the focus on to the next view.
    ASSERT_EQ(QApplication::focusWidget(), views.next);
    EXPECT_EQ(views.nextActivations, 0) << "Qt's move is not the user choosing that view";
}

TEST_P(ViewFocusByKind, TabPressedInAViewActivatesTheViewTheFocusMovesTo)
{
    // The rule must not throw away the move it exists for: Tab from a view
    // that is still there is the user going to the next view.
    TwoViewsInAWindow views(GetParam());
    ASSERT_TRUE(showActive(views.window));
    views.holder->setFocus(Qt::MouseFocusReason);
    processEvents();
    ASSERT_TRUE(views.holder->hasFocus());
    views.nextActivations = 0;

    QKeyEvent tab(QEvent::KeyPress, Qt::Key_Tab, Qt::NoModifier);
    QApplication::sendEvent(views.holder, &tab);

    ASSERT_EQ(QApplication::focusWidget(), views.next);
    EXPECT_EQ(views.nextActivations, 1);
}

TEST_P(ViewFocusByKind, FocusGivenBackByAMenuActivatesNothing)
{
    // A closing popup menu, or the menu bar left by the keyboard, gives the
    // focus back to the widget it was taken from: a return, not a choice.
    for (const Qt::FocusReason reason : {Qt::PopupFocusReason, Qt::MenuBarFocusReason}) {
        TwoViewsInAWindow views(GetParam());
        ASSERT_TRUE(showActive(views.window));
        views.holder->setFocus(Qt::MouseFocusReason);
        processEvents();
        views.nextActivations = 0;

        views.next->setFocus(reason);

        ASSERT_TRUE(views.next->hasFocus());
        EXPECT_EQ(views.nextActivations, 0) << "focus reason " << static_cast<int>(reason);
    }
}

INSTANTIATE_TEST_SUITE_P(EachViewWidget, ViewFocusByKind,
                         testing::Values(ViewKind::Plan, ViewKind::Model3D, ViewKind::Section),
                         [](const testing::TestParamInfo<ViewKind>& kind) {
                             return std::string(kindName(kind.param));
                         });

namespace {

// Three docked views, as the workspace holds them: a plan, a 3D view and a
// section, each in its own dock of one QMainWindow.
struct DockedViews {
    Document document;
    ViewSet views;
    QMainWindow window;
    QDockWidget* docks[3] = {};
    int activations[3] = {};

    DockedViews()
    {
        window.setDockNestingEnabled(true);
        const ViewKind kinds[3] = {ViewKind::Plan, ViewKind::Model3D, ViewKind::Section};
        for (int i = 0; i < 3; ++i) {
            docks[i] = new QDockWidget(QString("View%1").arg(i + 1), &window);
            docks[i]->setWidget(makeView(kinds[i], document, views.add(kinds[i]), docks[i],
                                         [this, i] { ++activations[i]; }));
            window.addDockWidget(Qt::LeftDockWidgetArea, docks[i]);
        }
        window.resize(900, 500);
    }

    void focusView(int index)
    {
        docks[index]->widget()->setFocus(Qt::MouseFocusReason);
        processEvents();
        activations[0] = activations[1] = activations[2] = 0;
    }

    [[nodiscard]] bool focusIsInAnotherViewThan(int index) const
    {
        const QWidget* focus = QApplication::focusWidget();
        for (int i = 0; i < 3; ++i) {
            if (i != index && focus != nullptr && focus == docks[i]->widget()) {
                return true;
            }
        }
        return false;
    }
};

} // namespace

TEST(ViewFocus, ReplacingTheFocusedViewInItsDockActivatesNoOtherView)
{
    // ViewWorkspace::buildContent on a change of kind: a new widget is set on
    // the dock, which hides the old one, and the old one is deleted. Before
    // the fix the view the focus fell to became active (the review: views
    // Plan, 3D, Section, Elevation with the 3D view active; turning it into
    // a plan made the section active).
    DockedViews docked;
    ASSERT_TRUE(showActive(docked.window));
    docked.focusView(1);

    QWidget* old = docked.docks[1]->widget();
    QWidget* replacement = makeView(ViewKind::Plan, docked.document, *docked.views.find(2),
                                    docked.docks[1], [&docked] { ++docked.activations[1]; });
    docked.docks[1]->setWidget(replacement);
    delete old;
    processEvents();

    ASSERT_TRUE(docked.focusIsInAnotherViewThan(1)) << "Qt moved the focus to another view";
    EXPECT_EQ(docked.activations[0], 0);
    EXPECT_EQ(docked.activations[2], 0);
}

TEST(ViewFocus, RemovingTheFocusedViewsDockActivatesNoOtherView)
{
    // ViewWorkspace::closeView and arrange: removeDockWidget hides the dock.
    // On a close the workspace's ViewSet already chooses the view that takes
    // over (the most recently active of the rest); the focus must not
    // overrule it with whichever view the tab chain reaches.
    DockedViews docked;
    ASSERT_TRUE(showActive(docked.window));
    docked.focusView(1);

    docked.window.removeDockWidget(docked.docks[1]);
    processEvents();

    ASSERT_TRUE(docked.focusIsInAnotherViewThan(1)) << "Qt moved the focus to another view";
    EXPECT_EQ(docked.activations[0], 0);
    EXPECT_EQ(docked.activations[2], 0);
}

TEST(ViewFocus, AWindowComingBackGivesTheFocusBackWithoutActivatingTheViewHoldingIt)
{
    // After a dialog, or on the way to the menu bar from a floating view, the
    // main window comes back and Qt hands its focus to the widget that last
    // held it (ActiveWindowFocusReason). That is not a choice of view: the
    // active view may well be another one (a floating view, or one the
    // workspace activated in code), and it must stay active.
    DockedViews docked;
    ASSERT_TRUE(showActive(docked.window));
    docked.focusView(0);

    QWidget dialog;
    dialog.resize(200, 100);
    ASSERT_TRUE(showActive(dialog));
    ASSERT_TRUE(showActive(docked.window));

    ASSERT_TRUE(docked.docks[0]->widget()->hasFocus()) << "the focus came back to the plan";
    EXPECT_EQ(docked.activations[0], 0);
}

TEST(ViewFocus, BringingAFloatingViewsWindowForwardActivatesThatView)
{
    // A floating view is a window of its own; bringing it forward is how the
    // user chooses it without clicking into the drawing. A floating dock is
    // a window with a parent - the workspace - which is what tells it from
    // the main window.
    DockedViews docked;
    ASSERT_TRUE(showActive(docked.window));
    docked.focusView(0);

    docked.docks[2]->setFloating(true);
    processEvents();
    ASSERT_TRUE(docked.docks[2]->isWindow());
    // Back to the main window first, in case floating the dock activated its
    // window on its own: the move under test is the one that follows.
    ASSERT_TRUE(showActive(docked.window));
    docked.activations[2] = 0;
    ASSERT_TRUE(showActive(*docked.docks[2]));

    ASSERT_TRUE(docked.docks[2]->widget()->hasFocus());
    EXPECT_EQ(docked.activations[2], 1);
}
