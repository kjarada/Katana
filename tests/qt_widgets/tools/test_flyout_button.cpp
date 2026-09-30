// The signs of a menu on a toolbar or title bar button
// (src/katana_qt/tools/flyout_button, theme.cpp): a tool family's corner
// triangle is drawn beside its icon, never on it; a press on the icon runs
// the tool while the triangle, a press held and a right click open the
// family; the button is framed only while the tool its icon shows runs, its
// triangle lit while any of its tools does; Undo's and Redo's arrows have a
// strip of their own beside the icon; a view's kind switcher draws its arrow
// beside its icon; the check the window runs headless (--check-toolbars)
// sees an arrow drawn over an icon; and a toolbar's overflow arrow is drawn
// whole, in whole device pixels.
//
// The toolbars are built as the window builds them - fillToolMenus for the
// families - with the theme's style and stylesheet on the window under test,
// not on the application, so the other widget tests keep the look they were
// written against (test_theme.cpp does the same). Geometry is asserted as
// rectangles; where pixels are compared it is a button against itself or
// against a plain button with the same icon, never against a count.
//
// Every test holds at any device-pixel ratio. What is drawn is measured in
// the grab's device pixels: hand-worked at a ratio of 1, and at any other
// held to the same rule, worked at 125% in the comments.
// qt_toolbar_signs_at_125_percent runs them all at the owner's 125%.

#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdlib>
#include <memory>
#include <string>
#include <vector>

#include <QAction>
#include <QApplication>
#include <QContextMenuEvent>
#include <QDockWidget>
#include <QElapsedTimer>
#include <QHelpEvent>
#include <QImage>
#include <QMainWindow>
#include <QMenu>
#include <QMouseEvent>
#include <QPainter>
#include <QStatusBar>
#include <QStyle>
#include <QStyleOptionToolButton>
#include <QTextBlock>
#include <QTextDocument>
#include <QTextLayout>
#include <QTextOption>
#include <QTimer>
#include <QToolBar>
#include <QToolButton>
#include <QToolTip>
#include <QWindow>

#include "dock_chrome.hpp"
#include "icons.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/interactive_tool.hpp"
#include "theme.hpp"
#include "tools/flyout_button.hpp"
#include "tools/tool_icons.hpp"
#include "tools/tool_menus.hpp"
#include "view_workspace.hpp"
#include "widget_harness.hpp"

// What QtTest's mouse functions call when QTEST_QPA_MOUSE_HANDLING is set
// (qtestmouse.h): the platform's way in, exported by QtGui. Declared here
// rather than linking QtTest for one function.
QT_BEGIN_NAMESPACE
Q_GUI_EXPORT void qt_handleMouseEvent(QWindow* window, const QPointF& local, const QPointF& global,
                                      Qt::MouseButtons state, Qt::MouseButton button,
                                      QEvent::Type type, Qt::KeyboardModifiers mods, int timestamp);
QT_END_NAMESPACE

namespace theme = katana::qt::theme;
using katana::cad::toolCatalog;
using katana::qt::DockChrome;
using katana::qt::ViewWorkspace;
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

// Whole device pixels in `widgetPixels` at `ratio`, taken down: the mark's
// legs and inset (flyout_button.cpp).
int wholeDevicePixels(int widgetPixels, double ratio)
{
    return static_cast<int>(std::floor(widgetPixels * ratio + 1e-6));
}

// The device pixel under the centre of widget pixel `at`: where a grab at
// `ratio` shows what the widget drew there.
QPoint devicePixel(QPoint at, double ratio)
{
    return {static_cast<int>(std::floor((at.x() + 0.5) * ratio)),
            static_cast<int>(std::floor((at.y() + 0.5) * ratio))};
}

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

// A pointer event as the platform delivers it, at `at` in `widget`: through
// QGuiApplication and the widget's window, which send Enter, Leave and the
// hover events in the order, and in the cases, a person's pointer gets them -
// a popup open among them - where a sendEvent hands the widget one event of
// the test's choosing. Each a second after the last, so no two presses make
// a double click.
void routed(QWidget& widget, QPoint at, QEvent::Type type, Qt::MouseButton button = Qt::NoButton)
{
    static int timestamp = 0;
    timestamp += 1000;
    QWindow* window = widget.window()->windowHandle();
    ASSERT_NE(window, nullptr) << "the widget's window is not shown";
    const QPointF global = widget.mapToGlobal(QPointF(at));
    const Qt::MouseButtons held =
        type == QEvent::MouseButtonPress ? Qt::MouseButtons(button) : Qt::MouseButtons();
    qt_handleMouseEvent(window, window->mapFromGlobal(global), global, held, button, type,
                        Qt::NoModifier, timestamp);
    katana::qt::test::processEvents();
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
    QWidget& canvas() const { return *window.centralWidget(); }
};

// What a family button at `size` px icons draws, against the design. By
// hand, under the theme a button is its icon, Qt's 3 px (QStyleSheetStyle,
// CT_ToolButton), 4 px of padding and a 1 px border each side: 13 px more
// than its icon, the icon's square centred 6 px in. The corner mark is 5 px,
// inset 2 px from the edge: at (s + 13 - 1 - 2) - 4 = s + 6, which leaves
// (s + 6) - (6 + s - 1) - 1 = 0 px between it and the icon's square - they
// meet, never overlap - at every size s.
//
// On a scaled screen, in the grab's device pixels: the icon a pixmap of s
// times the ratio; the mark a square of 5 px times the ratio taken down to
// whole pixels, set 2 px times the ratio (taken down) in from the button's
// last device pixel. At 125% and 20 px icons: the button's 33 px are 41.25,
// grabbed as 41, its last pixel 40; the mark 6 px ending at 38, 33..38; the
// icon's 25 px from 7.5, drawn from 8 - 8..32.
void expectDrawnAsDesigned(FlyoutButton& button, int size, const std::string& where)
{
    const auto parts = drawnParts(button);
    const double ratio = parts.ratio;
    ASSERT_FALSE(parts.iconDevice.isNull()) << where << ": no icon drawn";
    ASSERT_FALSE(parts.signDevice.isNull()) << where << ": no mark drawn";
    if (ratio == 1.0) {
        EXPECT_EQ(parts.icon, QRect(6, 6, size, size)) << where;
        EXPECT_EQ(parts.sign, QRect(size + 6, size + 6, 5, 5)) << where;
    }
    const int side = qRound(size * ratio);
    EXPECT_EQ(parts.iconDevice.size(), QSize(side, side)) << where;
    const int leg = wholeDevicePixels(5, ratio);
    const int last = qRound((size + 13) * ratio) - 1;
    const int corner = last - wholeDevicePixels(2, ratio);
    EXPECT_EQ(parts.signDevice, QRect(corner - leg + 1, corner - leg + 1, leg, leg)) << where;
    EXPECT_FALSE(parts.signDevice.intersects(parts.iconDevice)) << where;
}

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
    // View > Toolbars offers 16, 20 and 28 px icons. The widget's geometry
    // by hand, as expectDrawnAsDesigned works it: 13 px more than the icon,
    // the icon at 6, the 5 px mark at the icon's size and 6.
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
            // its square.
            expectDrawnAsDesigned(*button, size, where);
            // Measured leaves the button as it was: its menu, and the icon
            // its tool gave it.
            EXPECT_EQ(button->menu(), built.window.findChild<QMenu*>("toolFamily.Draw." + name))
                << where;
            EXPECT_EQ(button->icon().cacheKey(), button->defaultAction()->icon().cacheKey())
                << where;
        }
    }
}

