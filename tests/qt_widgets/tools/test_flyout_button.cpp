// The signs of a menu on a toolbar button (src/katana_qt/tools/flyout_button):
// a tool family's corner triangle is drawn beside its icon, never on it, and
// a press on the icon runs the tool while the triangle, a press held and a
// right click open the family; Undo's and Redo's arrows have a strip of
// their own beside the icon (theme.cpp); the check the window runs headless
// (--check-toolbars) sees an arrow drawn over an icon; and a toolbar's
// overflow arrow is drawn whole.
//
// The toolbars are built as the window builds them - fillToolMenus for the
// families - with the theme's style and stylesheet on the window under test,
// not on the application, so the other widget tests keep the look they were
// written against (test_theme.cpp does the same). Geometry is asserted as
// rectangles; where pixels are compared it is a button against itself or
// against a plain button with the same icon, never against a count.

#include <gtest/gtest.h>

#include <array>
#include <cmath>
#include <cstdlib>
#include <memory>
#include <string>
#include <vector>

#include <QAction>
#include <QApplication>
#include <QContextMenuEvent>
#include <QElapsedTimer>
#include <QHelpEvent>
#include <QHoverEvent>
#include <QImage>
#include <QMainWindow>
#include <QMenu>
#include <QMouseEvent>
#include <QPainter>
#include <QStyle>
#include <QStyleOptionToolButton>
#include <QTimer>
#include <QToolBar>
#include <QToolButton>
#include <QToolTip>

#include "icons.hpp"
#include "katana/cad/interactive_tool.hpp"
#include "theme.hpp"
#include "tools/flyout_button.hpp"
#include "tools/tool_icons.hpp"
#include "tools/tool_menus.hpp"
#include "widget_harness.hpp"

namespace theme = katana::qt::theme;
using katana::cad::toolCatalog;
using katana::qt::tools::drawnParts;
using katana::qt::tools::fillToolMenus;
using katana::qt::tools::FlyoutButton;
using katana::qt::tools::menuSignClashes;
using katana::qt::tools::ToolActions;
using katana::qt::tools::ToolMenuTargets;

namespace {

// The families the catalogue has, all in the Draw category: every tool
// named "Circle, ...", "Arc, ...", "Ellipse, ..." or "Vertices, ...".
const std::vector<QString> kFamilies = {"Circle", "Arc", "Ellipse", "Vertices"};

// The box of the pixels where two images of one size differ; null for none.
QRect differenceBox(const QImage& a, const QImage& b)
{
    QRect box;
    if (a.size() != b.size()) {
        return box;
    }
    for (int y = 0; y < a.height(); ++y) {
        for (int x = 0; x < a.width(); ++x) {
            if (a.pixel(x, y) != b.pixel(x, y)) {
                box = box.united(QRect(x, y, 1, 1));
            }
        }
    }
    return box;
}

QImage shot(QWidget& widget) { return widget.grab().toImage().convertToFormat(QImage::Format_ARGB32); }

// A left press and release at `at`, as a click on the button.
void click(QWidget& widget, QPoint at)
{
    const QPointF local(at);
    const QPointF global(widget.mapToGlobal(at));
    QMouseEvent press(QEvent::MouseButtonPress, local, global, Qt::LeftButton, Qt::LeftButton,
                      Qt::NoModifier);
    QCoreApplication::sendEvent(&widget, &press);
    QMouseEvent release(QEvent::MouseButtonRelease, local, global, Qt::LeftButton,
                        Qt::NoButton, Qt::NoModifier);
    QCoreApplication::sendEvent(&widget, &release);
}

// Counts the times `menu` is shown, closing it each time as soon as its own
// event loop runs: QMenu::exec would otherwise wait for a person.
struct MenuWatch {
    int shown = 0;
    QMetaObject::Connection connection;

    explicit MenuWatch(QMenu& menu)
    {
        QMenu* watched = &menu;
        connection = QObject::connect(watched, &QMenu::aboutToShow, watched, [this, watched] {
            ++shown;
            QTimer::singleShot(0, watched, [watched] { watched->close(); });
        });
    }
    ~MenuWatch() { QObject::disconnect(connection); }
    MenuWatch(const MenuWatch&) = delete;
    MenuWatch& operator=(const MenuWatch&) = delete;
};

// A vertical Draw toolbar as the window makes it, the theme's look on its
// window, and beside the families a plain button showing the Circle family's
// icon: the baseline a family button must match everywhere but its mark.
struct DrawBar {
    // Before the window, so it outlives every widget drawn with it.
    std::unique_ptr<QStyle> style{theme::makeStyle()};
    QMainWindow window;
    QToolBar* bar = nullptr;
    ToolActions actions;
    std::vector<std::string> started;
    QToolButton* twin = nullptr;

