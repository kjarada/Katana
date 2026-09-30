#pragma once

// The chrome every dock wears - the four panels and every drawing view
// (the user's request of 2026-09-23 for panels and views that minimise,
// float, maximise and close, and come back where they were; docs/cad.md,
// "The workspace", for the views as docks).
//
// WHY A TITLE BAR OF OUR OWN. QDockWidget's title bar has two buttons, Float
// and Close, drawn by the style - and Fusion drew them light on the dark
// theme, which is why theme.cpp used to strip them (titlebar-close-icon:
// none) and no dock could be closed or floated except by dragging. It has no
// Minimise, no Maximise and nowhere for a view's own controls (its kind, its
// layers). QDockWidget::setTitleBarWidget replaces the whole bar: Qt then
// draws no buttons, and a floating dock gets no native frame - Qt still
// resizes it from its edges (4 px wide by the theme and outlined by the
// chrome, since Fusion's was one pixel and invisible), and dragging the bar still
// moves the dock and re-docks it, PROVIDED the bar ignores the mouse events
// it does not use (DockTitleBar does, and must go on doing so).
//
// WHY MINIMISE IS A TRAY. A floating dock is a Qt::Tool window, and should
// stay one: Qt's shortcut matcher relays a Tool window's shortcuts to its
// parent, so Esc, Delete, Ctrl+Z and F9 keep working in a view floated onto
// another screen, where a Qt::Window would lose every one of them (and Qt
// resets the flags on every float anyway). A Tool window has no taskbar
// button, so the window manager's minimise would lose it; and a docked panel
// is not a window at all. So Minimise HIDES the dock and puts a button for it
// on a toolbar at the bottom of the main window, and the button puts it back
// exactly where it was: in its area and tab when docked, at its geometry on
// its screen when floating. A hidden dock keeps its place in QMainWindow's
// layout tree, so "where" comes free; its SIZE does not (the neighbours grow
// into the space and Qt shares the shortfall equally when it comes back), so
// the sizes of the docks around it are recorded and put back with it.
//
// REJECTED: rolling a dock up to its title bar. Measured offscreen in the
// 1360 x 860 window: Properties alone in the right area, its contents hidden
// and its height capped at its bar, took the whole column with it - 270 x 770
// became 131 x 26, and the column's space went to the drawing sideways;
// Layers rolled up above Reference Data narrowed the left column from 300 to
// 135 px, and the column stayed narrow after it was unrolled. Only a floating
// dock behaved (770 px tall became 28). Qt sizes a dock area from its docks'
// hints, and a dock with its contents hidden hints at the width of its bar.
// Minimise does the job for docked and floating docks alike and moves
// nothing else.
//
// ONE OWNER. One DockChrome per main window holds the tray and the
// bookkeeping, and the panels (MainWindow) and the views (ViewWorkspace)
// share it: a panel and a view minimised into two different trays would be
// two ways of doing one thing. It knows nothing of views or panels beyond the
// DockRole it is told.

#include <functional>
#include <limits>
#include <vector>

#include <QPointer>
#include <QRect>
#include <QSize>
#include <QStringList>
#include <QWidget>

#include "icons.hpp"

class QAction;
class QDockWidget;
class QHBoxLayout;
class QMainWindow;
class QToolBar;
class QToolButton;

namespace katana::qt {

class DockChrome;
class ElidedTitle;

enum class DockRole {
    Panel, // Close hides it; the menus bring it back
    View,  // a drawing view: it can be maximised, and one of them is active
};

// A compact, themed button for a title bar - the window buttons and a view's
// own tools use the same one, so the bar reads as one row. `name` is its
// object name (what a test finds it by); `tip` is shown as the tooltip under
// `label`, which is also its accessible name for a screen reader.
[[nodiscard]] QToolButton* makeTitleBarButton(QWidget* parent, Icon icon, const QString& name,
                                              const QString& label, const QString& tip);
// Sets such a button's tooltip - `tip` under `label` - and its accessible
// name and description, for a button whose words change with what it does
// (a view's Link and zoom tools).
void setTitleBarTip(QToolButton* button, const QString& label, const QString& tip);

// The title bar of one dock: [icon] title ... [tools] [_] [float] [max] [x].
class DockTitleBar final : public QWidget {
  public:
    DockTitleBar(QDockWidget& dock, DockChrome& chrome, Icon icon, DockRole role);