TEST(ToolFamilyButtons, AFamilyIsFramedOnlyWhileTheToolItsIconShowsRuns)
{
    // Ink against a baseline: the Circle family beside a plain button with
    // the same icon. They may differ only inside the mark - at rest, under
    // the pointer, running the family's first tool, the one its icon shows
    // (the plain button running too), and running another of its tools, 2
    // Points, with the plain button AT REST. A frame is the toolbar's one
    // sign that a button's tool runs, and a frame round the Centre Radius
    // icon says Centre Radius runs: until 2026-09-30 the family was framed
    // while any of its tools ran, and with Delete Vertex running the
    // Vertices button framed Insert Vertex's icon. While any of its tools
    // runs the mark is the accent, the running frame's colour; at rest it is
    // muted, and under the pointer, off its corner, the text colour.
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
    const QRect mark = drawnParts(*circle).signDevice;
    ASSERT_FALSE(mark.isNull()) << "no mark drawn";
    if (circle->devicePixelRatio() == 1.0) {
        EXPECT_EQ(mark, circle->markRect());
    }
    struct State {
        std::string name;
        bool hover;
        bool own;
        bool another;
        QColor ink;
    };
    const std::array states = {
        State{"at rest", false, false, false, theme::textMuted()},
        State{"under the pointer", true, false, false, theme::text()},
        State{"running the tool its icon shows", false, true, false, theme::accent()},
        State{"running another of its tools", false, false, true, theme::accent()},
    };
    for (const State& state : states) {
        for (QToolButton* each : {static_cast<QToolButton*>(circle), built.twin}) {
            each->setAttribute(Qt::WA_UnderMouse, state.hover);
        }
        first->setChecked(state.own);
        other->setChecked(state.another);
        plainRunning->setChecked(state.own);
        katana::qt::test::processEvents();
        EXPECT_EQ(circle->familyRunning(), state.own || state.another) << state.name;
        EXPECT_EQ(circle->isChecked(), state.own) << state.name;
        const QImage family = shot(*circle);
        const QRect differs = differenceBox(family, shot(*built.twin));
        EXPECT_FALSE(differs.isNull()) << state.name << ": the mark is not drawn";
        EXPECT_TRUE(mark.contains(differs))
            << state.name << ": the family differs from the plain button at x " << differs.left()
            << ".." << differs.right() << " y " << differs.top() << ".." << differs.bottom();
        // The right-angled corner, inked by every row of the triangle.
        EXPECT_EQ(family.pixelColor(mark.bottomRight()), state.ink) << state.name;
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
    // its mark stays unlit on the screen until the pointer passes over it.
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
    // must too, and then draws as one made at that size would. Made at 20 px
    // and switched, a family kept its 33 x 33 - at 28 its icon was drawn at
    // 23 px beside the others' 28, its mark 3 px against the icon's corner.
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
            expectDrawnAsDesigned(*button, size, where);
        }
    }
}

