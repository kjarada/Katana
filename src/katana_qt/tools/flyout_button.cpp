#include "tools/flyout_button.hpp"

#include <algorithm>
#include <cmath>

#include <QAction>
#include <QContextMenuEvent>
#include <QHelpEvent>
#include <QHoverEvent>
#include <QIcon>
#include <QImage>
#include <QMenu>
#include <QMouseEvent>
#include <QPainter>
#include <QPixmap>
#include <QStyle>
#include <QStyleOptionToolButton>
#include <QStylePainter>
#include <QToolBar>
#include <QToolTip>

#include "theme.hpp"

namespace katana::qt::tools {

namespace {

// The mark's two short sides, at most and at least. Five is what the theme's
// buttons leave between the icon's square and the frame (below) at every
// toolbar icon size; three is the smallest triangle that still reads as one.
constexpr int kMaxLeg = 5;
constexpr int kMinLeg = 3;
// From the button's edge to the mark: the theme's 1 px border and a pixel of
// air. Only the mark's right-angled tip reaches the border's 5 px rounded
// corner, where the border is the accent too while the tool runs.
constexpr int kInset = 2;
// The corner a press opens the menu from: as wide as the drop-down strip the
// mark replaced (a stylesheet's default ::menu-button is the base style's
// PM_MenuButtonIndicator, 12 px in QCommonStyle), so the menu is as easy to
// hit as it was.
constexpr int kZone = 12;

// The box, in the widget's own pixels, of the pixels where two grabs of it
// differ; null for none. A grab is in device pixels, which on a scaled
// screen are not the widget's, so the box is taken back to the widget's,
// rounded outwards.
QRect differenceBox(const QPixmap& a, const QPixmap& b)
{
    const QImage first = a.toImage().convertToFormat(QImage::Format_ARGB32);
    const QImage second = b.toImage().convertToFormat(QImage::Format_ARGB32);
    if (first.size() != second.size()) {
        return {};
    }
    int left = first.width();
    int top = first.height();
    int right = -1;
    int bottom = -1;
    for (int y = 0; y < first.height(); ++y) {
        const auto* one = reinterpret_cast<const QRgb*>(first.constScanLine(y));
        const auto* two = reinterpret_cast<const QRgb*>(second.constScanLine(y));
        for (int x = 0; x < first.width(); ++x) {
            if (one[x] != two[x]) {
                left = std::min(left, x);
                right = std::max(right, x);
                top = std::min(top, y);
                bottom = std::max(bottom, y);
            }
        }
    }
    if (right < 0) {
        return {};
    }
    const double scale = a.devicePixelRatio();
    const auto down = [scale](int device) { return static_cast<int>(std::floor(device / scale)); };
    const auto up = [scale](int device) {
        return static_cast<int>(std::ceil((device + 1) / scale)) - 1;
    };
    return {QPoint(down(left), down(top)), QPoint(up(right), up(bottom))};
}

QString rectText(const QRect& r)
{
    return QString("x %1..%2 y %3..%4").arg(r.left()).arg(r.right()).arg(r.top()).arg(r.bottom());
}

} // namespace

FlyoutButton::FlyoutButton(QWidget* parent) : QToolButton(parent)
{
    // Qt's own default, said here because it is the point: a click runs the
    // default action, a press held past SH_ToolButton_PopupDelay (600 ms in
    // QCommonStyle) opens the menu.
    setPopupMode(QToolButton::DelayedPopup);
    // Hover moves, to light the mark while the pointer is on its corner.
    // Fusion and the stylesheet ask for them too; this does not rely on it.
    setAttribute(Qt::WA_Hover);
}

QRect FlyoutButton::iconRect() const
{
    // The size the style is handed, which is the toolbar's icon size when the
    // button is on one (QToolButton::initStyleOption), not iconSize().
    QStyleOptionToolButton option;
    initStyleOption(&option);
    return QStyle::alignedRect(layoutDirection(), Qt::AlignCenter,
                               option.iconSize.boundedTo(size()), rect());
}

QRect FlyoutButton::markRect() const
{
    const QRect icon = iconRect();
    const int right = rect().right() - kInset;
    const int bottom = rect().bottom() - kInset;
    // As large as the corner allows, up to kMaxLeg. A style that left less
    // than kMinLeg would get the mark over the icon's corner; the theme
    // leaves kMaxLeg (tested at every toolbar icon size).
    const int room = std::min(right - icon.right(), bottom - icon.bottom());
    const int leg = std::clamp(room, kMinLeg, kMaxLeg);
    return {right - leg + 1, bottom - leg + 1, leg, leg};
}

QRegion FlyoutButton::menuZone() const
{
    const QRect corner(rect().right() - kZone + 1, rect().bottom() - kZone + 1, kZone, kZone);
    return QRegion(corner.intersected(rect())).subtracted(QRegion(iconRect()));
}

QString FlyoutButton::toolTipWithMenu() const
{
    const QMenu* family = menu();
    if (family == nullptr) {
        return toolTip();
    }
    QStringList others;
    for (const QAction* item : family->actions()) {
        if (!item->isSeparator() && item != defaultAction()) {
            others << QString(item->text()).remove('&').toHtmlEscaped();
        }
    }
    if (others.isEmpty()) {
        return toolTip();
    }
    return toolTip() + QString("<br><span style='color:%1'>Hold, right-click or press the "
                               "corner triangle for %2</span>")
                           .arg(theme::textMuted().name(), others.join(" / "));
}

bool FlyoutButton::event(QEvent* event)
{
    // QWidget's own tooltip handling with the fuller text: the toolTip
    // property stays the tool's, as setDefaultAction keeps setting it.
    if (event->type() == QEvent::ToolTip && menu() != nullptr && !toolTip().isEmpty()) {
        const auto* help = static_cast<QHelpEvent*>(event);
        QToolTip::showText(help->globalPos(), toolTipWithMenu(), this, QRect(), toolTipDuration());
        return true;
    }
    // The corner is a target of its own, so the mark says so under the
    // pointer, as a split button's strip lights up under it.
    if (event->type() == QEvent::HoverEnter || event->type() == QEvent::HoverMove ||
        event->type() == QEvent::HoverLeave) {
        const bool onMark =
            event->type() != QEvent::HoverLeave && menu() != nullptr &&
            menuZone().contains(static_cast<QHoverEvent*>(event)->position().toPoint());
        if (onMark != pointerOnMark_) {
            pointerOnMark_ = onMark;
            update();
        }
    }
    return QToolButton::event(event);
}

void FlyoutButton::paintEvent(QPaintEvent* /*event*/)
{
    QStylePainter painter(this);
    QStyleOptionToolButton option;
    initStyleOption(&option);
    // The style's own sign of a menu is left out: the stylesheet would draw
    // its ::menu-indicator arrow in the same corner, Fusion its arrow over
    // the icon. The mark below is the one sign.
    option.features &= ~QStyleOptionToolButton::HasMenu;
    painter.drawComplexControl(QStyle::CC_ToolButton, option);
    if (menu() == nullptr) {
        return;
    }
    // A right-angled triangle filling the mark's square on and below its
    // diagonal, the right angle in the corner: row by row, one pixel wider
    // each, so that it is the same pixels on every platform. A polygon fill
    // was not: without antialiasing Qt leaves out the pixels whose centres lie
    // on the diagonal, and a 5 px triangle came out 4 px; antialiased, at
    // five pixels its edge is a smudge.
    const QRect mark = markRect();
    // Quiet at rest, brighter under the pointer, the accent with the pointer
    // on it - a press there opens the family - and while the tool runs (the
    // checked frame is the accent too), and disabled with the button.
    QColor ink = theme::textMuted();
    if (!isEnabled()) {
        ink = theme::textDisabled();
    } else if (isChecked() || pointerOnMark_) {
        ink = theme::accent();
    } else if (underMouse() || isDown()) {
        ink = theme::text();
    }
    for (int row = 0; row < mark.height(); ++row) {
        painter.fillRect(QRect(mark.right() - row, mark.top() + row, row + 1, 1), ink);
    }
}

void FlyoutButton::mousePressEvent(QMouseEvent* event)
{
    // The mark's corner opens the menu at once, as the split button's strip
    // did; anywhere else is the tool's, the menu following only if the press
    // is held (DelayedPopup).
    if (event->button() == Qt::LeftButton && menu() != nullptr &&
        menuZone().contains(event->position().toPoint())) {
        event->accept();
        showMenu();
        return;
    }
    QToolButton::mousePressEvent(event);
}

void FlyoutButton::contextMenuEvent(QContextMenuEvent* event)
{
    // The family's menu, not the window's list of toolbars: a right click on
    // a tool group opens the group in drawing and graphics programs alike.
    if (menu() != nullptr) {
        event->accept();
        showMenu();
        return;
    }
    QToolButton::contextMenuEvent(event);
}

DrawnParts drawnParts(QToolButton& button)
{
    if (button.isHidden()) {
        button.resize(button.sizeHint());
    }
    // Large enough for any toolbar icon size (View > Toolbars offers up to
    // 28 px, setToolBarIconSize allows 48): the style scales it down to fit.
    QPixmap solid(64, 64);
    solid.fill(Qt::white);
    QPixmap clear(64, 64);
    clear.fill(Qt::transparent);
    const QIcon own = button.icon();
    button.setIcon(QIcon(solid));
    const QPixmap withSolid = button.grab();
    button.setIcon(QIcon(clear));
    const QPixmap withClear = button.grab();
    button.setIcon(own);

    QMenu* menu = button.menu();
    const QPixmap withMenu = button.grab();
    button.setMenu(nullptr);
    const QPixmap withoutMenu = button.grab();
    button.setMenu(menu);
    return {differenceBox(withSolid, withClear), differenceBox(withMenu, withoutMenu)};
}

QStringList menuSignClashes(QWidget& root, QStringList* checked, int* buttons)
{
    QStringList clashes;
    int measured = 0;
    for (QToolBar* bar : root.findChildren<QToolBar*>()) {
        for (QToolButton* button : bar->findChildren<QToolButton*>(Qt::FindDirectChildrenOnly)) {
            if (button->menu() == nullptr || button->objectName() == "qt_toolbar_ext_button" ||
                button->toolButtonStyle() != Qt::ToolButtonIconOnly || button->icon().isNull()) {
                continue;
            }
            ++measured;
            const QString where = bar->objectName() + " > " + button->objectName();
            const DrawnParts parts = drawnParts(*button);
            QString line;
            if (parts.sign.isNull()) {
                line = where + ": draws no sign of its menu";
                clashes << line;
            } else if (parts.icon.isNull()) {
                line = where + ": draws no icon";
                clashes << line;
            } else if (parts.sign.intersects(parts.icon)) {
                line = where + ": its menu's sign (" + rectText(parts.sign) +
                       ") is drawn over its icon (" + rectText(parts.icon) + ")";
                clashes << line;
            } else {
                line = where + ": sign " + rectText(parts.sign) + " clear of its icon " +
                       rectText(parts.icon);
            }
            if (checked != nullptr) {
                *checked << line;
            }
        }
    }
    if (buttons != nullptr) {
        *buttons = measured;
    }
    return clashes;
}

} // namespace katana::qt::tools