    explicit DrawBar(int iconSize = 20)
    {
        window.setStyle(style.get());
        window.setStyleSheet(theme::styleSheet());
        // A right click no button takes would reach the window's list of
        // toolbars, a menu nobody here would close: refused, it fails the
        // test that sent it instead of hanging it.
        window.setContextMenuPolicy(Qt::NoContextMenu);
        window.setCentralWidget(new QWidget);
        bar = new QToolBar("Draw", &window);
        bar->setObjectName("DrawToolBar");
        bar->setIconSize(QSize(iconSize, iconSize));
        bar->setToolButtonStyle(Qt::ToolButtonIconOnly);
        window.addToolBar(Qt::LeftToolBarArea, bar);
        ToolMenuTargets targets;
        targets.menus = {{"Draw", new QMenu("Draw", &window)}};
        targets.toolBars = {{"Draw", bar}};
        actions = fillToolMenus(toolCatalog(), targets, &window,
                                [this](const std::string& id) { started.push_back(id); });
        auto* plain = new QAction(katana::qt::tools::toolIcon("draw.circle"), "Plain Circle", &window);
        plain->setCheckable(true);
        bar->addAction(plain);
        twin = qobject_cast<QToolButton*>(bar->widgetForAction(plain));
        // Tall enough for every button: none behind the overflow arrow.
        window.resize(480, 2400);
        window.show();
        katana::qt::test::processEvents();
    }

    FlyoutButton* family(const QString& name) const
    {
        return dynamic_cast<FlyoutButton*>(
            window.findChild<QToolButton*>("toolFamilyButton.Draw." + name));
    }
};

} // namespace

TEST(ToolFamilyButtons, EachFamilyIsAFlyoutButtonWhoseClickRunsItsFirstTool)
{
    DrawBar built;
    for (const QString& name : kFamilies) {
        FlyoutButton* button = built.family(name);
        ASSERT_NE(button, nullptr) << name.toStdString();
        auto* menu = built.window.findChild<QMenu*>("toolFamily.Draw." + name);
        ASSERT_NE(menu, nullptr) << name.toStdString();
        EXPECT_EQ(button->menu(), menu) << name.toStdString();
        ASSERT_FALSE(menu->actions().isEmpty());
        EXPECT_EQ(button->defaultAction(), menu->actions().front()) << name.toStdString();
        // Not a split button: that was the arrow strip drawn over the icon.
        EXPECT_EQ(button->popupMode(), QToolButton::DelayedPopup) << name.toStdString();
        EXPECT_TRUE(button->isVisible()) << name.toStdString();
    }
    EXPECT_EQ(built.family("Circle")->defaultAction()->objectName(), "draw.circle");
}

TEST(ToolFamilyButtons, TheCornerMarkAndItsPressZoneStayOffTheIconAtEveryToolbarIconSize)
{
    // View > Toolbars offers 16, 20 and 28 px icons. By hand, under the
    // theme a button is its icon, Qt's 3 px (QStyleSheetStyle, CT_ToolButton),
    // 4 px of padding and a 1 px border each side: 13 px more than its icon,
    // the icon's square centred 6 px in. The corner mark is inset 2 px from
    // the edge, so it has (s + 13 - 1 - 2) - (6 + s - 1) = 5 px beside and
    // below the icon's square at every size s.
    for (const int size : {16, 20, 28}) {
        DrawBar built(size);
        for (const QString& name : kFamilies) {
            const std::string where = name.toStdString() + " at " + std::to_string(size) + " px";
            FlyoutButton* button = built.family(name);
            ASSERT_NE(button, nullptr) << where;
            ASSERT_EQ(button->size(), QSize(size + 13, size + 13)) << where;
            const QRect icon = button->iconRect();
            const QRect mark = button->markRect();
            EXPECT_EQ(icon, QRect(6, 6, size, size)) << where;
            EXPECT_EQ(mark, QRect(size + 6, size + 6, 5, 5)) << where;
            EXPECT_FALSE(mark.intersects(icon)) << where;
            // Inside the 1 px border.
            EXPECT_TRUE(button->rect().adjusted(1, 1, -1, -1).contains(mark)) << where;
            // The press zone holds the whole mark and none of the icon.
            const QRegion zone = button->menuZone();
            EXPECT_TRUE(QRegion(mark).subtracted(zone).isEmpty()) << where;
            EXPECT_FALSE(zone.intersects(icon)) << where;

            // And the geometry is what is drawn: the style puts the icon in
            // iconRect, and what the menu adds is the mark - a triangle whose
            // right column and bottom row are whole, so its ink spans exactly
            // markRect.
            const auto parts = drawnParts(*button);
            EXPECT_EQ(parts.icon, icon) << where;
            ASSERT_FALSE(parts.sign.isNull()) << where << ": no mark drawn";
            EXPECT_EQ(parts.sign, mark) << where;
            EXPECT_FALSE(parts.sign.intersects(parts.icon)) << where;
            // Measured leaves the button as it was: its menu, and the icon
            // its tool gave it.
            EXPECT_EQ(button->menu(), built.window.findChild<QMenu*>("toolFamily.Draw." + name))
                << where;
            EXPECT_EQ(button->icon().cacheKey(), button->defaultAction()->icon().cacheKey())
                << where;
        }
    }
}

