#include "dock_chrome.hpp"

#include <algorithm>

#include <QAction>
#include <QDockWidget>
#include <QEvent>
#include <QGuiApplication>
#include <QHBoxLayout>
#include <QLabel>
#include <QMainWindow>
#include <QMouseEvent>
#include <QPainter>
#include <QScreen>
#include <QStyle>
#include <QTabBar>
#include <QToolBar>
#include <QToolButton>

#include "theme.hpp"

namespace katana::qt {

namespace {

// A title bar is one row of 22 px buttons with 2 px above and below: 26 px,
// the height of the old style-drawn bar (6 px padding round a 14 px line), so
// the drawing area did not shrink to make room for the new buttons.
constexpr int kButtonSize = 22;
constexpr int kBarPadding = 2;
// 14 px icons in 22 px buttons: the window buttons of the desktop the user
// is on (Windows 11 draws its caption glyphs at 10 to 16 px), and small enough
// that four of them and a view's tools fit a view a third of a screen wide.
constexpr int kIconSize = 14;
// The accent stroke across the top of the active view's bar. Two pixels is
// the line under the selected tab (theme.cpp), so the two read as one idea.
constexpr int kActiveStroke = 2;

QMainWindow* hostOf(const QDockWidget* dock)
{
    // A floating dock keeps its main window as its parent - that is what
    // re-docks it - so this is its host whether docked or not.
    return dock != nullptr ? qobject_cast<QMainWindow*>(dock->parentWidget()) : nullptr;
}

void setTip(QToolButton* button, const QString& label, const QString& tip)
{
    button->setToolTip(QString("<b>%1</b><br>%2").arg(label, tip));
    button->setAccessibleName(label);
    button->setAccessibleDescription(tip);
}

// A floating window put back on a screen that has since gone - a laptop
// undocked from its second monitor - would come back where nobody can see
// it. It goes to the middle of the primary screen instead, at its own size
// where that fits.
QRect onAScreen(QRect geometry)
{
    if (QGuiApplication::screenAt(geometry.center()) != nullptr) {
        return geometry;
    }
    const QScreen* primary = QGuiApplication::primaryScreen();
    if (primary == nullptr) {
        return geometry;
    }
    const QRect available = primary->availableGeometry();
    geometry.setSize(geometry.size().boundedTo(available.size()));
    geometry.moveCenter(available.center());
    return geometry;
}

} // namespace

// The title, elided to the room the bar leaves it and drawn in the colour of
// the bar's state. A QLabel would take the global stylesheet's text colour and
// grow the bar to fit a long title rather than shortening the title.
class ElidedTitle final : public QWidget {
  public:
    explicit ElidedTitle(QWidget* parent) : QWidget(parent)
    {
        setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
        setMinimumWidth(24);
    }
    void setText(const QString& text)
    {
        text_ = text;
        update();
    }
    void setColor(const QColor& color)
    {
        color_ = color;
        update();
    }
    [[nodiscard]] QString elided() const
    {
        return fontMetrics().elidedText(text_, Qt::ElideRight, width());
    }
    [[nodiscard]] QSize sizeHint() const override
    {
        return {fontMetrics().horizontalAdvance(text_), fontMetrics().height()};
    }

  protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter painter(this);
        painter.setPen(color_);
        painter.drawText(rect(), Qt::AlignLeft | Qt::AlignVCenter, elided());
    }

  private:
    QString text_;
    QColor color_ = theme::textMuted();
};

QToolButton* makeTitleBarButton(QWidget* parent, Icon which, const QString& name,
                                const QString& label, const QString& tip)
{
    auto* button = new QToolButton(parent);
    button->setObjectName(name);
    // theme.cpp styles every button carrying this property alike; a type
    // selector cannot, because the bar is not a class Qt's metatype knows.
    button->setProperty("chrome", "button");
    button->setIcon(icon(which));
    button->setIconSize(QSize(kIconSize, kIconSize));
    button->setFixedSize(kButtonSize, kButtonSize);
    button->setAutoRaise(true);
    // Reached with Tab like any other control, so a keyboard user can
    // minimise or close a panel; never takes focus from a click, which would
    // take it away from the view the user is drawing in.
    button->setFocusPolicy(Qt::TabFocus);
    setTip(button, label, tip);
    return button;
}