    [[nodiscard]] QDockWidget& dock() const { return dock_; }
    [[nodiscard]] DockRole role() const { return role_; }
    [[nodiscard]] Icon icon() const { return icon_; }

    void setIcon(Icon icon);
    // Puts `widget` where the icon was: a view's kind switcher is its icon.
    void setLeadingWidget(QWidget* widget);

    // A tool shown whatever the bar's width.
    static constexpr int kAlways = std::numeric_limits<int>::max();
    // What an optional tool leaves the title: room to read it whole - its
    // text's width and kTitleGap after it - and never more than kTitleRoom,
    // so a narrow view keeps its name before its extras and a long name does
    // not crowd them all out. A flat 56 px left a 69 px gap beside "Plan 1"
    // at the owner's two-up width, where Zoom In and Out needed 46.
    static constexpr int kTitleRoom = 56;
    // Between the title's text and the first tool: the 6 px the bar leaves
    // between its tools and the window buttons, so the three read as apart.
    static constexpr int kTitleGap = 6;
    // Adds `widget` to the slot between the title and the window buttons, in
    // the order added. With a `priority` below kAlways the tool is OPTIONAL:
    // shown only while it fits with the title's room left for it, the
    // highest priority first - tools of one priority come and go together -
    // and never counted in the bar's least width, so it never stops a view
    // being narrowed. Every optional tool is also in a menu: nothing is lost
    // when one is hidden, and a menu of the hidden ones would be a modal loop
    // a headless press waits in for ever.
    void addTool(QWidget* widget, int priority = kAlways);
    // Whether the owner wants a tool on the bar at all - a plan view's Link,
    // a view's zoom tools where its kind has none. An optional tool that is
    // wanted is still shown only where it fits.
    void setToolWanted(QWidget* widget, bool wanted);

    // The least width the always-shown tools, the window buttons and the
    // title's least need: an optional tool is never in it.
    [[nodiscard]] QSize minimumSizeHint() const override;

    // The active view's bar carries the accent. Panels are never active.
    void setActive(bool active);
    [[nodiscard]] bool isActive() const { return active_; }

    // Re-reads the dock: its title, floating or docked, maximised or not, and
    // the features it allows (a dock that cannot float shows no Float).
    void refresh();

    // The window buttons, for a test to click as a person does. Maximise is
    // null on a panel.
    [[nodiscard]] QToolButton* minimiseButton() const { return minimise_; }
    [[nodiscard]] QToolButton* floatButton() const { return float_; }
    [[nodiscard]] QToolButton* maximiseButton() const { return maximise_; }
    [[nodiscard]] QToolButton* closeButton() const { return close_; }

    // Raised by a press anywhere on the bar that is not a button, before Qt
    // starts a drag: clicking a view's title makes it the active view.
    std::function<void()> onPressed;
    // Raised by DockChrome once minimise() has hidden this dock in the tray,
    // and once restore() has brought it back, whoever called them - its own
    // Minimise button, minimiseNamed for a saved layout, the tray button,
    // ViewWorkspace::ensureView. Not when something else shows a minimised
    // dock (View > Panels, ViewWorkspace::arrange): that only drops its tray
    // button. On the bar rather than on DockChrome because each dock has one
    // owner (the window for a panel, the workspace for a view) and a single
    // hook on the shared chrome would be one owner's to overwrite.
    std::function<void()> onMinimised;
    std::function<void()> onRestored;

