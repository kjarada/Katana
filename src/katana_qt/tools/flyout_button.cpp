#include "tools/flyout_button.hpp"

#include <algorithm>
#include <cmath>

#include <QAction>
#include <QContextMenuEvent>
#include <QCoreApplication>
#include <QDockWidget>
#include <QHelpEvent>
#include <QHoverEvent>
#include <QIcon>
#include <QImage>
#include <QMenu>
#include <QMouseEvent>
#include <QPainter>
#include <QPixmap>
#include <QStyle>
#include <QStatusTipEvent>
#include <QStyleOptionToolButton>
#include <QStylePainter>
#include <QToolBar>
#include <QToolTip>
#include <QTransform>

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
// corner, where the border is the accent too while the tool runs. On a
// scaled screen both are taken down to whole device pixels (paintEvent).
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
// Five, so Circle's five are named rather than three "and 2 more", which was
// hardly shorter.
constexpr qsizetype kNamedInTooltip = 5;

// A tool's name as the tooltip lists it: its words held together by
// non-breaking spaces, so that the line wraps between names and never inside
// one ("3 / Points" was Circle's), and escaped for the tooltip's rich text.
QString listedName(const QAction& tool)
{
    return QString(tool.text()).remove('&').replace(QChar(' '), QChar(QChar::Nbsp)).toHtmlEscaped();
}

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

const QAction* FlyoutButton::runningTool() const
{
    if (family_ == nullptr) {
        return nullptr;
    }
    const auto tools = family_->actions();
    const auto running =
        std::ranges::find_if(tools, [](const QAction* tool) { return tool->isChecked(); });
    return running != tools.end() ? *running : nullptr;
}

bool FlyoutButton::familyRunning() const { return isChecked() || runningTool() != nullptr; }

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
            others << listedName(*item);
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
    QString tip = toolTip() + QString("<br><span style='color:%1'>Hold, right-click or press the "
                                      "corner triangle for its other tools: %2</span>")
                                  .arg(theme::textMuted().name(), named);
    // Another of them running lights the mark but frames nothing - the icon
    // is not its - so this is where it is named, in the accent the mark is.
    if (const QAction* running = runningTool(); running != nullptr && running != defaultAction()) {
        tip += QString("<br><span style='color:%1'>Running now: %2</span>")
                   .arg(theme::accent().name(), listedName(*running));
    }
    return tip;
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
    // pointer, and the status bar names the family there. (A stylesheet's
    // split button cannot: its SC_ToolButton is the whole button, so the
    // strip is never the hovered part.)
    if (event->type() == QEvent::HoverEnter || event->type() == QEvent::HoverMove ||
        event->type() == QEvent::HoverLeave) {
        const bool onMark =
            event->type() != QEvent::HoverLeave && menu() != nullptr &&
            menuZone().contains(static_cast<QHoverEvent*>(event)->position().toPoint());
        if (onMark != pointerOnMark_) {
            pointerOnMark_ = onMark;
            update();
            if (event->type() != QEvent::HoverLeave) {
                showStatusTip(onMark);
            }
        }
    }
    // Leave as well as HoverLeave: while a popup is open Qt sends the button
    // a Leave when the pointer goes, but no HoverLeave (QApplication's
    // dispatchEnterLeave sends one only with no popup up, or to the popup's
    // own widgets). The corner's press opens the family's menu, and the
    // pointer leaves over it to pick a tool: with only HoverLeave heard, the
    // mark stayed lit in the accent - the colour of a tool running - after
    // the menu closed and after the tool it started had ended.
    if (event->type() == QEvent::Leave && pointerOnMark_) {
        pointerOnMark_ = false;
        update();
    }
    return QToolButton::event(event);
}