// ---- DockTitleBar -------------------------------------------------------------------------

DockTitleBar::DockTitleBar(QDockWidget& dock, DockChrome& chrome, Icon which, DockRole role)
    : QWidget(&dock), dock_(dock), chrome_(&chrome), role_(role), icon_(which)
{
    setObjectName("DockTitleBar");
    layout_ = new QHBoxLayout(this);
    layout_->setContentsMargins(6, kBarPadding, 3, kBarPadding);
    layout_->setSpacing(1);

    auto* iconLabel = new QLabel(this);
    iconLabel->setFixedSize(18, kButtonSize);
    iconLabel->setAlignment(Qt::AlignCenter);
    iconLabel_ = iconLabel;
    title_ = new ElidedTitle(this);
    tools_ = new QHBoxLayout();
    tools_->setSpacing(1);
    tools_->setContentsMargins(0, 0, 0, 0);

    layout_->addWidget(iconLabel_);
    layout_->addSpacing(4);
    layout_->addWidget(title_, 1);
    layout_->addLayout(tools_);
    layout_->addSpacing(6);

    const bool view = role == DockRole::View;
    const QString what = view ? "view" : "panel";
    minimise_ = makeTitleBarButton(
        this, Icon::Minimise, "DockMinimiseButton", "Minimise",
        QString("Hide this %1 to the bar at the bottom of the window; its button there "
                "puts it back where it was.")
            .arg(what));
    float_ = makeTitleBarButton(this, Icon::Float, "DockFloatButton", "Float", {});
    if (view) {
        maximise_ = makeTitleBarButton(this, Icon::Maximise, "DockMaximiseButton", "Maximise", {});
    }
    close_ = makeTitleBarButton(this, Icon::Close, "DockCloseButton", "Close",
                                view ? QString("Close this view. Its zoom and the layers it "
                                               "hides go with it; the drawing is not changed.")
                                     : QString("Hide this panel. View > Panels brings it back."));
    // Red on hover, as every desktop marks the one window button that loses
    // something.
    close_->setProperty("chrome", "close");
    for (QToolButton* button : {minimise_, float_, maximise_, close_}) {
        if (button != nullptr) {
            layout_->addWidget(button);
        }
    }

    connect(minimise_, &QToolButton::clicked, this, [this] {
        if (chrome_ != nullptr) {
            chrome_->minimise(&dock_);
        }
    });
    connect(float_, &QToolButton::clicked, this, [this] {
        if (chrome_ != nullptr) {
            chrome_->toggleFloating(&dock_);
        }
    });
    if (maximise_ != nullptr) {
        connect(maximise_, &QToolButton::clicked, this, [this] {
            if (chrome_ != nullptr) {
                chrome_->toggleMaximised(&dock_);
            }
        });
    }
    // close(), not hide(): a view's dock turns its close event into closing
    // the view (ViewDock::closeEvent), a panel's hides it, and the button
    // must mean whatever closing that dock means.
    connect(close_, &QToolButton::clicked, this, [this] { dock_.close(); });
    connect(&dock_, &QDockWidget::topLevelChanged, this, [this] { refresh(); });
    connect(&dock_, &QDockWidget::featuresChanged, this, [this] { refresh(); });
    connect(&dock_, &QWidget::windowTitleChanged, this, [this] { updateTitle(); });

    setIcon(which);
    refresh();
}

void DockTitleBar::setIcon(Icon which)
{
    icon_ = which;
    if (auto* label = qobject_cast<QLabel*>(iconLabel_)) {
        label->setPixmap(katana::qt::icon(which).pixmap(QSize(16, 16), devicePixelRatioF()));
    }
    if (chrome_ != nullptr) {
        chrome_->refreshTray(&dock_);
    }
}

void DockTitleBar::setLeadingWidget(QWidget* widget)
{
    if (widget == nullptr) {
        return;
    }
    QWidget* old = leading_ != nullptr ? leading_ : iconLabel_;
    layout_->replaceWidget(old, widget);
    old->hide();
    widget->setParent(this);
    widget->show();
    leading_ = widget;
}

void DockTitleBar::addTool(QWidget* widget)
{
    if (widget == nullptr) {
        return;
    }
    widget->setParent(this);
    tools_->addWidget(widget);
    widget->show();
}