  protected:
    void paintEvent(QPaintEvent* event) override;
    void resizeEvent(QResizeEvent* event) override;
    // Press, move and release IGNORE the event after looking at it.
    // QDockWidget moves and re-docks itself from the mouse events its title
    // bar passes up; a bar that accepts them makes a dock that cannot be
    // dragged. A double click is the one taken here: it is the Float button,
    // through DockChrome, so that floating and docking again give back the
    // sizes around the dock whichever of the two the user used.
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;
    void mouseDoubleClickEvent(QMouseEvent* event) override;

  private:
    void updateTitle();
    // Shows the optional tools that fit, highest priority first, and hides
    // the rest: at every resize and whenever a tool is added or wanted.
    void fitOptionalTools();
    // Marks both rows of the bar to be measured again (the bar's, and the
    // tools' nested in it), so that the next minimumSizeHint counts only the
    // tools shown now.
    void remeasure();
    // The width the shown optional tools take in the tools' row, their
    // spacing with them.
    [[nodiscard]] int optionalWidth() const;

    struct Tool {
        QWidget* widget = nullptr;
        int priority = kAlways;
        bool wanted = true;
    };
    std::vector<Tool> toolList_;

    QDockWidget& dock_;
    QPointer<DockChrome> chrome_;
    DockRole role_;
    Icon icon_;
    bool active_ = false;

    QHBoxLayout* layout_ = nullptr;
    QHBoxLayout* tools_ = nullptr;
    QWidget* iconLabel_ = nullptr;
    QWidget* leading_ = nullptr;
    ElidedTitle* title_ = nullptr;
    QToolButton* minimise_ = nullptr;
    QToolButton* float_ = nullptr;
    QToolButton* maximise_ = nullptr;
    QToolButton* close_ = nullptr;
};

class DockChrome final : public QObject {
  public:
    // Adds the tray - a toolbar named MinimisedToolBar - to the BOTTOM of
    // `window`, hidden while nothing is minimised. `window` owns this object.
    explicit DockChrome(QMainWindow& window);

    DockChrome(const DockChrome&) = delete;
    DockChrome& operator=(const DockChrome&) = delete;

    // Gives `dock` a DockTitleBar and puts it under this chrome's bookkeeping.
    // `icon` is on its bar and on its tray button. A dock installed twice
    // keeps its first bar, which is returned.
    DockTitleBar* install(QDockWidget* dock, Icon icon, DockRole role);
    [[nodiscard]] DockTitleBar* titleBar(const QDockWidget* dock) const;

    // ---- minimise ----------------------------------------------------------
    // Hides `dock` and puts a button for it on the tray. A maximised view is
    // restored first, so the views it hid come back. Works on a dock that is
    // already hidden - one a saved layout restored hidden - whose place and
    // geometry Qt still holds. Does nothing for a dock not installed here.
    void minimise(QDockWidget* dock);
    // Puts a minimised dock back where it was, raised to the front of its
    // tab group; false when it is not minimised. A docked view maximised in
    // the same window meanwhile is restored first: the dock comes back beside
    // it, so the maximise is over.
    bool restore(QDockWidget* dock);
    [[nodiscard]] bool isMinimised(const QDockWidget* dock) const;
    // In the order they were minimised, which is the tray's order.
    [[nodiscard]] std::vector<QDockWidget*> minimisedDocks() const;
    [[nodiscard]] QToolBar* tray() const;
    // The tray's button for a minimised dock; null when it is not minimised.
    [[nodiscard]] QToolButton* trayButton(const QDockWidget* dock) const;

    // For the saved layout. The object names of the minimised docks, in tray
    // order; and minimising by name after QMainWindow::restoreState, which
    // restores them hidden but cannot know why. Names that no installed dock
    // carries are returned rather than dropped, so the caller can say so.
    [[nodiscard]] QStringList minimisedNames() const;
    QStringList minimiseNamed(const QStringList& names);