TEST(ToolFamilyButtons, TheMarkIsTheSameWholeStaircaseOfDevicePixelsWhereverTheButtonSits)
{
    // At 125% and 150% - where Windows puts most laptop screens, and where
    // the owner's is (qt_render_view_at_125_percent) - a widget pixel is not
    // a whole number of device pixels. The triangle is painted a device row
    // at a time: the k-th row from its top is k pixels ending at its right
    // column. Its legs are 5 px times the ratio taken down, 5, 6, 7, 8 and
    // 10 device pixels at 100, 125, 150, 175 and 200%, and its corner is 2 px
    // times the ratio, taken down, in from the button's last device pixel -
    // wherever the button sits. Painted a widget row at a time the rows were
    // uneven (1, 2, 3, 3, 5, 6 at 125%); with the mark's own square snapped
    // to the device its size hung on the button's place in the window: at
    // 150%, 8 rows where the button began on a whole device pixel and 7
    // where it began on a half. Rendered here at each ratio with the button
    // 0 to 3 widget pixels in (0, 1.5, 3 and 4.5 device pixels at 150%),
    // and grabbed at the screen's own ratio.
    DrawBar built;
    FlyoutButton* circle = built.family("Circle");
    ASSERT_NE(circle, nullptr);
    QMenu* family = circle->menu();
    const double screen = circle->devicePixelRatio();
    struct Case {
        double ratio;
        int offset;
        bool grabbed;
    };
    std::vector<Case> cases;
    for (const double ratio : {1.0, 1.25, 1.5, 1.75, 2.0}) {
        for (int offset = 0; offset < 4; ++offset) {
            cases.push_back({ratio, offset, false});
        }
    }
    cases.push_back({screen, 0, true});
    for (const Case& each : cases) {
        const auto render = [&] {
            if (each.grabbed) {
                return shot(*circle);
            }
            const QSize room = circle->size() + QSize(each.offset, each.offset);
            QImage image((QSizeF(room) * each.ratio).toSize(), QImage::Format_ARGB32);
            image.setDevicePixelRatio(each.ratio);
            image.fill(Qt::transparent);
            circle->render(&image, QPoint(each.offset, each.offset));
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
        const std::string where =
            (each.grabbed ? "grabbed at the screen's ratio of " : "rendered at a ratio of ") +
            QString::number(each.ratio).toStdString() + ", " + std::to_string(each.offset) +
            " px in";
        const int leg = static_cast<int>(rows.size());
        ASSERT_EQ(leg, wholeDevicePixels(5, each.ratio)) << where;
        for (int k = 0; k < leg; ++k) {
            const auto& [count, left, right, y] = rows[static_cast<std::size_t>(k)];
            EXPECT_EQ(count, k + 1) << where << ", row " << k;
            EXPECT_EQ(right - left + 1, count) << where << ", row " << k << " has a gap";
            EXPECT_EQ(right, rows.front()[2]) << where << ", row " << k << " off the right column";
            EXPECT_EQ(y, rows.front()[3] + k) << where << ", row " << k << " not below the last";
        }
        const int last = qRound((33 + each.offset) * each.ratio) - 1;
        const int corner = last - wholeDevicePixels(2, each.ratio);
        EXPECT_EQ(rows.back()[2], corner) << where << ": the corner's column";
        EXPECT_EQ(rows.back()[3], corner) << where << ": the corner's row";
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
    // tool, so the mark says which the pointer is over. The pointer is moved
    // as a person moves it, through the platform. The mark's right-angled
    // corner pixel is inked by every row of the triangle, and is the mark's
    // colour exactly.
    DrawBar built;
    FlyoutButton* circle = built.family("Circle");
    ASSERT_NE(circle, nullptr);
    const QPoint corner = drawnParts(*circle).signDevice.bottomRight();
    const auto moveTo = [&](QWidget& widget, QPoint at) {
        routed(widget, at, QEvent::MouseMove);
        return shot(*circle).pixelColor(corner);
    };
    moveTo(built.canvas(), built.canvas().rect().center());
    EXPECT_EQ(moveTo(*circle, circle->iconRect().center()), theme::text());
    EXPECT_TRUE(circle->underMouse());
    EXPECT_FALSE(circle->pointerOnMark());
    EXPECT_EQ(moveTo(*circle, circle->markRect().center()), theme::accent());
    EXPECT_TRUE(circle->pointerOnMark());
    // Off the mark but still in its corner, beside the icon: the press there
    // opens the family too, so the mark stays lit.
    const QPoint besideIcon(circle->iconRect().right() + 2, circle->markRect().top() - 3);
    ASSERT_TRUE(circle->menuZone().contains(besideIcon));
    EXPECT_EQ(moveTo(*circle, besideIcon), theme::accent());
    EXPECT_EQ(moveTo(*circle, circle->iconRect().center()), theme::text());
    EXPECT_FALSE(circle->pointerOnMark());
    EXPECT_EQ(moveTo(*circle, circle->markRect().center()), theme::accent());
    EXPECT_EQ(moveTo(built.canvas(), built.canvas().rect().center()), theme::textMuted());
    EXPECT_FALSE(circle->underMouse());
    EXPECT_FALSE(circle->pointerOnMark());
}

TEST(ToolFamilyButtons, TheMarkGoesOutWhenThePointerLeavesForTheFamilysMenu)
{
    // The corner's press opens the family, and to pick from it the pointer
    // leaves the button while the menu is open. Qt then tells the button
    // with a Leave and no HoverLeave (QApplication sends a HoverLeave only
    // with no popup up), so a mark lit by the hover events alone stayed lit
    // in the accent - the colour of a tool running - after the menu closed,
    // with nothing running, until the pointer next crossed the button. The
    // pointer is moved as a person moves it, through the platform: a Leave
    // sent by hand would say what the test thinks Qt sends, not what it does.
    DrawBar built;
    FlyoutButton* circle = built.family("Circle");
    ASSERT_NE(circle, nullptr);
    QMenu* family = circle->menu();
    ASSERT_NE(family, nullptr);
    const QPoint corner = drawnParts(*circle).signDevice.bottomRight();
    routed(built.canvas(), built.canvas().rect().center(), QEvent::MouseMove);
    routed(*circle, circle->iconRect().center(), QEvent::MouseMove);
    routed(*circle, circle->markRect().center(), QEvent::MouseMove);
    ASSERT_TRUE(circle->pointerOnMark()) << "the pointer did not reach the corner";
    ASSERT_EQ(shot(*circle).pixelColor(corner), theme::accent());

    // Pressed on the corner, the family opens in a loop of its own; there
    // the pointer goes over the canvas and the menu is closed, as Escape or
    // a click elsewhere closes it.
    bool opened = false;
    bool leftWhileOpen = false;
    const QMetaObject::Connection hook =
        QObject::connect(family, &QMenu::aboutToShow, family, [&] {
            QTimer::singleShot(0, family, [&] {
                opened = true;
                routed(built.canvas(), built.canvas().rect().center(), QEvent::MouseMove);
                leftWhileOpen = !circle->underMouse();
                family->close();
            });
        });
    routed(*circle, circle->markRect().center(), QEvent::MouseButtonPress, Qt::LeftButton);
    QObject::disconnect(hook);
    routed(built.canvas(), built.canvas().rect().center(), QEvent::MouseButtonRelease,
           Qt::LeftButton);
    ASSERT_TRUE(opened) << "the corner's press did not open the family";
    EXPECT_TRUE(leftWhileOpen) << "Qt did not tell the button the pointer had gone";
    EXPECT_FALSE(circle->underMouse());
    EXPECT_FALSE(circle->pointerOnMark());
    EXPECT_FALSE(circle->familyRunning());
    EXPECT_EQ(shot(*circle).pixelColor(corner), theme::textMuted());
}

TEST(ToolFamilyButtons, OnItsCornerTheStatusBarNamesTheFamilyAndElsewhereTheTool)
{
    // The status bar is the window's other line about what is under the
    // pointer. QWidget puts the button's status tip there on Enter - the
    // tool's, which setDefaultAction copies - so on the corner, where a
    // press opens the family, it described Insert Vertex alone. There it is
    // the family's own line, the Draw menu's for its submenu.
    DrawBar built;
    QStatusBar* status = built.window.statusBar();
    katana::qt::test::processEvents();
    FlyoutButton* vertices = built.family("Vertices");
    ASSERT_NE(vertices, nullptr);
    auto* family = built.window.findChild<QMenu*>("toolFamily.Draw.Vertices");
    ASSERT_NE(family, nullptr);
    const QString familyTip = family->menuAction()->statusTip();
    ASSERT_TRUE(familyTip.startsWith("Vertices: Insert Vertex / Delete Vertex / "))
        << familyTip.toStdString();
    const QString toolTip = vertices->defaultAction()->statusTip();
    ASSERT_FALSE(toolTip.isEmpty());
    ASSERT_NE(toolTip, familyTip);

    routed(built.canvas(), built.canvas().rect().center(), QEvent::MouseMove);
    routed(*vertices, vertices->iconRect().center(), QEvent::MouseMove);
    EXPECT_EQ(status->currentMessage().toStdString(), toolTip.toStdString());
    routed(*vertices, vertices->markRect().center(), QEvent::MouseMove);
    EXPECT_EQ(status->currentMessage().toStdString(), familyTip.toStdString());
    routed(*vertices, vertices->iconRect().center(), QEvent::MouseMove);
    EXPECT_EQ(status->currentMessage().toStdString(), toolTip.toStdString());
    // Off the button the line is cleared, as QWidget clears it on Leave.
    routed(*vertices, vertices->markRect().center(), QEvent::MouseMove);
    routed(built.canvas(), built.canvas().rect().center(), QEvent::MouseMove);
    EXPECT_TRUE(status->currentMessage().isEmpty()) << status->currentMessage().toStdString();
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

// A name as the tooltip lists it: its spaces non-breaking.
QString unbroken(QString name) { return name.replace(QChar(' '), QChar(QChar::Nbsp)); }

} // namespace

TEST(ToolFamilyButtons, TheTooltipShownSaysHowToReachTheOtherToolsAndNamesFiveAtMost)
{
    // By hand, on families of letters and one two-word name: with five
    // others each is named, in the menu's order, slashes between; with six,
    // four are and "and 2 more" stands for the rest. A name's own spaces are
    // non-breaking (U+00A0). The tool the click runs is the tooltip's own,
    // never an other.
    const std::unique_ptr<QStyle> style(theme::makeStyle());
    QMainWindow window;
    window.setStyle(style.get());
    window.setStyleSheet(theme::styleSheet());
    auto* bar = new QToolBar("Draw", &window);
    window.addToolBar(Qt::LeftToolBarArea, bar);
    const auto familyOf = [&](const QStringList& names) {
        auto* menu = new QMenu("Letters", &window);
        for (const QString& name : names) {
            menu->addAction(name);
        }
        auto* button = new FlyoutButton(*menu, bar);
        auto* own = menu->actions().front();
        own->setToolTip("<b>A</b> (LETTER)");
        button->setDefaultAction(own);
        bar->addWidget(button);
        return button;
    };
    FlyoutButton* six = familyOf({"A", "B", "C", "D", "E", "Two Words"});
    FlyoutButton* seven = familyOf({"A", "B", "C", "D", "E", "F", "G"});
    window.show();
    katana::qt::test::processEvents();
    // Compared as std::string, which a failure prints as text.
    const std::string muted = theme::textMuted().name().toStdString();
    const std::string nbsp = QString(QChar(QChar::Nbsp)).toStdString();
    EXPECT_EQ(tooltipShown(*six).toStdString(),
              "<b>A</b> (LETTER)<br><span style='color:" + muted +
                  "'>Hold, right-click or press the corner triangle for its other tools: "
                  "B / C / D / E / Two" + nbsp + "Words</span>");
    EXPECT_EQ(tooltipShown(*seven).toStdString(),
              "<b>A</b> (LETTER)<br><span style='color:" + muted +
                  "'>Hold, right-click or press the corner triangle for its other tools: "
                  "B / C / D / E and 2 more</span>");
    // Shown, not only built: the button's own tooltip property is the
    // tool's, as setDefaultAction keeps setting it, and the family's line
    // is added when the tooltip is asked for.
    EXPECT_EQ(seven->toolTip().toStdString(), "<b>A</b> (LETTER)");
}

TEST(ToolFamilyButtons, EachFamilysTooltipShownNamesItsOtherToolsOrFourAndACount)
{
    // The catalogue's families, as the window makes them: the rule above on
    // each - Circle's five others and Arc's four all named, Ellipse's two,
    // and of the Vertices family's seventeen, four and "and 13 more": the
    // seventeen all named were a wall of six lines under the tool's own two.
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
                others << unbroken(QString(item->text()).remove('&')).toHtmlEscaped();
            }
        }
        ASSERT_FALSE(others.isEmpty()) << where;
        const QString named = others.size() <= 5
                                  ? others.join(" / ")
                                  : others.first(4).join(" / ") +
                                        QString(" and %1 more").arg(others.size() - 4);
        EXPECT_EQ(familyLine(tip).toStdString(),
                  ("<br><span style='color:" + theme::textMuted().name() +
                   "'>Hold, right-click or press the corner triangle for its other tools: " +
                   named + "</span>")
                      .toStdString())
            << where;
    }
    EXPECT_EQ(built.family("Circle")->menu()->actions().size(), 6);
    EXPECT_TRUE(familyLine(tooltipShown(*built.family("Vertices"))).endsWith(" and 13 more</span>"));
}