void DockTitleBar::setActive(bool active)
{
    if (active_ == active) {
        return;
    }
    active_ = active;
    updateTitle();
    update();
}

void DockTitleBar::refresh()
{
    const bool floating = dock_.isFloating();
    const QDockWidget::DockWidgetFeatures features = dock_.features();
    const QString what = role_ == DockRole::View ? "view" : "panel";

    float_->setVisible(features.testFlag(QDockWidget::DockWidgetFloatable));
    float_->setIcon(katana::qt::icon(floating ? Icon::Dock : Icon::Float));
    if (floating) {
        setTip(float_, "Dock",
               QString("Put this %1 back where it was docked in the window. Double-clicking "
                       "the title bar does the same.")
                   .arg(what));
    } else {
        setTip(float_, "Float",
               QString("Take this %1 out into a window of its own, which can go on another "
                       "screen. Double-clicking the title bar does the same; drag the title "
                       "bar back over the window to dock it anywhere.")
                   .arg(what));
    }
    close_->setVisible(features.testFlag(QDockWidget::DockWidgetClosable));

    if (maximise_ != nullptr) {
        const bool maximised = chrome_ != nullptr && chrome_->isMaximised(&dock_);
        maximise_->setIcon(katana::qt::icon(maximised ? Icon::Restore : Icon::Maximise));
        if (maximised) {
            setTip(maximise_, "Restore",
                   floating ? QString("Put this window back to the size and place it had.")
                            : QString("Bring back the other views, at the sizes they had."));
        } else {
            setTip(maximise_, "Maximise",
                   floating ? QString("Fill the screen this window is on.")
                            : QString("Give this view the whole drawing area, hiding the other "
                                      "docked views until Restore."));
        }
    }
    updateTitle();
    update();
}

void DockTitleBar::updateTitle()
{
    title_->setText(dock_.windowTitle());
    // Panels are always readable; views are muted unless active, so the one
    // the menus act on stands out without the others fading away.
    const bool bright = role_ == DockRole::Panel || active_;
    title_->setColor(bright ? theme::text() : theme::textMuted());
    title_->setToolTip(dock_.windowTitle());
}

void DockTitleBar::paintEvent(QPaintEvent*)
{
    QPainter painter(this);
    const bool marked = role_ == DockRole::View && active_;
    painter.fillRect(rect(), marked ? theme::panel() : theme::window());
    painter.fillRect(QRect(0, height() - 1, width(), 1), theme::border());
    if (marked) {
        painter.fillRect(QRect(0, 0, width(), kActiveStroke), theme::accent());
    }
}

void DockTitleBar::resizeEvent(QResizeEvent* event)
{
    QWidget::resizeEvent(event);
    title_->update();
}

void DockTitleBar::mousePressEvent(QMouseEvent* event)
{
    if (event->button() == Qt::LeftButton && onPressed) {
        onPressed();
    }
    event->ignore();
}

void DockTitleBar::mouseMoveEvent(QMouseEvent* event) { event->ignore(); }

void DockTitleBar::mouseReleaseEvent(QMouseEvent* event) { event->ignore(); }

void DockTitleBar::mouseDoubleClickEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton || chrome_ == nullptr ||
        !dock_.features().testFlag(QDockWidget::DockWidgetFloatable)) {
        event->ignore();
        return;
    }
    chrome_->toggleFloating(&dock_);
    event->accept();
}

// ---- DockChrome ---------------------------------------------------------------------------

DockChrome::DockChrome(QMainWindow& window) : QObject(&window)
{
    setObjectName("DockChrome");
    tray_ = new QToolBar("Minimised", &window);
    tray_->setObjectName("MinimisedToolBar");
    tray_->setMovable(false);
    tray_->setFloatable(false);
    tray_->setIconSize(QSize(16, 16));
    tray_->setToolButtonStyle(Qt::ToolButtonTextBesideIcon);
    // Left out of the main window's right-click list of toolbars: the tray
    // shows itself when there is something on it, and a switch that hid it
    // would only hide the way back to a minimised panel.
    tray_->toggleViewAction()->setVisible(false);
    auto* label = new QLabel("Minimised:", tray_);
    label->setObjectName("MinimisedLabel");
    tray_->addWidget(label);
    window.addToolBar(Qt::BottomToolBarArea, tray_);
    tray_->hide();
}