TEST(ToolFamilyButtons, AFamilyButtonLooksLikeAPlainButtonWithItsIconSaveForTheCornerMark)
{
    // Ink against a baseline: the Circle family beside a plain button with
    // the same icon, at rest, under the pointer, running its first tool and
    // running another of its tools - the plain button running in both. They
    // may differ only inside the mark's square. As a split button the family
    // differed over the icon's right third, where the arrow's strip was
    // painted on it; and with a tool other than its first running (2 Points,
    // or Delete Vertex in the Vertices family) it was not framed at all, so
    // no button on the toolbar said a tool was running.
    DrawBar built;
    FlyoutButton* circle = built.family("Circle");
    ASSERT_NE(circle, nullptr);
    ASSERT_NE(built.twin, nullptr);
    ASSERT_EQ(circle->size(), built.twin->size());
    QAction* first = circle->defaultAction();
    ASSERT_GE(circle->menu()->actions().size(), 2);
    QAction* other = circle->menu()->actions().at(1);
    ASSERT_NE(other, first);
    QAction* plainRunning = built.twin->defaultAction();
    ASSERT_NE(plainRunning, nullptr);
    const std::string another = "running another of its tools";
    for (const std::string state : {"at rest", "under the pointer", "running", another.c_str()}) {
        const bool hover = state == "under the pointer";
        const bool running = state == "running" || state == another;
        for (QToolButton* each : {static_cast<QToolButton*>(circle), built.twin}) {
            each->setAttribute(Qt::WA_UnderMouse, hover);
        }
        first->setChecked(state == "running");
        other->setChecked(state == another);
        plainRunning->setChecked(running);
        katana::qt::test::processEvents();
        EXPECT_EQ(circle->familyRunning(), running) << state;
        const QImage family = shot(*circle);
        const QRect differs = differenceBox(family, shot(*built.twin));
        EXPECT_FALSE(differs.isNull()) << state << ": the mark is not drawn";
        EXPECT_TRUE(circle->markRect().contains(differs))
            << state << ": the family differs from the plain button at x " << differs.left()
            << ".." << differs.right() << " y " << differs.top() << ".." << differs.bottom();
        // Running, the mark is the accent, as the frame is.
        if (running) {
            EXPECT_EQ(family.pixelColor(circle->markRect().bottomRight()), theme::accent()) << state;
        }
    }
    for (QToolButton* each : {static_cast<QToolButton*>(circle), built.twin}) {
        each->setAttribute(Qt::WA_UnderMouse, false);
    }
    other->setChecked(false);
    plainRunning->setChecked(false);
}

TEST(ToolFamilyButtons, TheButtonIsPaintedAgainWhenAnyOfItsToolsStartsOrStops)
{
    // A grab paints afresh, so the test above cannot see this: the screen
    // repaints only what is marked for it. A tool of the family started from
    // the command line checks its action and the menu holding it is told;
    // the button, told only of its own default action, must be told too, or
    // it stays unframed on the screen until the pointer passes over it.
    DrawBar built;
    FlyoutButton* circle = built.family("Circle");
    ASSERT_NE(circle, nullptr);
    QAction* other = circle->menu()->actions().at(1);
    ASSERT_NE(other, circle->defaultAction());
    struct PaintCount final : QObject {
        int painted = 0;
        bool eventFilter(QObject* /*watched*/, QEvent* event) override
        {
            if (event->type() == QEvent::Paint) {
                ++painted;
            }
            return false;
        }
    } count;
    circle->installEventFilter(&count);
    katana::qt::test::processEvents();
    count.painted = 0;
    other->setChecked(true);
    katana::qt::test::processEvents();
    EXPECT_GT(count.painted, 0) << "not painted again when its second tool started";
    count.painted = 0;
    other->setChecked(false);
    katana::qt::test::processEvents();
    EXPECT_GT(count.painted, 0) << "not painted again when its second tool stopped";
    circle->removeEventFilter(&count);
}

TEST(ToolFamilyButtons, AFamilyTakesItsToolbarsNewIconSizeAsTheToolbarsOwnButtonsDo)
{
    // View > Toolbars changes a bar's icon size after its buttons are made
    // (MainWindow::setToolBarIconSize calls QToolBar::setIconSize and nothing
    // more). The bar's own buttons follow it; a family, a widget on the bar,
    // must too. By hand, as in the test above that builds each size: a
    // button is its icon and 13 px, the icon's square at 6, the 5 px mark
    // at the icon's size and 6. Made at 20 px and switched, a family kept
    // its 33 x 33 - at 28 its icon was drawn at 23 px beside the others'
    // 28, its mark 3 px against the icon's corner.
    for (const int size : {16, 28}) {
        DrawBar built(20);
        built.bar->setIconSize(QSize(size, size));
        katana::qt::test::processEvents();
        ASSERT_EQ(built.twin->size(), QSize(size + 13, size + 13)) << "the bar's own button at " << size;
        for (const QString& name : kFamilies) {
            const std::string where = name.toStdString() + " switched to " + std::to_string(size) + " px";
            FlyoutButton* button = built.family(name);
            ASSERT_NE(button, nullptr) << where;
            EXPECT_EQ(button->size(), QSize(size + 13, size + 13)) << where;
            EXPECT_EQ(button->iconRect(), QRect(6, 6, size, size)) << where;
            EXPECT_EQ(button->markRect(), QRect(size + 6, size + 6, 5, 5)) << where;
            const auto parts = drawnParts(*button);
            EXPECT_EQ(parts.icon, QRect(6, 6, size, size)) << where;
            EXPECT_EQ(parts.sign, QRect(size + 6, size + 6, 5, 5)) << where;
        }
    }
}