TEST(ToolFamilyButtons, TheTooltipWrapsBetweenTheOtherToolsNamesNeverInsideOne)
{
    // A tooltip long enough is laid out wrapped (QTipLabel word-wraps rich
    // text wider than a quarter of the screen), and with plain spaces Qt
    // broke the line inside a name: Circle's read "... 2 Points / 3" then
    // "Points and 2 more". Laid out here at every width from 60 to 400 px as
    // the tooltip's label lays it out - QLabel's word wrap is
    // QTextOption::WordWrap, where a word wider than the line runs past it
    // rather than breaking - each name the tooltip gives is whole on one
    // line. The document is laid out when asked its size: until then a
    // block has no lines, every position is on "line 0", and the check
    // passed with the names' spaces plain.
    DrawBar built;
    for (const QString& name : kFamilies) {
        FlyoutButton* button = built.family(name);
        ASSERT_NE(button, nullptr) << name.toStdString();
        QStringList names;
        for (const QAction* item : button->menu()->actions()) {
            if (item != button->defaultAction()) {
                names << QString(item->text()).remove('&');
            }
        }
        QTextDocument layout;
        layout.setDefaultFont(QToolTip::font());
        QTextOption wrap = layout.defaultTextOption();
        wrap.setWrapMode(QTextOption::WordWrap);
        layout.setDefaultTextOption(wrap);
        layout.setHtml(familyLine(button->toolTipWithMenu()));
        // How many of the widths wrapped the line of names: the check is
        // only a check where there are lines to break it over.
        int wrapped = 0;
        for (int width = 60; width <= 400; width += 2) {
            layout.setTextWidth(width);
            ASSERT_GT(layout.size().height(), 0.0) << name.toStdString() << " not laid out";
            for (QTextBlock block = layout.begin(); block.isValid(); block = block.next()) {
                // The block's text with its non-breaking spaces as spaces,
                // to find the names by.
                const QString text = QString(block.text()).replace(QChar(QChar::Nbsp), QChar(' '));
                const QTextLayout* lines = block.layout();
                ASSERT_GT(lines->lineCount(), 0) << name.toStdString() << " at " << width << " px";
                wrapped += lines->lineCount() > 2 ? 1 : 0; // the <br>'s empty line, then the names
                for (const QString& tool : names) {
                    const qsizetype at = text.indexOf(tool);
                    if (at < 0) {
                        continue; // one of "and N more"
                    }
                    const QTextLine first = lines->lineForTextPosition(static_cast<int>(at));
                    const QTextLine last =
                        lines->lineForTextPosition(static_cast<int>(at + tool.size() - 1));
                    ASSERT_TRUE(first.isValid() && last.isValid()) << tool.toStdString();
                    EXPECT_EQ(first.lineNumber(), last.lineNumber())
                        << name.toStdString() << ": \"" << tool.toStdString()
                        << "\" broken over two lines at " << width << " px";
                }
            }
        }
        EXPECT_GT(wrapped, 0) << name.toStdString() << ": never wrapped between 60 and 400 px";
    }
}