QToolBar* DockChrome::tray() const { return tray_; }

const DockChrome::Installed* DockChrome::installed(const QDockWidget* dock) const
{
    for (const Installed& entry : installed_) {
        if (entry.dock == dock && dock != nullptr) {
            return &entry;
        }
    }
    return nullptr;
}

DockTitleBar* DockChrome::install(QDockWidget* dock, Icon which, DockRole role)
{
    if (dock == nullptr) {
        return nullptr;
    }
    if (const Installed* entry = installed(dock)) {
        return entry->bar;
    }
    auto* bar = new DockTitleBar(*dock, *this, which, role);
    dock->setTitleBarWidget(bar);
    installed_.push_back({dock, bar, role});
    dock->installEventFilter(this);

    // Destroyed with its window, or closed as a view whose workspace forgot
    // it first: either way only the records go.
    connect(dock, &QObject::destroyed, this, [this] { prune(); });
    connect(dock, &QWidget::windowTitleChanged, this, [this, dock] { refreshTray(dock); });
    connect(dock, &QDockWidget::topLevelChanged, this, [this, dock](bool floating) {
        // Docked by a drag rather than the Dock button: the sizes recorded
        // when it floated belong to a place it may not have gone back to.
        if (!floating) {
            std::erase_if(floated_, [&](const Floated& entry) { return entry.dock == dock; });
        }
        // A maximise is of a docked view or a floating one; crossing between
        // the two ends it. The views a docked maximise hid come back; a
        // floating view docked while it filled the screen keeps nothing of
        // that geometry, because it is docked.
        const auto at = std::ranges::find_if(
            maximised_, [&](const Maximised& entry) { return entry.dock == dock; });
        if (at != maximised_.end() && at->floating != floating) {
            if (at->floating) {
                maximised_.erase(at);
                refreshBar(dock);
                notify();
            } else {
                (void)unmaximise(dock);
            }
        }
    });
    return bar;
}

DockTitleBar* DockChrome::titleBar(const QDockWidget* dock) const
{
    const Installed* entry = installed(dock);
    return entry != nullptr ? entry->bar : nullptr;
}

// ---- sizes --------------------------------------------------------------------------------

std::vector<DockChrome::DockSize> DockChrome::snapshot(const QMainWindow& host)
{
    std::vector<DockSize> sizes;
    const QList<QTabBar*> tabBars = host.findChildren<QTabBar*>(Qt::FindDirectChildrenOnly);
    for (QDockWidget* dock : host.findChildren<QDockWidget*>(Qt::FindDirectChildrenOnly)) {
        if (dock->isFloating() || dock->isHidden()) {
            continue;
        }
        QRect extent = dock->geometry();
        // A tab page that is not the current one is moved off the window
        // rather than hidden; its geometry says nothing, and the current page
        // stands for the group.
        if (!host.rect().intersects(extent)) {
            continue;
        }
        // A tab group is bigger than its current page by its tab bar, and
        // resizeDocks sizes the group from the size it is given for the page.
        if (!host.tabifiedDockWidgets(dock).isEmpty()) {
            for (const QTabBar* bar : tabBars) {
                if (bar->isVisible() && extent.adjusted(-2, -2, 2, 2).intersects(bar->geometry())) {
                    extent = extent.united(bar->geometry());
                }
            }
        }
        sizes.push_back({dock, extent.size()});
    }
    return sizes;
}

void DockChrome::applySizes(QMainWindow& host, const std::vector<DockSize>& sizes)
{
    // Every dock at once and in both directions. resizeDocks sets the size of
    // each dock's place in the layout, and when the places of one row add up
    // to more than the row has, Qt takes the same number of pixels off each
    // of them - so a row where only the returning dock's size is set comes
    // out unequal. Setting all of them to what they were puts the row back.
    QList<QDockWidget*> docks;
    QList<int> widths;
    QList<int> heights;
    for (const DockSize& entry : sizes) {
        QDockWidget* dock = entry.dock;
        if (dock == nullptr || dock->isFloating() || dock->isHidden() || hostOf(dock) != &host) {
            continue;
        }
        docks.push_back(dock);
        widths.push_back(std::max(1, entry.size.width()));
        heights.push_back(std::max(1, entry.size.height()));
    }
    if (docks.isEmpty()) {
        return;
    }
    host.resizeDocks(docks, widths, Qt::Horizontal);
    host.resizeDocks(docks, heights, Qt::Vertical);
}