TEST(ToolFamilyButtons, TheMarkIsAWholeStaircaseOfDevicePixelsAtEveryScreenScale)
{
    // At 125% and 150% - where Windows puts most laptop screens, and where
    // the owner's is (qt_render_view_at_125_percent) - a row of the widget's
    // pixels is one device row or two, and the triangle painted a widget row
    // at a time came out uneven: rows 1, 2, 3, 3, 5, 6 wide at 125%. Painted
    // a device row at a time it is whole at every ratio: the k-th row from
    // its top is k pixels ending at its right column, and its legs are the
    // 5 px mark at that ratio, give or take the rounding of its two edges to
    // the device's pixels. Rendered here at each ratio (as the screen's grab
    // at 125% renders it: the same rows, measured apart), and grabbed at the
    // screen's own, which qt_toolbar_signs_at_125_percent makes 125%.
    DrawBar built;
    FlyoutButton* circle = built.family("Circle");
    ASSERT_NE(circle, nullptr);
    QMenu* family = circle->menu();
    const double screen = circle->devicePixelRatio();
    for (const double ratio : {1.0, 1.25, 1.5, 2.0, 0.0}) {
        const auto render = [&] {
            if (ratio == 0.0) {
                return shot(*circle); // the screen's own grab
            }
            QImage image((QSizeF(circle->size()) * ratio).toSize(), QImage::Format_ARGB32);
            image.setDevicePixelRatio(ratio);
            image.fill(Qt::transparent);
            circle->render(&image);
            return image;
        };
        const QImage with = render();
        circle->setMenu(nullptr);
        const QImage without = render();
        circle->setMenu(family);
        // Each row of the mark: how many pixels, the first and last, where.
        std::vector<std::array<int, 4>> rows;
        for (int y = 0; y < with.height(); ++y) {
            int count = 0;
            int left = -1;
            int right = -1;
            for (int x = 0; x < with.width(); ++x) {
                if (with.pixel(x, y) != without.pixel(x, y)) {
                    ++count;
                    left = left < 0 ? x : left;
                    right = x;
                }
            }
            if (count > 0) {
                rows.push_back({count, left, right, y});
            }
        }
        const double at = ratio == 0.0 ? screen : ratio;
        const std::string where = (ratio == 0.0 ? "grabbed at the screen's ratio of "
                                                : "rendered at a ratio of ") +
                                  QString::number(at).toStdString();
        const int leg = static_cast<int>(rows.size());
        EXPECT_GE(leg, static_cast<int>(std::floor(5 * at))) << where;
        EXPECT_LE(leg, static_cast<int>(std::ceil(5 * at))) << where;
        for (int k = 0; k < leg; ++k) {
            const auto& [count, left, right, y] = rows[static_cast<std::size_t>(k)];
            EXPECT_EQ(count, k + 1) << where << ", row " << k;
            EXPECT_EQ(right - left + 1, count) << where << ", row " << k << " has a gap";
            EXPECT_EQ(right, rows.front()[2]) << where << ", row " << k << " off the right column";
            EXPECT_EQ(y, rows.front()[3] + k) << where << ", row " << k << " not below the last";
        }
    }
}

TEST(ToolFamilyButtons, AClickOnTheIconRunsTheToolWhileTheMarkAHoldOrARightClickOpensTheFamily)
{
    DrawBar built;
    FlyoutButton* circle = built.family("Circle");
    ASSERT_NE(circle, nullptr);
    QMenu* family = circle->menu();
    ASSERT_NE(family, nullptr);
    const MenuWatch watch(*family);
    const QPoint onIcon = circle->iconRect().center();
    // The icon's own bottom-right pixel: on the old split button this was
    // under the arrow's strip, and opened the menu.
    const QPoint iconCorner = circle->iconRect().bottomRight();

    click(*circle, onIcon);
    EXPECT_EQ(built.started, std::vector<std::string>{"draw.circle"});
    EXPECT_EQ(watch.shown, 0);

    click(*circle, iconCorner);
    EXPECT_EQ(built.started.size(), 2u) << "a click on the icon's corner runs the tool";
    EXPECT_EQ(watch.shown, 0);

    // The mark opens the family at once, and runs nothing.
    click(*circle, circle->markRect().center());
    EXPECT_EQ(watch.shown, 1);
    EXPECT_EQ(built.started.size(), 2u);

    // A right click opens it too, rather than the window's list of toolbars.
    QContextMenuEvent right(QContextMenuEvent::Mouse, onIcon, circle->mapToGlobal(onIcon));
    QCoreApplication::sendEvent(circle, &right);
    EXPECT_TRUE(right.isAccepted());
    EXPECT_EQ(watch.shown, 2);
    EXPECT_EQ(built.started.size(), 2u);

    // What --press does (QAbstractButton::click): the tool, no menu.
    circle->click();
    EXPECT_EQ(built.started.size(), 3u);
    EXPECT_EQ(watch.shown, 2);

    // A press held on the icon past the style's popup delay opens the
    // family, and letting go then runs nothing.
    const int delay = circle->style()->styleHint(QStyle::SH_ToolButton_PopupDelay, nullptr, circle);
    ASSERT_GT(delay, 0);
    const QPointF local(onIcon);
    const QPointF global(circle->mapToGlobal(onIcon));
    QMouseEvent press(QEvent::MouseButtonPress, local, global, Qt::LeftButton, Qt::LeftButton,
                      Qt::NoModifier);
    QCoreApplication::sendEvent(circle, &press);
    QElapsedTimer waited;
    waited.start();
    while (watch.shown < 3 && waited.elapsed() < delay + 3000) {
        QCoreApplication::processEvents(QEventLoop::AllEvents, 20);
    }
    EXPECT_EQ(watch.shown, 3) << "no menu after holding for " << waited.elapsed() << " ms";
    EXPECT_GE(waited.elapsed(), delay - 50) << "the menu came before the delay";
    QMouseEvent release(QEvent::MouseButtonRelease, local, global, Qt::LeftButton, Qt::NoButton,
                        Qt::NoModifier);
    QCoreApplication::sendEvent(circle, &release);
    EXPECT_EQ(built.started.size(), 3u) << "a held press that opened the menu also ran the tool";
}