TEST(ToolFamilyButtons, WhileAnotherOfItsToolsRunsTheTooltipNamesIt)
{
    // Another of the family's tools running lights the mark and frames
    // nothing (the icon is not its), so the tooltip says which runs, in the
    // accent the mark is lit in. The tool its icon shows running is said by
    // its frame; nothing running, nothing is said.
    DrawBar built;
    FlyoutButton* vertices = built.family("Vertices");
    ASSERT_NE(vertices, nullptr);
    QAction* remove = built.actions.action("draw.vertex.delete");
    ASSERT_NE(remove, nullptr);
    const std::string running =
        "<br><span style='color:" + theme::accent().name().toStdString() + "'>Running now: ";
    EXPECT_EQ(tooltipShown(*vertices).toStdString().find(running), std::string::npos);
    remove->setChecked(true);
    katana::qt::test::processEvents();
    const std::string withDelete = tooltipShown(*vertices).toStdString();
    const std::string deleteVertex = unbroken("Delete Vertex").toStdString();
    EXPECT_TRUE(withDelete.ends_with(running + deleteVertex + "</span>")) << withDelete;
    remove->setChecked(false);
    vertices->defaultAction()->setChecked(true);
    katana::qt::test::processEvents();
    EXPECT_EQ(tooltipShown(*vertices).toStdString().find(running), std::string::npos);
    vertices->defaultAction()->setChecked(false);
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
    // the icon's square: the stylesheet pads the split button for the strip.
    // Without that rule the strip was the button's right 12 px, over the
    // icon. By hand: 20 px of icon, Qt's 3 (QStyleSheetStyle,
    // CT_ToolButton), 4 px of padding on the left and 13 on the right, a
    // 1 px border each side - 42 px, the contents at 5..27 with the icon's
    // square centred at 6..25. The strip is laid in the BORDER rectangle
    // (QStyleSheetStyle's defaultOrigin gives ::menu-button Origin_Border),
    // its right 12 px: 30..41, two pixels of padding past the contents.
    // Fusion draws its arrow 8 px wide, (12 - 8) / 2 = 2 px into the strip
    // (qt_fusion_draw_arrow): 32..39, two pixels in from the button's edge.
    // At 125% Fusion paints it into a cached pixmap set at the strip's
    // device position, 37.5, and its ink spreads over 39..50 of the grab's
    // 53, two pixels short of the last. With a 10 px strip and 10 of padding
    // the arrow was 30..37 of 39, a pixel from the edge against the rounded
    // hover - at 125% 37..47 of 49, one short.
    EditBar built;
    QToolButton& button = *built.undo;
    EXPECT_EQ(button.size(), QSize(42, 33));
    const QRect icon(6, 6, 20, 20); // by hand, above
    const auto parts = drawnParts(button);
    ASSERT_FALSE(parts.iconDevice.isNull());
    ASSERT_FALSE(parts.signDevice.isNull()) << "no arrow drawn";
    EXPECT_FALSE(parts.signDevice.intersects(parts.iconDevice));

    const QRect strip = built.strip();
    ASSERT_TRUE(strip.isValid());
    EXPECT_EQ(strip.left(), 30);
    EXPECT_EQ(strip.right(), 41);
    EXPECT_FALSE(strip.intersects(icon));
    if (parts.ratio == 1.0) {
        EXPECT_EQ(parts.icon, icon);
        EXPECT_EQ(parts.sign.left(), 32);
        EXPECT_EQ(parts.sign.right(), 39);
    }
    // At any ratio the arrow is inside the strip and short of the button's
    // last device pixel by at least the 2 px times the ratio, taken down.
    const QRect stripOnDevice(QPoint(wholeDevicePixels(strip.left(), parts.ratio), 0),
                              QPoint(qRound((strip.right() + 1) * parts.ratio) - 1,
                                     qRound(button.height() * parts.ratio) - 1));
    EXPECT_TRUE(stripOnDevice.contains(parts.signDevice));
    EXPECT_LE(parts.signDevice.right(),
              qRound(button.width() * parts.ratio) - 1 - wholeDevicePixels(2, parts.ratio));

    // A click on the icon's right edge undoes; the strip opens the history.
    const MenuWatch watch(*built.history);
    click(button, QPoint(icon.right(), icon.center().y()));
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
    const double ratio = parts.ratio;
    const QPoint inStrip(strip.center().x() - 1, strip.top() + 5);
    const QPoint inBody(3, button.height() / 2);
    ASSERT_LT(devicePixel(inStrip, ratio).y(), parts.signDevice.top()) << "no room above the arrow";
    ASSERT_FALSE(parts.iconDevice.contains(devicePixel(inBody, ratio)));
    routed(*built.window.centralWidget(), built.window.centralWidget()->rect().center(),
           QEvent::MouseMove);
    // The icon's middle, by hand as in the test above: its square at 6..25.
    routed(button, QPoint(16, 16), QEvent::MouseMove);
    ASSERT_TRUE(button.underMouse());
    const QImage hovered = shot(button);
    EXPECT_EQ(hovered.pixelColor(devicePixel(inBody, ratio)), theme::hover());
    EXPECT_EQ(hovered.pixelColor(devicePixel(inStrip, ratio)), theme::hover());
}