    // ---- float -------------------------------------------------------------
    // Floats a docked dock where it stands, or docks a floating one back where
    // it came from with the sizes it had there.
    void toggleFloating(QDockWidget* dock);

    // ---- maximise (views) --------------------------------------------------
    // A docked view: every other docked view of the same window is hidden
    // until unmaximise, so this one has the whole drawing area. A floating
    // view: it fills the available geometry of the screen it is on. Any
    // number of floating views may be maximised, one on each screen; one
    // docked view per window.
    void maximise(QDockWidget* dock);
    // Undoes maximise(dock) - the hidden views come back at the sizes they
    // had, a floating view goes back to its geometry. False when `dock` is
    // not maximised.
    bool unmaximise(QDockWidget* dock);
    void toggleMaximised(QDockWidget* dock);
    // Undoes every docked maximise in `host`. A workspace calls it before it
    // opens or arranges views: a maximised view is a temporary state, not a
    // layout, and a split made while the others were hidden would be made
    // against the wrong neighbours.
    void unmaximiseDocked(const QMainWindow& host);
    // Undoes every maximise - before a layout is saved.
    void unmaximiseAll();
    [[nodiscard]] bool isMaximised(const QDockWidget* dock) const;

    // Drops every trace of `dock`, putting back what it changed: its tray
    // button goes and, when it was the maximised view, the views it hid come
    // back. A workspace calls it before deleting a view's dock. A dock that is
    // simply destroyed is dropped without putting anything back, because it
    // goes with its window.
    void forget(QDockWidget* dock);

    // Raised whenever the set of minimised docks or a maximise changes, for a
    // menu that lists them.
    std::function<void()> onChanged;

  protected:
    bool eventFilter(QObject* watched, QEvent* event) override;

  private:
    friend class DockTitleBar; // setIcon updates the tray button

    // A minimised dock's tray button follows its title bar's icon and title:
    // a view can change kind while minimised, from the View menu.
    void refreshTray(const QDockWidget* dock);

    struct Installed {
        QPointer<QDockWidget> dock;
        DockTitleBar* bar = nullptr;
        DockRole role = DockRole::Panel;
    };
    // The size a docked dock had, to be given back to it.
    struct DockSize {
        QPointer<QDockWidget> dock;
        QSize size;
    };
    struct Minimised {
        QPointer<QDockWidget> dock;
        bool floating = false;
        QRect geometry;              // floating: where it was, on its screen
        std::vector<DockSize> sizes; // docked: its own and its neighbours'
        QAction* action = nullptr;   // on the tray
    };
    struct Maximised {
        QPointer<QDockWidget> dock;
        bool floating = false;
        QRect geometry;                            // floating: where to go back to
        std::vector<QPointer<QDockWidget>> hidden; // docked: the views hidden for it
        std::vector<DockSize> sizes;               // docked: everything's size before
    };

    [[nodiscard]] const Installed* installed(const QDockWidget* dock) const;
    [[nodiscard]] static std::vector<DockSize> snapshot(const QMainWindow& host);
    static void applySizes(QMainWindow& host, const std::vector<DockSize>& sizes);
    // Drops the records of docks that have been destroyed, without putting
    // anything back.
    void prune();
    void refreshBar(const QDockWidget* dock) const;
    void notify() const;

    // A QPointer because the tray is the main window's child and this is
    // too: which of the two Qt deletes first is the order they were made in.
    QPointer<QToolBar> tray_;
    std::vector<Installed> installed_;
    std::vector<Minimised> minimised_;
    std::vector<Maximised> maximised_;
    // The sizes around a dock floated by its button or a double click on its
    // title, given back when the same puts it back.
    struct Floated {
        QPointer<QDockWidget> dock;
        std::vector<DockSize> sizes;
    };
    std::vector<Floated> floated_;
};

} // namespace katana::qt