TEST(ToolFamilyButtons, WithNoToolRunningTheMarkIsLitInTheAccentOnlyWhileThePointerIsOnItsCorner)
{
    // The corner opens the family where the rest of the button runs the
    // tool, so the mark says which the pointer is over. Its right-angled
    // corner pixel is inked by every row of the triangle, and is the mark's
    // colour exactly.
    DrawBar built;
    FlyoutButton* circle = built.family("Circle");
    ASSERT_NE(circle, nullptr);
    const QPoint corner = circle->markRect().bottomRight();
    const auto hover = [&](QEvent::Type type, QPoint at) {
        const QPointF local(at);
        QHoverEvent event(type, local, QPointF(circle->mapToGlobal(at)), local);
        QCoreApplication::sendEvent(circle, &event);
        katana::qt::test::processEvents();
        return shot(*circle).pixelColor(corner);
    };
    EXPECT_NE(hover(QEvent::HoverEnter, circle->iconRect().center()), theme::accent());
    EXPECT_FALSE(circle->pointerOnMark());
    EXPECT_EQ(hover(QEvent::HoverMove, circle->markRect().center()), theme::accent());
    EXPECT_TRUE(circle->pointerOnMark());
    // Off the mark but still in its corner, beside the icon: the press there
    // opens the family too, so the mark stays lit.
    const QPoint besideIcon(circle->iconRect().right() + 2, circle->markRect().top() - 3);
    ASSERT_TRUE(circle->menuZone().contains(besideIcon));
    EXPECT_EQ(hover(QEvent::HoverMove, besideIcon), theme::accent());
    EXPECT_NE(hover(QEvent::HoverMove, circle->iconRect().center()), theme::accent());
    EXPECT_FALSE(circle->pointerOnMark());
    EXPECT_EQ(hover(QEvent::HoverMove, circle->markRect().center()), theme::accent());
    EXPECT_NE(hover(QEvent::HoverLeave, QPoint(-1, -1)), theme::accent());
    EXPECT_FALSE(circle->pointerOnMark());
}

namespace {

// The tooltip a person sees: the event the pointer resting on the button
// sends, and what the tooltip then shows.
QString tooltipShown(QToolButton& button)
{
    const QPoint at = button.rect().center();
    QHelpEvent help(QEvent::ToolTip, at, button.mapToGlobal(at));
    QCoreApplication::sendEvent(&button, &help);
    const QString text = QToolTip::isVisible() ? QToolTip::text() : QString();
    QToolTip::hideText();
    return text;
}

// The family's line of the tooltip, what follows the tool's own.
QString familyLine(const QString& tip)
{
    const qsizetype from = tip.indexOf("<br><span");
    return from < 0 ? QString() : tip.mid(from);
}

} // namespace

TEST(ToolFamilyButtons, TheTooltipShownSaysHowToReachTheOtherToolsAndNamesFourAtMost)
{
    // By hand, on a family of letters: with four others each is named, in
    // the menu's order, slashes between; with five, three are and "and 2
    // more" stands for the rest. The tool the click runs is the tooltip's
    // own, never an other.
    const std::unique_ptr<QStyle> style(theme::makeStyle());
    QMainWindow window;
    window.setStyle(style.get());
    window.setStyleSheet(theme::styleSheet());
    auto* bar = new QToolBar("Draw", &window);
    window.addToolBar(Qt::LeftToolBarArea, bar);
    const auto familyOf = [&](const QString& letters) {
        auto* menu = new QMenu("Letters", &window);
        for (const QChar letter : letters) {
            menu->addAction(QString(letter));
        }
        auto* button = new FlyoutButton(*menu, bar);
        auto* own = menu->actions().front();
        own->setToolTip("<b>A</b> (LETTER)");
        button->setDefaultAction(own);
        bar->addWidget(button);
        return button;
    };
    FlyoutButton* five = familyOf("ABCDE");
    FlyoutButton* six = familyOf("ABCDEF");
    window.show();
    katana::qt::test::processEvents();
    // Compared as std::string, which a failure prints as text.
    const std::string muted = theme::textMuted().name().toStdString();
    EXPECT_EQ(tooltipShown(*five).toStdString(),
              "<b>A</b> (LETTER)<br><span style='color:" + muted +
                  "'>Hold, right-click or press the corner triangle for its other tools: "
                  "B / C / D / E</span>");
    EXPECT_EQ(tooltipShown(*six).toStdString(),
              "<b>A</b> (LETTER)<br><span style='color:" + muted +
                  "'>Hold, right-click or press the corner triangle for its other tools: "
                  "B / C / D and 2 more</span>");
    // Shown, not only built: the button's own tooltip property is the
    // tool's, as setDefaultAction keeps setting it, and the family's line
    // is added when the tooltip is asked for.
    EXPECT_EQ(six->toolTip().toStdString(), "<b>A</b> (LETTER)");
}