namespace {

// The view workspace and the dock chrome as MainWindow builds them, the
// theme's look on their window: each view's title bar opens with its kind
// switcher (ViewWorkspace::installChrome).
struct ThemedViews {
    // Before the window, so it outlives every widget drawn with it.
    std::unique_ptr<QStyle> style{theme::makeStyle()};
    katana::cad::Document document;
    QMainWindow window;
    ViewWorkspace* views = nullptr;
    DockChrome* chrome = nullptr;
    QToolButton* kind = nullptr;

    ThemedViews()
    {
        window.setStyle(style.get());
        window.setStyleSheet(theme::styleSheet());
        views = new ViewWorkspace(document, &window);
        window.setCentralWidget(views);
        chrome = new DockChrome(window);
        views->setChrome(chrome);
        window.resize(800, 600);
        window.show();
        katana::qt::test::processEvents();
        kind = window.findChild<QToolButton*>("ViewKindButton");
    }
    ThemedViews(const ThemedViews&) = delete;
    ThemedViews& operator=(const ThemedViews&) = delete;
};

} // namespace

TEST(TitleBarButtons, AViewsKindSwitcherDrawsItsArrowBesideItsIconNotOnIt)
{
    // The first button of every view's title bar opens a menu (Plan, 3D,
    // section, elevation), and its arrow sat on its icon's frame: 2 px over
    // at 100% and 3 at 125%, the owner's display. By hand at a ratio of 1,
    // from Qt 6.11's qstylesheetstyle.cpp and qfusionstyle.cpp: the button
    // is 34 x 22 (makeTitleBarButton's 22, widened to 34 by the workspace)
    // with a 1 px border, so its padding box is 1..32 across and 1..20 down.
    // The arrow's box is a ::menu-indicator's default, 13 x 13
    // (defaultSize), set right and centred in it by theme.cpp: 20..32
    // across, 1 + 20/2 - 13/2 = 5 down (QStyle::alignedRect). Fusion draws
    // its arrow 8 px wide and 8 x 8 / 14 = 4 deep, (13 - 8) / 2 = 2.5 px in
    // (qt_fusion_draw_arrow): 22.5..30.5, antialiased over columns 22..30.
    // The theme pads the button's right by that box's 13 px, so with the
    // 2 px on the left the contents are 3..19 across and 3..18 down, and the
    // 14 px icon is centred at 4..17 both ways - four pixels clear of the
    // arrow. Unpadded, the contents were 3..30 and the icon 10..23.
    ThemedViews built;
    ASSERT_NE(built.kind, nullptr) << "no view kind switcher";
    QToolButton& kind = *built.kind;
    ASSERT_NE(kind.menu(), nullptr);
    EXPECT_EQ(kind.size(), QSize(34, 22));
    const auto parts = drawnParts(kind);
    ASSERT_FALSE(parts.iconDevice.isNull()) << "no icon drawn";
    ASSERT_FALSE(parts.signDevice.isNull()) << "no arrow drawn";
    if (parts.ratio == 1.0) {
        EXPECT_EQ(parts.icon, QRect(4, 4, 14, 14));
        EXPECT_EQ(parts.sign.left(), 22);
        EXPECT_EQ(parts.sign.right(), 30);
    }
    EXPECT_FALSE(parts.signDevice.intersects(parts.iconDevice));
    // Apart, not merely not over: 4 px at a ratio of 1, and at any ratio at
    // least 3 px times it, taken down.
    EXPECT_GE(parts.signDevice.left() - parts.iconDevice.right() - 1,
              wholeDevicePixels(3, parts.ratio));
    // And the check sees it, clear, under its dock's name.
    QStringList checked;
    const QStringList clashes = menuSignClashes(built.window, &checked);
    EXPECT_TRUE(clashes.isEmpty()) << clashes.join("\n").toStdString();
    const QString dock = built.views->findChild<QDockWidget*>()->objectName();
    const QString line = "title bar button " + dock + " > ViewKindButton: sign ";
    EXPECT_TRUE(std::ranges::any_of(checked, [&](const QString& each) {
        return each.startsWith(line) && each.contains(" clear of its icon ");
    })) << checked.join("\n").toStdString();
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
        EXPECT_TRUE(clashes.front().startsWith(
            "toolbar button EditToolBar > editUndoButton: its menu's sign"))
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

TEST(ToolBarSigns, TheCheckSeesAViewKindSwitchersArrowDrawnOverItsIcon)
{
    // The check reaches the docks' title bars too. The kind switcher with
    // the 2 px of padding every title bar button has on its right, as it had
    // before theme.cpp padded a title bar button with a menu (a widget's own
    // stylesheet outranks its window's), is a clash under its dock's name.
    ThemedViews built;
    ASSERT_NE(built.kind, nullptr);
    built.kind->setStyleSheet("QToolButton { padding-right: 2px; }");
    katana::qt::test::processEvents();
    int buttons = 0;
    QStringList checked;
    const QStringList clashes = menuSignClashes(built.window, &checked, &buttons);
    EXPECT_GE(buttons, 1);
    const QString dock = built.views->findChild<QDockWidget*>()->objectName();
    ASSERT_EQ(clashes.size(), 1) << checked.join("\n").toStdString();
    EXPECT_TRUE(clashes.front().startsWith("title bar button " + dock +
                                           " > ViewKindButton: its menu's sign"))
        << clashes.front().toStdString();
    EXPECT_TRUE(clashes.front().contains("is drawn over its icon")) << clashes.front().toStdString();
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
        EXPECT_TRUE(line.startsWith("toolbar button DrawToolBar > toolFamilyButton.Draw."))
            << line.toStdString();
        EXPECT_EQ(line.contains("(device pixels at a ratio of"), ratio != 1.0) << line.toStdString();
    }
    // The boxes themselves, both ways: in device pixels apart, and on a
    // scaled screen meeting in the widget's.
    FlyoutButton* circle = built.family("Circle");
    ASSERT_NE(circle, nullptr);
    const auto parts = drawnParts(*circle);
    EXPECT_DOUBLE_EQ(parts.ratio, ratio);
    EXPECT_FALSE(parts.signDevice.intersects(parts.iconDevice));
    // They meet at 100, 125 and 200%; at 150% the mark's 7 px, taken down
    // from 7.5, leave a pixel between them (icon 9..38, mark 40..46).
    EXPECT_GE(parts.signDevice.left(), parts.iconDevice.right() + 1);
    if (ratio == 1.0) {
        EXPECT_EQ(parts.icon, parts.iconDevice);
        EXPECT_EQ(parts.sign, parts.signDevice);
    }
}

