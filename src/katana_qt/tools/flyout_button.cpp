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
// The corner a press opens the menu from, 12 px each way: the width of the
// drop-down strip the mark replaced (a stylesheet's default ::menu-button is
// the base style's PM_MenuButtonIndicator, 12 px in QCommonStyle). Less than
// the strip's area all the same - the icon keeps its whole square, so at
// 20 px icons the zone is an L of 119 square pixels against the strip's
// 12 x 33 = 396 - and the press held and the right click, anywhere on the
// button, make up for it.
constexpr int kZone = 12;
// A family names this many of its other tools in its tooltip, or one fewer
// and how many more: the Vertices family's seventeen others, all named, were
// a wall of six lines under the tool's own two, and the menu lists them.
constexpr qsizetype kNamedInTooltip = 4;

// The box, in device pixels, of the pixels where two grabs of a widget
// differ; null for none.
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
    return {QPoint(left, top), QPoint(right, bottom)};
}

// A box of device pixels taken back to the widget's pixels at `ratio`,
// rounded outwards: every widget pixel a device pixel of the box falls in.
QRect widgetPixels(const QRect& device, double ratio)
{
    if (device.isNull()) {
        return {};
    }
    const auto down = [ratio](int at) { return static_cast<int>(std::floor(at / ratio)); };
    const auto up = [ratio](int at) { return static_cast<int>(std::ceil((at + 1) / ratio)) - 1; };
    return {QPoint(down(device.left()), down(device.top())),
            QPoint(up(device.right()), up(device.bottom()))};
}

QString rectText(const QRect& r)
{
    return QString("x %1..%2 y %3..%4").arg(r.left()).arg(r.right()).arg(r.top()).arg(r.bottom());
}

} // namespace

FlyoutButton::FlyoutButton(QMenu& family, QWidget* parent) : QToolButton(parent), family_(&family)
{
    // Qt's own default, said here because it is the point: a click runs the
    // default action, a press held past SH_ToolButton_PopupDelay (600 ms in
    // QCommonStyle) opens the menu.
    setPopupMode(QToolButton::DelayedPopup);
    setMenu(&family);
    // Hover moves, to light the mark while the pointer is on its corner.
    // Fusion and the stylesheet ask for them too; this does not rely on it.
    setAttribute(Qt::WA_Hover);
    // A tool of the family starting or stopping checks or unchecks its
    // action, and the menu, holding the action, is told (ActionChanged);
    // the button is told only of its default action's.
    family.installEventFilter(this);
}

bool FlyoutButton::familyRunning() const
{
    if (isChecked()) {
        return true;
    }
    return family_ != nullptr && std::ranges::any_of(family_->actions(), [](const QAction* tool) {
               return tool->isChecked();
           });
}