TEST(ToolFamilyButtons, EachFamilysTooltipShownNamesItsOtherToolsOrThreeAndACount)
{
    // The catalogue's families, as the window makes them: the rule above
    // on each - Circle's six tools, Ellipse's three, the eighteen of
    // Vertices, whose seventeen others were a wall of six lines under the
    // tool's own two when every one was named.
    DrawBar built;
    for (const QString& name : kFamilies) {
        const std::string where = name.toStdString();
        FlyoutButton* button = built.family(name);
        ASSERT_NE(button, nullptr) << where;
        const QString tip = tooltipShown(*button);
        ASSERT_FALSE(tip.isEmpty()) << where << ": no tooltip shown";
        EXPECT_TRUE(tip.startsWith(button->defaultAction()->toolTip())) << tip.toStdString();
        QStringList others;
        for (const QAction* item : button->menu()->actions()) {
            if (item != button->defaultAction()) {
                others << QString(item->text()).remove('&').toHtmlEscaped();
            }
        }
        ASSERT_FALSE(others.isEmpty()) << where;
        const QString named = others.size() <= 4
                                  ? others.join(" / ")
                                  : others.first(3).join(" / ") +
                                        QString(" and %1 more").arg(others.size() - 3);
        EXPECT_EQ(familyLine(tip).toStdString(),
                  ("<br><span style='color:" + theme::textMuted().name() +
                   "'>Hold, right-click or press the corner triangle for its other tools: " +
                   named + "</span>")
                      .toStdString())
            << where;
    }
    // The Vertices family names three and counts the rest: its fourth other
    // is in the menu, not the tooltip.
    FlyoutButton* vertices = built.family("Vertices");
    ASSERT_GT(vertices->menu()->actions().size(), 5);
    const QString fourthOther = QString(vertices->menu()->actions().at(4)->text()).remove('&');
    EXPECT_FALSE(familyLine(tooltipShown(*vertices)).contains(fourthOther.toHtmlEscaped()))
        << fourthOther.toStdString();
}

namespace {

// Undo as MainWindow::buildActions makes it - a split button with its
// history, following its bar's icon size - on a horizontal Edit toolbar with
// the theme's look on its window.
struct EditBar {
    // Before the window, so it outlives every widget drawn with it.
    std::unique_ptr<QStyle> style{theme::makeStyle()};
    QMainWindow window;
    QToolBar* bar = nullptr;
    QToolButton* undo = nullptr;
    QMenu* history = nullptr;
    int undone = 0;

    EditBar()
    {
        window.setStyle(style.get());
        window.setStyleSheet(theme::styleSheet());
        window.setCentralWidget(new QWidget);
        bar = new QToolBar("Edit", &window);
        bar->setObjectName("EditToolBar");
        bar->setIconSize(QSize(20, 20));
        window.addToolBar(Qt::TopToolBarArea, bar);
        auto* action = new QAction(katana::qt::icon(katana::qt::Icon::Undo), "Undo", &window);
        QObject::connect(action, &QAction::triggered, &window, [this] { ++undone; });
        undo = new QToolButton(bar);
        undo->setObjectName("editUndoButton");
        undo->setDefaultAction(action);
        undo->setPopupMode(QToolButton::MenuButtonPopup);
        undo->setAutoRaise(true);
        history = new QMenu(undo);
        history->addAction("1 CREATE_POINT");
        undo->setMenu(history);
        bar->addWidget(undo);
        katana::qt::tools::followToolBarIconSize(*undo, *bar);
        window.resize(640, 200);
        window.show();
        katana::qt::test::processEvents();
    }
    EditBar(const EditBar&) = delete;
    EditBar& operator=(const EditBar&) = delete;

    // The strip the history opens from, as the style lays it.
    QRect strip() const
    {
        QStyleOptionToolButton option;
        option.initFrom(undo);
        option.rect = undo->rect();
        option.subControls = QStyle::SC_ToolButton | QStyle::SC_ToolButtonMenu;
        option.features = QStyleOptionToolButton::MenuButtonPopup | QStyleOptionToolButton::HasMenu;
        option.iconSize = bar->iconSize();
        return undo->style()->subControlRect(QStyle::CC_ToolButton, &option,
                                             QStyle::SC_ToolButtonMenu, undo);
    }
};

} // namespace