// ---- minimise -----------------------------------------------------------------------------

void DockChrome::minimise(QDockWidget* dock)
{
    const Installed* entry = installed(dock);
    if (entry == nullptr || isMinimised(dock)) {
        return;
    }
    (void)unmaximise(dock);

    Minimised record;
    record.dock = dock;
    record.floating = dock->isFloating();
    record.geometry = dock->geometry();
    if (!record.floating) {
        if (const QMainWindow* host = hostOf(dock)) {
            record.sizes = snapshot(*host);
        }
        // Already hidden (a restored layout): not in the snapshot, but Qt
        // keeps a hidden widget's last size, which is the one it had there.
        if (dock->isHidden()) {
            record.sizes.push_back({dock, dock->size()});
        }
    }
    auto* action = new QAction(icon(entry->bar->icon()), dock->windowTitle(), tray_);
    action->setObjectName("Restore" + dock->objectName());
    action->setToolTip(QString("<b>%1</b><br>Put it back where it was.").arg(dock->windowTitle()));
    connect(action, &QAction::triggered, this, [this, target = QPointer<QDockWidget>(dock)] {
        if (target != nullptr) {
            (void)restore(target);
        }
    });
    tray_->addAction(action);
    record.action = action;
    minimised_.push_back(std::move(record));

    dock->hide();
    tray_->show();
    notify();
}

bool DockChrome::restore(QDockWidget* dock)
{
    const auto at = std::ranges::find_if(
        minimised_, [&](const Minimised& entry) { return entry.dock == dock && dock != nullptr; });
    if (at == minimised_.end()) {
        return false;
    }
    // Out of the list BEFORE the dock is shown: the Show event filter drops a
    // minimised dock that something else showed, and must not find this one.
    Minimised record = std::move(*at);
    minimised_.erase(at);
    tray_->removeAction(record.action);
    delete record.action;
    if (minimised_.empty()) {
        tray_->hide();
    }

    if (dock->isFloating()) {
        dock->setGeometry(onAScreen(record.geometry));
        dock->show();
        dock->raise();
        dock->activateWindow();
    } else {
        dock->show();
        // For a tab page, raise() is what makes it the current tab again.
        dock->raise();
        if (QMainWindow* host = hostOf(dock)) {
            applySizes(*host, record.sizes);
        }
    }
    notify();
    return true;
}

bool DockChrome::isMinimised(const QDockWidget* dock) const
{
    return dock != nullptr && std::ranges::any_of(minimised_, [&](const Minimised& entry) {
               return entry.dock == dock;
           });
}

std::vector<QDockWidget*> DockChrome::minimisedDocks() const
{
    std::vector<QDockWidget*> out;
    for (const Minimised& entry : minimised_) {
        if (entry.dock != nullptr) {
            out.push_back(entry.dock);
        }
    }
    return out;
}

QToolButton* DockChrome::trayButton(const QDockWidget* dock) const
{
    for (const Minimised& entry : minimised_) {
        if (entry.dock == dock && dock != nullptr) {
            return qobject_cast<QToolButton*>(tray_->widgetForAction(entry.action));
        }
    }
    return nullptr;
}

QStringList DockChrome::minimisedNames() const
{
    QStringList names;
    for (const QDockWidget* dock : minimisedDocks()) {
        names << dock->objectName();
    }
    return names;
}

QStringList DockChrome::minimiseNamed(const QStringList& names)
{
    QStringList missing;
    for (const QString& name : names) {
        const auto at = std::ranges::find_if(installed_, [&](const Installed& entry) {
            return entry.dock != nullptr && entry.dock->objectName() == name;
        });
        if (at == installed_.end()) {
            missing << name;
            continue;
        }
        minimise(at->dock);
    }
    return missing;
}

void DockChrome::refreshTray(const QDockWidget* dock)
{
    const Installed* entry = installed(dock);
    for (const Minimised& record : minimised_) {
        if (record.dock == dock && entry != nullptr) {
            record.action->setText(dock->windowTitle());
            record.action->setIcon(icon(entry->bar->icon()));
        }
    }
}