namespace {

// The 8-connected pieces of the pixels that are `ink` exactly.
int piecesOf(const QImage& image, const QColor& ink)
{
    const int width = image.width();
    const int height = image.height();
    std::vector<char> seen(static_cast<std::size_t>(width * height), 0);
    const auto isInk = [&](int x, int y) {
        return x >= 0 && y >= 0 && x < width && y < height && image.pixelColor(x, y) == ink;
    };
    int pieces = 0;
    for (int y = 0; y < height; ++y) {
        for (int x = 0; x < width; ++x) {
            if (!isInk(x, y) || seen[static_cast<std::size_t>(y * width + x)] != 0) {
                continue;
            }
            ++pieces;
            std::vector<QPoint> todo{QPoint(x, y)};
            seen[static_cast<std::size_t>(y * width + x)] = 1;
            while (!todo.empty()) {
                const QPoint at = todo.back();
                todo.pop_back();
                for (int dy = -1; dy <= 1; ++dy) {
                    for (int dx = -1; dx <= 1; ++dx) {
                        const int nx = at.x() + dx;
                        const int ny = at.y() + dy;
                        if (isInk(nx, ny) && seen[static_cast<std::size_t>(ny * width + nx)] == 0) {
                            seen[static_cast<std::size_t>(ny * width + nx)] = 1;
                            todo.emplace_back(nx, ny);
                        }
                    }
                }
            }
        }
    }
    return pieces;
}

// The Draw toolbar too short for its buttons: its overflow button shown.
QToolButton* overflowOf(DrawBar& built)
{
    built.window.resize(480, 300);
    katana::qt::test::processEvents();
    return built.bar->findChild<QToolButton*>("qt_toolbar_ext_button");
}

} // namespace

TEST(ToolBarOverflow, AToolbarTooShortForItsButtonsShowsItsWholeOverflowArrow)
{
    // The overflow button is 12 px deep (PM_ToolBarExtensionExtent); the
    // padding and border every tool button has left its arrow 2 px, a dot.
    // In the button's grab the arrow is its two chevrons, each whole: two
    // pieces of the muted colour exactly, neither touching the button's
    // edge, where a clipped arrow would.
    DrawBar built;
    QToolButton* overflow = overflowOf(built);
    ASSERT_NE(overflow, nullptr);
    ASSERT_TRUE(overflow->isVisible()) << "the Draw toolbar fitted in 300 px";
    const QImage drawn = shot(*overflow);
    EXPECT_EQ(piecesOf(drawn, theme::textMuted()), 2);
    for (int x = 0; x < drawn.width(); ++x) {
        EXPECT_NE(drawn.pixelColor(x, 0), theme::textMuted()) << x;
        EXPECT_NE(drawn.pixelColor(x, drawn.height() - 1), theme::textMuted()) << x;
    }
}

TEST(ToolBarOverflow, TheOverflowArrowIsPaintedInWholeDevicePixelsAtEveryScale)
{
    // Every pixel of the arrow is clear or the muted colour, at 100, 125,
    // 150 and 200% and at the sizes it is shown at (10 px deep in its
    // button, 12 as the style's extent), pointing down and right: two
    // chevrons, each a piece of its own. It was two pixmaps, 12 and 24 px,
    // antialiased; at 125% and 150% Qt scaled the 24 down, and the chevrons
    // came out in grey halos (21 colours in the 125% grab against 7 at
    // 100%).
    DrawBar built;
    QToolButton* overflow = overflowOf(built);
    ASSERT_NE(overflow, nullptr);
    const QIcon down = overflow->style()->standardIcon(QStyle::SP_ToolBarVerticalExtensionButton,
                                                       nullptr, overflow);
    const QIcon right = overflow->style()->standardIcon(
        QStyle::SP_ToolBarHorizontalExtensionButton, nullptr, overflow);
    for (const QIcon& arrow : {down, right}) {
        for (const QSize size : {QSize(20, 10), QSize(10, 20), QSize(12, 12)}) {
            for (const double ratio : {1.0, 1.25, 1.5, 2.0}) {
                const std::string where = std::to_string(size.width()) + "x" +
                                          std::to_string(size.height()) + " at " +
                                          QString::number(ratio).toStdString();
                const QImage image =
                    arrow.pixmap(size, ratio).toImage().convertToFormat(QImage::Format_ARGB32);
                int inked = 0;
                for (int y = 0; y < image.height(); ++y) {
                    for (int x = 0; x < image.width(); ++x) {
                        const QColor pixel = image.pixelColor(x, y);
                        if (pixel.alpha() == 0) {
                            continue;
                        }
                        ++inked;
                        EXPECT_EQ(pixel, theme::textMuted()) << where << " at " << x << "," << y;
                    }
                }
                EXPECT_GT(inked, 0) << where;
                EXPECT_EQ(piecesOf(image, theme::textMuted()), 2) << where;
            }
        }
    }
}