TEST(SplitButtons, UndosArrowHasAStripOfItsOwnBesideTheIcon)
{
    // The strip the menu opens from, and the arrow drawn in it, are clear of
    // the icon's square: the stylesheet pads the split button by the strip's
    // width. Without that rule the strip was the button's right 12 px, over
    // the icon. By hand: 20 px of icon, Qt's 3 (QStyleSheetStyle,
    // CT_ToolButton), 4 px of padding on the left and the strip's 10 on the
    // right, a 1 px border each side - 39 px, the contents at 5..27 with the
    // icon's square centred at 6..25. The strip is laid in the BORDER
    // rectangle (QStyleSheetStyle's defaultOrigin gives ::menu-button
    // Origin_Border), its right 10 px: 29..38, a pixel of padding past the
    // contents. With the 4 px every button has added to the 10, the strip was
    // 33..42 and each arrow sat 11 px from its own icon's ink and 12 from the
    // next button's, halfway between the two.
    EditBar built;
    QToolButton& button = *built.undo;
    EXPECT_EQ(button.size(), QSize(39, 33));
    const auto parts = drawnParts(button);
    ASSERT_FALSE(parts.icon.isNull());
    ASSERT_FALSE(parts.sign.isNull()) << "no arrow drawn";
    EXPECT_EQ(parts.icon, QRect(6, 6, 20, 20));
    EXPECT_FALSE(parts.sign.intersects(parts.icon));

    const QRect strip = built.strip();
    ASSERT_TRUE(strip.isValid());
    EXPECT_EQ(strip.left(), 29);
    EXPECT_EQ(strip.right(), 38);
    EXPECT_FALSE(strip.intersects(parts.icon));
    EXPECT_TRUE(strip.contains(parts.sign));

    // A click on the icon's right edge undoes; the strip opens the history.
    const MenuWatch watch(*built.history);
    click(button, QPoint(parts.icon.right(), parts.icon.center().y()));
    EXPECT_EQ(built.undone, 1);
    EXPECT_EQ(watch.shown, 0);
    click(button, strip.center());
    EXPECT_EQ(watch.shown, 1);
    EXPECT_EQ(built.undone, 1);
}

TEST(SplitButtons, UndosStripTakesTheButtonsHoverWithNoShadeOfItsOwn)
{
    // The stylesheet gives the strip the whole button's hover, never the
    // pointer's part of it (its SC_ToolButton is the whole button), so a
    // hover shade of the strip's own lay on it with the pointer on the icon
    // too: a band of the pressed colour, darker than the rest of the button,
    // that read as pressed. Under the pointer the strip is the button's
    // hover colour. Compared beside the arrow (the strip's upper rows, below
    // its rounded corner) and in the padding left of the icon.
    EditBar built;
    QToolButton& button = *built.undo;
    const QRect strip = built.strip();
    const auto parts = drawnParts(button);
    ASSERT_GT(parts.sign.top(), strip.top() + 6) << "no room above the arrow";
    const QPoint inStrip(strip.center().x() - 1, strip.top() + 5);
    const QPoint inBody(3, button.height() / 2);
    ASSERT_FALSE(parts.icon.contains(inBody));
    button.setAttribute(Qt::WA_UnderMouse, true);
    const QPointF at(parts.icon.center());
    QHoverEvent enter(QEvent::HoverEnter, at, QPointF(button.mapToGlobal(parts.icon.center())), at);
    QCoreApplication::sendEvent(&button, &enter);
    katana::qt::test::processEvents();
    const QImage hovered = shot(button);
    EXPECT_EQ(hovered.pixelColor(inBody), theme::hover());
    EXPECT_EQ(hovered.pixelColor(inStrip), theme::hover());
    button.setAttribute(Qt::WA_UnderMouse, false);
}

TEST(ToolBarSigns, TheCheckSeesASplitButtonsArrowDrawnOverItsIcon)
{
    // --check-toolbars must be able to fail. The same Undo button under the
    // theme's stylesheet as it was before the split buttons were padded -
    // every tool button's padding and border, nothing for the strip - is a
    // clash; under the theme it is not.
    const auto build = [](QMainWindow& window, const QString& css) {
        window.setStyleSheet(css);
        window.setCentralWidget(new QWidget);
        auto* bar = new QToolBar("Edit", &window);
        bar->setObjectName("EditToolBar");
        bar->setIconSize(QSize(20, 20));
        window.addToolBar(Qt::TopToolBarArea, bar);
        auto* button = new QToolButton(bar);
        button->setObjectName("editUndoButton");
        button->setDefaultAction(
            new QAction(katana::qt::icon(katana::qt::Icon::Undo), "Undo", &window));
        button->setPopupMode(QToolButton::MenuButtonPopup);
        button->setAutoRaise(true);
        auto* history = new QMenu(button);
        history->addAction("1 CREATE_POINT");
        button->setMenu(history);
        bar->addWidget(button);
        window.resize(640, 200);
        window.show();
        katana::qt::test::processEvents();
    };
    const std::unique_ptr<QStyle> style(theme::makeStyle());
    {
        QMainWindow unpadded;
        unpadded.setStyle(style.get());
        build(unpadded, "QToolButton { background: transparent; border: 1px solid transparent; "
                        "border-radius: 5px; padding: 4px; }");
        int buttons = 0;
        QStringList checked;
        const QStringList clashes = menuSignClashes(unpadded, &checked, &buttons);
        EXPECT_EQ(buttons, 1);
        ASSERT_EQ(clashes.size(), 1) << checked.join("\n").toStdString();
        EXPECT_TRUE(clashes.front().startsWith("EditToolBar > editUndoButton: its menu's sign"))
            << clashes.front().toStdString();
        EXPECT_TRUE(clashes.front().contains("is drawn over its icon")) << clashes.front().toStdString();
    }
    {
        QMainWindow themed;
        themed.setStyle(style.get());
        build(themed, theme::styleSheet());
        int buttons = 0;
        QStringList checked;
        const QStringList clashes = menuSignClashes(themed, &checked, &buttons);
        EXPECT_EQ(buttons, 1);
        EXPECT_TRUE(clashes.isEmpty()) << clashes.join("\n").toStdString();
        ASSERT_EQ(checked.size(), 1);
        EXPECT_TRUE(checked.front().contains("clear of its icon")) << checked.front().toStdString();
    }
}