// ---- float --------------------------------------------------------------------------------

void DockChrome::toggleFloating(QDockWidget* dock)
{
    if (dock == nullptr || !dock->features().testFlag(QDockWidget::DockWidgetFloatable)) {
        return;
    }
    (void)unmaximise(dock);
    QMainWindow* host = hostOf(dock);
    if (!dock->isFloating()) {
        std::erase_if(floated_, [&](const Floated& entry) { return entry.dock == dock; });
        Floated record;
        record.dock = dock;
        if (host != nullptr) {
            record.sizes = snapshot(*host);
        }
        floated_.push_back(std::move(record));
        dock->setFloating(true);
        dock->raise();
        dock->activateWindow();
        return;
    }
    std::vector<DockSize> sizes;
    const auto at = std::ranges::find_if(floated_,
                                         [&](const Floated& entry) { return entry.dock == dock; });
    if (at != floated_.end()) {
        sizes = std::move(at->sizes);
        floated_.erase(at);
    }
    dock->setFloating(false);
    if (host != nullptr) {
        applySizes(*host, sizes);
    }
}

// ---- maximise -----------------------------------------------------------------------------

void DockChrome::maximise(QDockWidget* dock)
{
    const Installed* entry = installed(dock);
    if (entry == nullptr || entry->role != DockRole::View || isMaximised(dock) ||
        dock->isHidden()) {
        return;
    }
    Maximised record;
    record.dock = dock;
    record.floating = dock->isFloating();
    if (record.floating) {
        record.geometry = dock->geometry();
        maximised_.push_back(std::move(record));
        if (const QScreen* screen = dock->screen()) {
            dock->setGeometry(screen->availableGeometry());
        }
        dock->raise();
    } else {
        QMainWindow* host = hostOf(dock);
        if (host == nullptr) {
            return;
        }
        unmaximiseDocked(*host);
        record.sizes = snapshot(*host);
        for (const Installed& other : installed_) {
            if (other.dock != nullptr && other.dock != dock && other.role == DockRole::View &&
                hostOf(other.dock) == host && !other.dock->isFloating() &&
                !other.dock->isHidden()) {
                record.hidden.push_back(other.dock);
            }
        }
        const std::vector<QPointer<QDockWidget>> hide = record.hidden;
        maximised_.push_back(std::move(record));
        for (QDockWidget* other : hide) {
            other->hide();
        }
        dock->raise();
    }
    refreshBar(dock);
    notify();
}

bool DockChrome::unmaximise(QDockWidget* dock)
{
    const auto at = std::ranges::find_if(
        maximised_, [&](const Maximised& entry) { return entry.dock == dock && dock != nullptr; });
    if (at == maximised_.end()) {
        return false;
    }
    // Out of the list before anything is shown, for the reason restore()
    // gives: the Show filter ends a maximise when a view it hid is shown.
    Maximised record = std::move(*at);
    maximised_.erase(at);
    if (record.floating) {
        if (dock->isFloating()) {
            dock->setGeometry(onAScreen(record.geometry));
        }
    } else {
        for (QDockWidget* other : record.hidden) {
            if (other != nullptr && !isMinimised(other)) {
                other->show();
            }
        }
        if (QMainWindow* host = hostOf(dock)) {
            applySizes(*host, record.sizes);
        }
    }
    refreshBar(dock);
    notify();
    return true;
}

void DockChrome::toggleMaximised(QDockWidget* dock)
{
    if (!unmaximise(dock)) {
        maximise(dock);
    }
}

void DockChrome::unmaximiseDocked(const QMainWindow& host)
{
    std::vector<QPointer<QDockWidget>> docked;
    for (const Maximised& entry : maximised_) {
        if (!entry.floating && entry.dock != nullptr && hostOf(entry.dock) == &host) {
            docked.push_back(entry.dock);
        }
    }
    for (QDockWidget* dock : docked) {
        if (dock != nullptr) {
            (void)unmaximise(dock);
        }
    }
}

void DockChrome::unmaximiseAll()
{
    std::vector<QPointer<QDockWidget>> all;
    for (const Maximised& entry : maximised_) {
        all.push_back(entry.dock);
    }
    for (QDockWidget* dock : all) {
        if (dock != nullptr) {
            (void)unmaximise(dock);
        }
    }
}