TEST(ToolBarOverflow, TheArrowIsLitWhileAToolWhoseButtonItHidesRuns)
{
    // At the window's first size the Draw toolbar hides the Vertices family
    // behind its overflow arrow, and with Delete Vertex running nothing on
    // the toolbars said a tool ran: the family's mark and frame were out of
    // sight. The arrow's chevrons take the accent then, and only then - not
    // for a tool whose button shows, whose own frame says it, and not once
    // the window is tall enough to show the family.
    DrawBar built;
    QToolButton* overflow = overflowOf(built);
    ASSERT_NE(overflow, nullptr);
    ASSERT_TRUE(overflow->isVisible()) << "the Draw toolbar fitted in 300 px";
    FlyoutButton* vertices = built.family("Vertices");
    ASSERT_NE(vertices, nullptr);
    ASSERT_TRUE(vertices->isHidden()) << "the Vertices family fitted in 300 px";
    const auto chevrons = [&](const QColor& ink) { return piecesOf(shot(*overflow), ink); };
    EXPECT_EQ(chevrons(theme::textMuted()), 2);

    QAction* remove = built.actions.action("draw.vertex.delete");
    ASSERT_NE(remove, nullptr);
    remove->setChecked(true);
    katana::qt::test::processEvents();
    EXPECT_EQ(chevrons(theme::accent()), 2) << "not lit with Delete Vertex running";
    EXPECT_EQ(chevrons(theme::textMuted()), 0);

    // A tool on a button the toolbar shows: the first shown.
    QAction* shown = nullptr;
    for (QAction* action : built.bar->actions()) {
        QWidget* button = built.bar->widgetForAction(action);
        if (built.actions.action(action->objectName().toStdString()) == action &&
            button != nullptr && !button->isHidden()) {
            shown = action;
            break;
        }
    }
    ASSERT_NE(shown, nullptr) << "no tool's button shows";
    shown->setChecked(true); // and Delete Vertex stops: one tool runs at a time
    katana::qt::test::processEvents();
    EXPECT_FALSE(remove->isChecked());
    EXPECT_EQ(chevrons(theme::textMuted()), 2) << "lit for " << shown->objectName().toStdString();

    // Tall enough for every button, the family shows and the arrow goes.
    remove->setChecked(true);
    katana::qt::test::processEvents();
    EXPECT_EQ(chevrons(theme::accent()), 2);
    built.window.resize(480, 2400);
    katana::qt::test::processEvents();
    EXPECT_FALSE(vertices->isHidden());
    EXPECT_FALSE(overflow->property(theme::kToolRunsBehindOverflow).toBool());
    remove->setChecked(false);
}

TEST(ToolBarOverflow, TheArrowIsNotLitForACheckedButtonBehindItThatIsNoTool)
{
    // The window's Select heads the Draw toolbar, checkable and checked
    // while NO tool runs; a toolbar short enough hides it too. Only a tool's
    // button lights the arrow. Here the plain button at the bar's end, a
    // checkable action that is no tool, stands for it: checked behind the
    // arrow, then a tool started and stopped so the arrow is worked out
    // again, it must leave the arrow muted.
    DrawBar built;
    QToolButton* overflow = overflowOf(built);
    ASSERT_NE(overflow, nullptr);
    ASSERT_TRUE(built.twin->isHidden()) << "the plain button fitted in 300 px";
    QAction* notATool = built.twin->defaultAction();
    ASSERT_EQ(built.actions.action(notATool->objectName().toStdString()), nullptr);
    notATool->setChecked(true);
    QAction* remove = built.actions.action("draw.vertex.delete");
    ASSERT_NE(remove, nullptr);
    remove->setChecked(true);
    katana::qt::test::processEvents();
    EXPECT_EQ(piecesOf(shot(*overflow), theme::accent()), 2) << "not lit with Delete Vertex running";
    remove->setChecked(false);
    katana::qt::test::processEvents();
    EXPECT_FALSE(overflow->property(theme::kToolRunsBehindOverflow).toBool());
    EXPECT_EQ(piecesOf(shot(*overflow), theme::textMuted()), 2) << "lit for a button that is no tool";
    notATool->setChecked(false);
}

TEST(ToolBarOverflow, TheArrowIsLitForAToolHiddenWhileTheToolbarWasOffTheScreen)
{
    // The toolbar lays its buttons out as the window is shown, before the
    // buttons are on the screen, and hide() then sends a button no Hide
    // event (QWidget sends one only to a widget that was visible). A tool
    // running when the window is shown short - or its toolbar shown again -
    // must light the arrow all the same: the overflow button's own Show,
    // after that layout, works it out.
    DrawBar built;
    QToolButton* overflow = built.bar->findChild<QToolButton*>("qt_toolbar_ext_button");
    FlyoutButton* vertices = built.family("Vertices");
    ASSERT_NE(vertices, nullptr);
    QAction* remove = built.actions.action("draw.vertex.delete");
    ASSERT_NE(remove, nullptr);
    built.window.hide();
    katana::qt::test::processEvents();
    remove->setChecked(true);
    built.window.resize(480, 300);
    built.window.show();
    katana::qt::test::processEvents();
    overflow = built.bar->findChild<QToolButton*>("qt_toolbar_ext_button");
    ASSERT_NE(overflow, nullptr);
    ASSERT_TRUE(overflow->isVisible()) << "the Draw toolbar fitted in 300 px";
    ASSERT_TRUE(vertices->isHidden()) << "the Vertices family fitted in 300 px";
    EXPECT_TRUE(overflow->property(theme::kToolRunsBehindOverflow).toBool());
    EXPECT_EQ(piecesOf(shot(*overflow), theme::accent()), 2);
    remove->setChecked(false);
}

TEST(ToolBarOverflow, AnExpandedToolbarsOverflowButtonHasTheOutlineEveryCheckedButtonHas)
{
    // The overflow button is checked while its toolbar is expanded, and a
    // checked tool button is outlined in the accent (QToolButton:checked).
    // The theme's rule for the overflow button took its border away to make
    // room for the arrow, and with it the outline: a plain grey pill. It
    // keeps its border now. Checked by hand here, as QToolBarLayout checks
    // it when the button is clicked; the border's top row is the accent.
    DrawBar built;
    QToolButton* overflow = overflowOf(built);
    ASSERT_NE(overflow, nullptr);
    ASSERT_TRUE(overflow->isCheckable());
    const int middle = shot(*overflow).width() / 2;
    EXPECT_NE(shot(*overflow).pixelColor(middle, 0), theme::accent()) << "outlined at rest";
    overflow->setChecked(true);
    katana::qt::test::processEvents();
    EXPECT_EQ(shot(*overflow).pixelColor(middle, 0), theme::accent());
    overflow->setChecked(false);
}