void FlyoutButton::showStatusTip(bool onMark)
{
    const QString familyTip = family_ != nullptr ? family_->menuAction()->statusTip() : QString();
    QStatusTipEvent tip(onMark && !familyTip.isEmpty() ? familyTip : statusTip());
    QCoreApplication::sendEvent(this, &tip);
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
    // The frame is the button's own checked state, its default action's, as
    // on every tool button: it says the tool the icon shows runs. It was
    // once drawn while ANY of the family's tools ran, and with Delete Vertex
    // running the Vertices button framed Insert Vertex's icon - the wrong
    // tool, named by the one sign the toolbar has for "this is running".
    painter.drawComplexControl(QStyle::CC_ToolButton, option);
    if (menu() == nullptr) {
        return;
    }
    // Quiet at rest, brighter under the pointer, the accent with the pointer
    // on it - a press there opens the family - and while any tool of the
    // family runs, framed or not (the checked frame is the accent too), and
    // disabled with the button. So with Delete Vertex running the Vertices
    // button is unframed with its triangle lit: one of the tools behind the
    // triangle runs, and the tooltip names it.
    QColor ink = theme::textMuted();
    if (!isEnabled()) {
        ink = theme::textDisabled();
    } else if (familyRunning() || pointerOnMark_) {
        ink = theme::accent();
    } else if (underMouse() || isDown()) {
        ink = theme::text();
    }
    // A right-angled triangle filling the mark's square on and below its
    // diagonal, the right angle in the corner: row by row, one pixel wider
    // each, so that it is the same pixels on every platform. A polygon fill
    // was not: without antialiasing Qt leaves out the pixels whose centres lie
    // on the diagonal, and a 5 px triangle came out 4 px; antialiased, at
    // five pixels its edge is a smudge. The rows are the DEVICE's. Rows of
    // the widget's pixels were whole at a ratio of 1 and 2, but at 1.25 and
    // 1.5 each became one device row or two and the staircase came out
    // uneven (rows 1, 2, 3, 3, 5, 6 at 125%). The square is the mark's legs
    // and inset times the ratio, each taken down to whole device pixels and
    // set back from the button's last device pixel, so the triangle is the
    // same on every button: snapping the mark's own square to the device, as
    // was done first, made its size hang on where the button sat in the
    // window - at 150% Arc's came out 8 px beside the others' 7, running two
    // rows into the running frame's rounded corner. Taken down, not rounded,
    // since the mark's square starts the pixel after the icon's ends.
    const QTransform& toDevice = painter.deviceTransform();
    const double ratio = toDevice.m11();
    // Whole device pixels in `widgetPixels`, taken down; the slack keeps a
    // ratio a hair under 2 from losing a pixel of ten.
    const auto whole = [ratio](int widgetPixels) {
        return static_cast<int>(std::floor(widgetPixels * ratio + 1e-6));
    };
    const QRect mark = markRect();
    const int lastColumn = qRound(width() * ratio + toDevice.dx()) - 1;
    const int lastRow = qRound(height() * ratio + toDevice.dy()) - 1;
    const int inset = whole(rect().right() - mark.right());
    const int leg = std::max(1, whole(mark.width()));
    const QRect square(QPoint(lastColumn - inset - leg + 1, lastRow - inset - leg + 1),
                       QSize(leg, leg));
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

namespace {

// The toolbar's overflow arrow, lit while a button it hides shows a tool
// running. Worked out afresh whenever a tool starts or stops and whenever
// the toolbar's layout hides or shows a button (QToolBarLayout calls hide()
// and show() on the buttons that no longer fit or fit again), from what is
// hidden and running then - never kept as a count that could drift. When the
// toolbar is expanded its buttons are shown, their own frames and marks
// visible, and the arrow is not lit.
class OverflowLight final : public QObject {
  public:
    OverflowLight(QToolBar& bar, const QList<QAction*>& tools)
        : QObject(&bar), bar_(&bar), tools_(tools)
    {
    }

    void refresh() const
    {
        auto* overflow =
            bar_->findChild<QToolButton*>("qt_toolbar_ext_button", Qt::FindDirectChildrenOnly);
        if (overflow == nullptr) {
            return;
        }
        bool behind = false;
        for (QToolButton* button : bar_->findChildren<QToolButton*>(Qt::FindDirectChildrenOnly)) {
            if (button == overflow || !button->isHidden()) {
                continue;
            }
            // A tool's button, not any checked one: Select, which heads the
            // Draw toolbar, is checked while NO tool runs.
            const auto* family = dynamic_cast<const FlyoutButton*>(button);
            const bool tool = tools_.contains(button->defaultAction());
            if ((tool && button->isChecked()) || (family != nullptr && family->familyRunning())) {
                behind = true;
                break;
            }
        }
        if (overflow->property(theme::kToolRunsBehindOverflow).toBool() != behind) {
            overflow->setProperty(theme::kToolRunsBehindOverflow, behind);
            overflow->update();
        }
    }

  protected:
    bool eventFilter(QObject* watched, QEvent* event) override
    {
        // Hide and Show come only while the toolbar is on the screen, and its
        // layout also hides buttons before then, as the window is shown. The
        // overflow button is watched too, so its own Show - which comes after
        // that layout - works the light out before the arrow is seen.
        if (event->type() == QEvent::Show || event->type() == QEvent::Hide) {
            refresh();
        }
        return QObject::eventFilter(watched, event);
    }

  private:
    QToolBar* bar_;
    QList<QAction*> tools_;
};

} // namespace

void lightOverflowWhileAHiddenToolRuns(QToolBar& bar, const QList<QAction*>& tools)
{
    auto* light = new OverflowLight(bar, tools);
    for (QToolButton* button : bar.findChildren<QToolButton*>(Qt::FindDirectChildrenOnly)) {
        button->installEventFilter(light);
    }
    for (QAction* tool : tools) {
        QObject::connect(tool, &QAction::toggled, light, [light] { light->refresh(); });
    }
    light->refresh();
}

QStringList menuSignClashes(QWidget& root, QStringList* checked, int* buttons)
{
    QStringList clashes;
    int measured = 0;
    const auto measure = [&](const QString& holder, QWidget& row) {
        for (QToolButton* button : row.findChildren<QToolButton*>(Qt::FindDirectChildrenOnly)) {
            if (button->menu() == nullptr || button->objectName() == "qt_toolbar_ext_button" ||
                button->toolButtonStyle() != Qt::ToolButtonIconOnly || button->icon().isNull()) {
                continue;
            }
            ++measured;
            const QString where = holder + " > " + button->objectName();
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
    };
    for (QToolBar* bar : root.findChildren<QToolBar*>()) {
        measure("toolbar button " + bar->objectName(), *bar);
    }
    // A view's title bar opens with its kind switcher, a button with a menu
    // at the left of every view: its arrow sat on its icon's frame, 2 px at
    // 100% and 3 at 125%, where no check looked.
    for (QDockWidget* dock : root.findChildren<QDockWidget*>()) {
        if (QWidget* titleBar = dock->titleBarWidget()) {
            measure("title bar button " + dock->objectName(), *titleBar);
        }
    }
    if (buttons != nullptr) {
        *buttons = measured;
    }
    return clashes;
}

} // namespace katana::qt::tools
