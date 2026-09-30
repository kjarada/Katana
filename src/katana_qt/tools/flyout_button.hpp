#pragma once

// The toolbar button of a tool family, and the check that holds the menu
// signs of every toolbar and dock title bar off their icons.
//
// A family ("Circle": Centre Radius, 2 Points, 3 Points ...) is one button on
// its toolbar (tool_menus.hpp). A click runs the family's first tool. The
// others are one gesture away: a press on the small triangle in the button's
// bottom-right corner, a press held (Qt's DelayedPopup) or a right click. The
// triangle is the sign that the button holds more than one tool, drawn in the
// corner the centred icon leaves free, as drawing and graphics programs mark
// their tool groups. The button is framed as running only while the tool its
// icon shows runs, as every tool button is; while any of the family's tools
// runs - that one or one chosen from the menu - the triangle takes the
// accent, and the tooltip names the one that runs.
//
// It replaced Qt's split button (QToolButton::MenuButtonPopup), whose arrow
// the application's stylesheet drew OVER the icon. For a button whose rule has
// padding or a border, QStyleSheetStyle reserves no width for the arrow's
// strip (PM_MenuButtonIndicator is 0), yet still lays the strip over the
// button's right 12 px, paints it after the icon and takes presses there: a
// click on the right of the circle's ring opened the menu. docs/desktop.md,
// "Tool families on the toolbars", has the alternatives measured and the
// reasons for this one.

#include <QList>
#include <QMenu>
#include <QPointer>
#include <QRect>
#include <QRegion>
#include <QString>
#include <QStringList>
#include <QToolButton>

class QAction;
class QToolBar;
class QWidget;

namespace katana::qt::tools {

// No Q_OBJECT (docs/desktop.md, "The rules a dialog or panel follows"): it
// overrides virtuals only, and the stylesheet sees it as the QToolButton it
// must look like. Find it with dynamic_cast, not qobject_cast.
class FlyoutButton final : public QToolButton {
  public:
    // The button of `family`, a menu of the family's tools that becomes its
    // menu; the caller gives it the default action a click runs.
    explicit FlyoutButton(QMenu& family, QWidget* parent = nullptr);

    // The square the style draws the icon in. QCommonStyle centres an
    // icon-only button's icon in the button's contents (CE_ToolButtonLabel),
    // and the theme insets the contents alike on every side, so it is the
    // icon's size centred in the button - the button being the size its
    // toolbar's icons make it (followToolBarIconSize, below).
    [[nodiscard]] QRect iconRect() const;
    // The corner mark's square: in the bottom-right corner iconRect leaves
    // free, inside the button's frame.
    [[nodiscard]] QRect markRect() const;
    // Where a left press opens the menu rather than running the tool: the
    // bottom-right corner, 12 px each way as the strip it replaces was wide,
    // less the icon's square, so a press on the icon always runs the tool.
    [[nodiscard]] QRegion menuZone() const;
    // The tooltip shown: the tool's own, then a line saying how to open the
    // menu and naming the family's other tools - five at most, past that
    // four and how many more - and, while another of them runs, a line
    // naming it. Built when shown, since a family's tools are added to its
    // menu after the button is made.
    [[nodiscard]] QString toolTipWithMenu() const;
    // Whether the pointer is over menuZone, where the mark is lit. Cleared
    // whenever the pointer leaves the button, however Qt says so.
    [[nodiscard]] bool pointerOnMark() const { return pointerOnMark_; }
    // The family's tool that runs, if one does: the one a click starts or
    // any other from its menu; null while none does.
    [[nodiscard]] const QAction* runningTool() const;
    // Whether a tool of the family runs. The mark is lit in the accent while
    // one does; the button is framed only while it is the one its icon
    // shows (isChecked), since a frame round Insert Vertex's icon says
    // Insert Vertex runs.
    [[nodiscard]] bool familyRunning() const;

  protected:
    bool event(QEvent* event) override;
    bool eventFilter(QObject* watched, QEvent* event) override;
    void paintEvent(QPaintEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void contextMenuEvent(QContextMenuEvent* event) override;

  private:
    // The status bar's line for where the pointer is: the family's own
    // ("Vertices: Insert Vertex / Delete Vertex / ...") on the corner that
    // opens it, the tool's elsewhere on the button, as QWidget shows it on
    // Enter.
    void showStatusTip(bool onMark);

    // The family the button stands for, which its running tool is read from.
    // Kept apart from menu(), what a press opens, which drawnParts takes away
    // for a moment to see what it adds.
    QPointer<QMenu> family_;
    bool pointerOnMark_ = false;
};

// Keeps a button put on `bar` with addWidget at the bar's icon size, as
// QToolBar keeps the buttons it makes for its own actions (its layout
// connects the bar's iconSizeChanged to their setIconSize). A widget's
// button is otherwise never told, and QToolButton caches its size hint:
// after View > Toolbars > Large Icons the tool families stayed the size they
// were made at while the bar's own buttons grew, their icons drawn smaller
// than their neighbours' and their marks squeezed to 3 px. The families
// (tool_menus.cpp) and Undo and Redo (main_window.cpp) follow their bars.
void followToolBarIconSize(QToolButton& button, QToolBar& bar);

// Lights `bar`'s overflow arrow in the accent (theme::kToolRunsBehindOverflow)
// while a button the arrow hides shows a tool running: a tool's button
// framed, or a family with one of its tools running. At the window's first
// size the Draw toolbar hides Ellipse, Divide, Measure and all eighteen
// Vertices tools behind its arrow, and with one of them running nothing on
// the toolbars said so. `tools` are the tools: only their buttons light it
// (Select, checked while no tool runs, does not), and it hears them start
// and stop; it hears the toolbar hide and show its buttons as their room
// changes. fillToolMenus calls it for each toolbar it fills.
void lightOverflowWhileAHiddenToolRuns(QToolBar& bar, const QList<QAction*>& tools);

// Where a tool button draws its icon and the sign of its menu, each measured
// from its own rendering against itself: the icon's square as where the
// button with an opaque square for its icon differs from the button with a
// clear one, the sign as where the button with its menu differs from the
// button without. A null rect for what it does not draw. The button is
// restored after; one the toolbar has hidden behind its overflow arrow is
// measured at its size hint.
struct DrawnParts {
    // In the widget's own pixels; on a scaled screen each box is rounded
    // outwards to the widget's pixels it touches.
    QRect icon;
    QRect sign;
    // The same in the grab's device pixels, where a clash is judged. At a
    // ratio of 1.25 an icon's square ending at device column 32 and a mark
    // starting at 33 both touch the widget's pixel 26: rounded outwards they
    // overlapped though nothing on the screen did.
    QRect iconDevice;
    QRect signDevice;
    double ratio = 1.0;
};
[[nodiscard]] DrawnParts drawnParts(QToolButton& button);

// Every icon-only button with a menu under `root` whose menu sign is drawn
// over its icon's square, in device pixels, or not drawn at all: on each
// toolbar ("toolbar button <bar> > <button>: <why>"), then in each dock's
// title bar ("title bar button <dock> > <button>: <why>" - a view's kind
// switcher). `checked`
// receives a line for each button measured, clear or not, and `buttons` how
// many there were. A toolbar's own overflow button is not one of them: it
// draws its menu arrow as its icon. katana --check-toolbars fails a run with
// any (docs/headless.md).
[[nodiscard]] QStringList menuSignClashes(QWidget& root, QStringList* checked = nullptr,
                                          int* buttons = nullptr);

} // namespace katana::qt::tools