TEST(ToolBarSigns, TheCheckJudgesAFamilysMarkAgainstItsIconInTheScreensOwnPixels)
{
    // A family's mark starts the pixel after its icon's square ends, so on a
    // scaled screen the two meet at a widget pixel: at 125% the icon ends at
    // device column 32 and the mark starts at 33, and each box taken back to
    // the widget's pixels, rounded outwards, reached pixel 26 - four clashes
    // that were not on the screen. The check judges in device pixels. At a
    // ratio of 1 the two are the same; qt_toolbar_signs_at_125_percent runs
    // this at the owner's 125%.
    DrawBar built;
    int buttons = 0;
    QStringList checked;
    const QStringList clashes = menuSignClashes(built.window, &checked, &buttons);
    EXPECT_EQ(buttons, static_cast<int>(kFamilies.size()));
    EXPECT_TRUE(clashes.isEmpty()) << clashes.join("\n").toStdString();
    const double ratio = built.window.devicePixelRatio();
    for (const QString& line : checked) {
        EXPECT_EQ(line.contains("(device pixels at a ratio of"), ratio != 1.0) << line.toStdString();
    }
    // The boxes themselves, both ways: in device pixels apart, and on a
    // scaled screen meeting in the widget's.
    FlyoutButton* circle = built.family("Circle");
    ASSERT_NE(circle, nullptr);
    const auto parts = drawnParts(*circle);
    EXPECT_DOUBLE_EQ(parts.ratio, ratio);
    EXPECT_FALSE(parts.signDevice.intersects(parts.iconDevice));
    EXPECT_EQ(parts.signDevice.left(), parts.iconDevice.right() + 1);
    if (ratio == 1.0) {
        EXPECT_EQ(parts.icon, parts.iconDevice);
        EXPECT_EQ(parts.sign, parts.signDevice);
    }
}

TEST(ToolBarOverflow, AToolbarTooShortForItsButtonsShowsItsWholeOverflowArrow)
{
    // The overflow button is 12 px deep; the padding and border every tool
    // button has left its arrow 2 px, a dot. Ink against a baseline: the
    // button shows as many rows of ink as the style's own arrow has when it
    // is painted at that depth on the toolbar's ground.
    DrawBar built;
    built.window.resize(480, 300);
    katana::qt::test::processEvents();
    auto* overflow = built.bar->findChild<QToolButton*>("qt_toolbar_ext_button");
    ASSERT_NE(overflow, nullptr);
    ASSERT_TRUE(overflow->isVisible()) << "the Draw toolbar fitted in 300 px";

    // Rows holding a pixel unlike the ground, give or take antialiasing.
    const auto inkRows = [](const QImage& image, const QColor& ground) {
        int rows = 0;
        for (int y = 0; y < image.height(); ++y) {
            for (int x = 0; x < image.width(); ++x) {
                const QColor pixel = image.pixelColor(x, y);
                if (std::abs(pixel.red() - ground.red()) > 24 ||
                    std::abs(pixel.green() - ground.green()) > 24 ||
                    std::abs(pixel.blue() - ground.blue()) > 24) {
                    ++rows;
                    break;
                }
            }
        }
        return rows;
    };
    const QIcon arrow =
        overflow->style()->standardIcon(QStyle::SP_ToolBarVerticalExtensionButton, nullptr, overflow);
    const int extent =
        overflow->style()->pixelMetric(QStyle::PM_ToolBarExtensionExtent, nullptr, overflow);
    QImage own(extent, extent, QImage::Format_ARGB32);
    own.fill(theme::window());
    {
        QPainter painter(&own);
        arrow.paint(&painter, own.rect());
    }
    const int arrowRows = inkRows(own, theme::window());
    ASSERT_GT(arrowRows, 2) << "the style's arrow is itself a dot";
    // The toolbar where the button is: the button has no ground of its own.
    const QImage drawn = built.bar->grab(overflow->geometry()).toImage();
    EXPECT_EQ(inkRows(drawn, theme::window()), arrowRows);

    // In the muted text colour: antialiasing changes a pixel's coverage, not
    // its colour, so every pixel mostly covered is that colour.
    QImage alone(extent, extent, QImage::Format_ARGB32);
    alone.fill(Qt::transparent);
    {
        QPainter painter(&alone);
        arrow.paint(&painter, alone.rect());
    }
    int covered = 0;
    for (int y = 0; y < alone.height(); ++y) {
        for (int x = 0; x < alone.width(); ++x) {
            const QColor pixel = alone.pixelColor(x, y);
            if (pixel.alpha() >= 128) {
                ++covered;
                EXPECT_LE(std::abs(pixel.red() - theme::textMuted().red()), 3) << x << "," << y;
                EXPECT_LE(std::abs(pixel.green() - theme::textMuted().green()), 3) << x << "," << y;
                EXPECT_LE(std::abs(pixel.blue() - theme::textMuted().blue()), 3) << x << "," << y;
            }
        }
    }
    EXPECT_GT(covered, 0);
}