bool FlyoutButton::eventFilter(QObject* watched, QEvent* event)
{
    if (watched == family_ &&
        (event->type() == QEvent::ActionChanged || event->type() == QEvent::ActionAdded ||
         event->type() == QEvent::ActionRemoved)) {
        update();
    }
    return QToolButton::eventFilter(watched, event);
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
    // Slashes between them, as the family's status tip has: a variant's
    // name may hold a comma.
    QString named = others.join(" / ");
    if (others.size() > kNamedInTooltip) {
        named = others.first(kNamedInTooltip - 1).join(" / ") +
                QString(" and %1 more").arg(others.size() - (kNamedInTooltip - 1));
    }
    // "for its other tools:" before the names, so that the triangle does not
    // read as the first of them ("the corner triangle for Delete Vertex").
    return toolTip() + QString("<br><span style='color:%1'>Hold, right-click or press the "
                               "corner triangle for its other tools: %2</span>")
                           .arg(theme::textMuted().name(), named);
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
    // pointer. (A stylesheet's split button cannot: its SC_ToolButton is the
    // whole button, so the strip is never the hovered part.)
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
    // Running while any of the family's tools runs, not only the one a click
    // starts: the button's checked state is its default action's, and with
    // Delete Vertex running no button on the toolbar was framed at all. The
    // state a checked button's option has (QToolButton::initStyleOption),
    // so the frame is the stylesheet's :checked one.
    const bool running = familyRunning();
    if (running) {
        option.state |= QStyle::State_On;
        option.state &= ~QStyle::State_Raised;
    }
    painter.drawComplexControl(QStyle::CC_ToolButton, option);
    if (menu() == nullptr) {
        return;
    }
    // Quiet at rest, brighter under the pointer, the accent with the pointer
    // on it - a press there opens the family - and while a tool of the
    // family runs (the checked frame is the accent too), and disabled with
    // the button.
    QColor ink = theme::textMuted();
    if (!isEnabled()) {
        ink = theme::textDisabled();
    } else if (running || pointerOnMark_) {
        ink = theme::accent();
    } else if (underMouse() || isDown()) {
        ink = theme::text();
    }
    // A right-angled triangle filling the mark's square on and below its
    // diagonal, the right angle in the corner: row by row, one pixel wider
    // each, so that it is the same pixels on every platform. A polygon fill
    // was not: without antialiasing Qt leaves out the pixels whose centres lie
    // on the diagonal, and a 5 px triangle came out 4 px; antialiased, at
    // five pixels its edge is a smudge. The rows are the DEVICE's: the
    // square is taken to the device and snapped to its pixels, and painted
    // there. Rows of the widget's pixels were whole at a ratio of 1 and 2,
    // but at 1.25 and 1.5 each became one device row or two and the
    // staircase came out uneven (rows 1, 2, 3, 3, 5, 6 at 125%).
    const QRectF onDevice = painter.deviceTransform().mapRect(QRectF(markRect()));
    const QRect square(QPoint(qRound(onDevice.left()), qRound(onDevice.top())),
                       QPoint(qRound(onDevice.right()) - 1, qRound(onDevice.bottom()) - 1));
    const int leg = std::min(square.width(), square.height());
    painter.save();
    // World coordinates that are device pixels: the inverse of what maps the
    // widget's pixels to the device (the screen's ratio, and the offset of
    // the button in the window it is painted into).
    painter.setWorldTransform(painter.deviceTransform().inverted() * painter.worldTransform());
    for (int row = 0; row < leg; ++row) {
        painter.fillRect(QRect(square.right() - row, square.bottom() - leg + 1 + row, row + 1, 1),
                         ink);
    }
    painter.restore();
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

    DrawnParts parts;
    parts.ratio = withMenu.devicePixelRatio();
    parts.iconDevice = differenceBox(withSolid, withClear);
    parts.signDevice = differenceBox(withMenu, withoutMenu);
    parts.icon = widgetPixels(parts.iconDevice, parts.ratio);
    parts.sign = widgetPixels(parts.signDevice, parts.ratio);
    return parts;
}

void followToolBarIconSize(QToolButton& button, QToolBar& bar)
{
    button.setIconSize(bar.iconSize());
    QObject::connect(&bar, &QToolBar::iconSizeChanged, &button, &QToolButton::setIconSize);
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
            // Judged, and so said, in device pixels on a scaled screen.
            const bool scaled = parts.ratio != 1.0;
            const QRect sign = scaled ? parts.signDevice : parts.sign;
            const QRect icon = scaled ? parts.iconDevice : parts.icon;
            const QString unit =
                scaled ? QString(" (device pixels at a ratio of %1)").arg(parts.ratio) : QString();
            QString line;
            if (sign.isNull()) {
                line = where + ": draws no sign of its menu";
                clashes << line;
            } else if (icon.isNull()) {
                line = where + ": draws no icon";
                clashes << line;
            } else if (sign.intersects(icon)) {
                line = where + ": its menu's sign (" + rectText(sign) + ") is drawn over its icon (" +
                       rectText(icon) + ")" + unit;
                clashes << line;
            } else {
                line = where + ": sign " + rectText(sign) + " clear of its icon " + rectText(icon) +
                       unit;
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