bool DockChrome::isMaximised(const QDockWidget* dock) const
{
    return dock != nullptr && std::ranges::any_of(maximised_, [&](const Maximised& entry) {
               return entry.dock == dock;
           });
}

// ---- lifetime -----------------------------------------------------------------------------

void DockChrome::forget(QDockWidget* dock)
{
    if (dock == nullptr) {
        return;
    }
    const auto minimised = std::ranges::find_if(
        minimised_, [&](const Minimised& entry) { return entry.dock == dock; });
    if (minimised != minimised_.end()) {
        tray_->removeAction(minimised->action);
        delete minimised->action;
        minimised_.erase(minimised);
        if (minimised_.empty()) {
            tray_->hide();
        }
    }
    (void)unmaximise(dock);
    for (Maximised& entry : maximised_) {
        std::erase_if(entry.hidden, [&](const QPointer<QDockWidget>& hidden) {
            return hidden == dock;
        });
    }
    std::erase_if(floated_, [&](const Floated& entry) { return entry.dock == dock; });
    notify();
}

void DockChrome::prune()
{
    // Called from a dock's destroyed(), when its QPointer is already null: the
    // records with null docks are exactly the ones to drop. Nothing is shown
    // or resized - a dock destroyed without forget() goes with its window.
    std::erase_if(installed_, [](const Installed& entry) { return entry.dock == nullptr; });
    for (auto at = minimised_.begin(); at != minimised_.end();) {
        if (at->dock == nullptr) {
            if (tray_ != nullptr) {
                tray_->removeAction(at->action);
            }
            delete at->action;
            at = minimised_.erase(at);
        } else {
            ++at;
        }
    }
    std::erase_if(maximised_, [](const Maximised& entry) { return entry.dock == nullptr; });
    for (Maximised& entry : maximised_) {
        std::erase_if(entry.hidden,
                      [](const QPointer<QDockWidget>& hidden) { return hidden == nullptr; });
    }
    std::erase_if(floated_, [](const Floated& entry) { return entry.dock == nullptr; });
    if (minimised_.empty() && tray_ != nullptr) {
        tray_->hide();
    }
}

bool DockChrome::eventFilter(QObject* watched, QEvent* event)
{
    auto* dock = qobject_cast<QDockWidget*>(watched);
    if (dock == nullptr) {
        return false;
    }
    if (event->type() == QEvent::Paint) {
        // A floating dock has no native frame (it has our title bar instead)
        // and QDockWidget paints nothing of its own once it has a title bar
        // widget, so without this its edge vanished against a dark desktop.
        // Painted here, before the dock's own (empty) paintEvent, into the
        // frame theme.cpp sizes - the band that resizes the window.
        if (dock->isFloating()) {
            QPainter painter(dock);
            painter.fillRect(dock->rect(), theme::window());
            painter.setPen(theme::border());
            painter.drawRect(dock->rect().adjusted(0, 0, -1, -1));
        }
        return false;
    }
    if (event->type() != QEvent::Show) {
        return false;
    }
    // Shown by something other than its tray button - View > Panels, an
    // arrangement of the views, a restored layout: it is not minimised any
    // more, and a tray button left behind would show it a second time.
    const auto minimised = std::ranges::find_if(
        minimised_, [&](const Minimised& entry) { return entry.dock == dock; });
    if (minimised != minimised_.end()) {
        tray_->removeAction(minimised->action);
        delete minimised->action;
        minimised_.erase(minimised);
        if (minimised_.empty()) {
            tray_->hide();
        }
        notify();
    }
    // A view a maximise hid, shown by something else - the View menu raising
    // the 3D view: the maximise is over.
    QPointer<QDockWidget> ended;
    for (const Maximised& entry : maximised_) {
        if (std::ranges::any_of(entry.hidden,
                                [&](const QPointer<QDockWidget>& hidden) { return hidden == dock; })) {
            ended = entry.dock;
        }
    }
    if (ended != nullptr) {
        (void)unmaximise(ended);
    }
    return false;
}

void DockChrome::refreshBar(const QDockWidget* dock) const
{
    if (DockTitleBar* bar = titleBar(dock)) {
        bar->refresh();
    }
}

void DockChrome::notify() const
{
    if (onChanged) {
        onChanged();
    }
}

} // namespace katana::qt
