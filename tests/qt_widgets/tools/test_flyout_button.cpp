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

#include <cstdlib>
#include <memory>
#include <string>
#include <vector>

#include <QAction>
#include <QApplication>
#include <QContextMenuEvent>
#include <QElapsedTimer>
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
    // the same icon, at rest, under the pointer and running. They may differ
    // only inside the mark's square. As a split button the family differed
    // over the icon's right third, where the arrow's strip was painted on it.
    DrawBar built;
    FlyoutButton* circle = built.family("Circle");
    ASSERT_NE(circle, nullptr);
    ASSERT_NE(built.twin, nullptr);
    ASSERT_EQ(circle->size(), built.twin->size());
    QAction* running = circle->defaultAction();
    QAction* plainRunning = built.twin->defaultAction();
    ASSERT_NE(plainRunning, nullptr);
    for (const char* state : {"at rest", "under the pointer", "running"}) {
        const bool hover = std::string(state) == "under the pointer";
        const bool checked = std::string(state) == "running";
        for (QToolButton* each : {static_cast<QToolButton*>(circle), built.twin}) {
            each->setAttribute(Qt::WA_UnderMouse, hover);
        }
        running->setChecked(checked);
        plainRunning->setChecked(checked);
        katana::qt::test::processEvents();
        const QRect differs = differenceBox(shot(*circle), shot(*built.twin));
        EXPECT_FALSE(differs.isNull()) << state << ": the mark is not drawn";
        EXPECT_TRUE(circle->markRect().contains(differs))
            << state << ": the family differs from the plain button at x " << differs.left()
            << ".." << differs.right() << " y " << differs.top() << ".." << differs.bottom();
    }
    for (QToolButton* each : {static_cast<QToolButton*>(circle), built.twin}) {
        each->setAttribute(Qt::WA_UnderMouse, false);
    }
    running->setChecked(false);
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

TEST(ToolFamilyButtons, TheMarkIsLitInTheAccentOnlyWhileThePointerIsOnItsCorner)
{
    // The corner opens the family where the rest of the button runs the
    // tool, so the mark says which the pointer is over, as a split button's
    // strip lights up under it. Its right-angled corner pixel is inked by
    // every row of the triangle, and is the mark's colour exactly.
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

TEST(ToolFamilyButtons, TheTooltipNamesTheFamilysOtherToolsAndHowToReachThem)
{
    DrawBar built;
    FlyoutButton* circle = built.family("Circle");
    ASSERT_NE(circle, nullptr);
    const QString tip = circle->toolTipWithMenu();
    // The tool's own first - its name and aliases, as every tool's - and the
    // property itself left the tool's, as setDefaultAction keeps setting it.
    EXPECT_EQ(circle->toolTip(), circle->defaultAction()->toolTip());
    EXPECT_TRUE(tip.startsWith(circle->toolTip())) << tip.toStdString();
    EXPECT_TRUE(tip.contains("Hold, right-click or press the corner triangle"))
        << tip.toStdString();
    const QString others = tip.mid(tip.indexOf("corner triangle"));
    int listed = 0;
    for (const QAction* item : circle->menu()->actions()) {
        const QString text = QString(item->text()).remove('&').toHtmlEscaped();
        if (item == circle->defaultAction()) {
            EXPECT_FALSE(others.contains(text)) << "the tool the click runs is not an other";
        } else {
            EXPECT_TRUE(others.contains(text)) << text.toStdString();
            ++listed;
        }
    }
    EXPECT_EQ(listed, circle->menu()->actions().size() - 1);
}

TEST(SplitButtons, UndosArrowHasAStripOfItsOwnBesideTheIcon)
{
    // Undo as MainWindow::buildActions makes it, on a horizontal toolbar with
    // the theme. The strip the menu opens from, and the arrow drawn in it,
    // are clear of the icon's square: the stylesheet pads the split button
    // by the strip's width. Without that rule the strip was the button's
    // right 12 px, over the icon.
    const std::unique_ptr<QStyle> style(theme::makeStyle());
    QMainWindow window;
    window.setStyle(style.get());
    window.setStyleSheet(theme::styleSheet());
    window.setCentralWidget(new QWidget);
    auto* bar = new QToolBar("Edit", &window);
    bar->setObjectName("EditToolBar");
    bar->setIconSize(QSize(20, 20));
    window.addToolBar(Qt::TopToolBarArea, bar);
    auto* undo = new QAction(katana::qt::icon(katana::qt::Icon::Undo), "Undo", &window);
    int undone = 0;
    QObject::connect(undo, &QAction::triggered, &window, [&undone] { ++undone; });
    auto* button = new QToolButton(bar);
    button->setObjectName("editUndoButton");
    button->setDefaultAction(undo);
    button->setPopupMode(QToolButton::MenuButtonPopup);
    button->setAutoRaise(true);
    button->setIconSize(bar->iconSize());
    auto* history = new QMenu(button);
    history->addAction("1 CREATE_POINT");
    button->setMenu(history);
    bar->addWidget(button);
    window.resize(640, 200);
    window.show();
    katana::qt::test::processEvents();

    const auto parts = drawnParts(*button);
    ASSERT_FALSE(parts.icon.isNull());
    ASSERT_FALSE(parts.sign.isNull()) << "no arrow drawn";
    EXPECT_FALSE(parts.sign.intersects(parts.icon));

    QStyleOptionToolButton option;
    option.initFrom(button);
    option.rect = button->rect();
    option.subControls = QStyle::SC_ToolButton | QStyle::SC_ToolButtonMenu;
    option.features = QStyleOptionToolButton::MenuButtonPopup | QStyleOptionToolButton::HasMenu;
    option.iconSize = bar->iconSize();
    const QRect strip =
        button->style()->subControlRect(QStyle::CC_ToolButton, &option, QStyle::SC_ToolButtonMenu, button);
    ASSERT_TRUE(strip.isValid());
    EXPECT_FALSE(strip.intersects(parts.icon));
    EXPECT_TRUE(strip.contains(parts.sign));

    // A click on the icon's right edge undoes; the strip opens the history.
    const MenuWatch watch(*history);
    click(*button, QPoint(parts.icon.right(), parts.icon.center().y()));
    EXPECT_EQ(undone, 1);
    EXPECT_EQ(watch.shown, 0);
    click(*button, strip.center());
    EXPECT_EQ(watch.shown, 1);
    EXPECT_EQ(undone, 1);
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
