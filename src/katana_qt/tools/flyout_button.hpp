#pragma once

// The toolbar button of a tool family, and the check that holds every
// toolbar's menu signs off their icons.
//
// A family ("Circle": Centre Radius, 2 Points, 3 Points ...) is one button on
// its toolbar (tool_menus.hpp). A click runs the family's first tool. The
// others are one gesture away: a press on the small triangle in the button's
// bottom-right corner, a press held (Qt's DelayedPopup) or a right click. The
// triangle is the sign that the button holds more than one tool, drawn in the
// corner the centred icon leaves free, as drawing and graphics programs mark
// their tool groups.
//
// It replaced Qt's split button (QToolButton::MenuButtonPopup), whose arrow
// the application's stylesheet drew OVER the icon. For a button whose rule has
// padding or a border, QStyleSheetStyle reserves no width for the arrow's
// strip (PM_MenuButtonIndicator is 0), yet still lays the strip over the
// button's right 12 px, paints it after the icon and takes presses there: a
// click on the right of the circle's ring opened the menu. docs/desktop.md,
// "Tool families on the toolbars", has the alternatives measured and the
// reasons for this one.

#include <QRect>
#include <QRegion>
#include <QString>
#include <QStringList>
#include <QToolButton>

class QWidget;

namespace katana::qt::tools {

// No Q_OBJECT (docs/desktop.md, "The rules a dialog or panel follows"): it
// overrides virtuals only, and the stylesheet sees it as the QToolButton it
// must look like. Find it with dynamic_cast, not qobject_cast.
class FlyoutButton final : public QToolButton {
  public:
    explicit FlyoutButton(QWidget* parent = nullptr);

    // The square the style draws the icon in. QCommonStyle centres an
    // icon-only button's icon in the button's contents (CE_ToolButtonLabel),
    // and the theme insets the contents alike on every side, so it is the
    // icon's size centred in the button.
    [[nodiscard]] QRect iconRect() const;
    // The corner mark's square: in the bottom-right corner iconRect leaves
    // free, inside the button's frame.
    [[nodiscard]] QRect markRect() const;
    // Where a left press opens the menu rather than running the tool: the
    // bottom-right corner as wide as the 12 px strip it replaces, less the
    // icon's square, so a press on the icon always runs the tool.
    [[nodiscard]] QRegion menuZone() const;
    // The tooltip shown: the tool's own, then a line naming the family's
    // other tools and the three ways to them. Built when shown, since a
    // family's tools are added to its menu after the button is made.
    [[nodiscard]] QString toolTipWithMenu() const;
    // Whether the pointer is over menuZone, where the mark is lit.
    [[nodiscard]] bool pointerOnMark() const { return pointerOnMark_; }

  protected:
    bool event(QEvent* event) override;
    void paintEvent(QPaintEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void contextMenuEvent(QContextMenuEvent* event) override;

  private:
    bool pointerOnMark_ = false;
};

// Where a tool button draws its icon and the sign of its menu, each measured
// from its own rendering against itself: the icon's square as where the
// button with an opaque square for its icon differs from the button with a
// clear one, the sign as where the button with its menu differs from the
// button without. A null rect for what it does not draw. The button is
// restored after; one the toolbar has hidden behind its overflow arrow is
// measured at its size hint.
struct DrawnParts {
    QRect icon;
    QRect sign;
};
[[nodiscard]] DrawnParts drawnParts(QToolButton& button);

// Every icon-only button with a menu on a toolbar under `root` whose menu
// sign is drawn over its icon's square or not drawn at all, as
// "<toolbar> > <button>: <why>". `checked` receives a line for each button
// measured, clear or not, and `buttons` how many there were. The toolbar's
// own overflow button is not one of them: it draws its menu arrow as its
// icon. katana --check-toolbars fails a run with any (docs/headless.md).
[[nodiscard]] QStringList menuSignClashes(QWidget& root, QStringList* checked = nullptr,
                                          int* buttons = nullptr);

} // namespace katana::qt::tools
