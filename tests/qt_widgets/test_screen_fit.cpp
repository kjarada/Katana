// A window never runs off its screen (src/katana_qt/screen_fit.hpp, theme.cpp):
// a dialog taller or wider than the room its screen leaves scrolls rather than
// losing its bottom, a dialog that fits is left as written, and a menu taller than
// the screen is one scrolling column.
//
// The offscreen platform's screen is 800 x 600, so the room is 784 x 584 less a
// frame: every dialog here is built far over it, and a menu of a hundred items is
// over it at any text size. Nothing is measured in pixels of ink: sizes are
// compared with the screen's own, and a control is "reachable" when scrolling brings
// it inside the viewport of the scroll area that holds it.

#include <gtest/gtest.h>

#include <QAction>
#include <QApplication>
#include <QDialog>
#include <QHBoxLayout>
#include <QLabel>
#include <QMenu>
#include <QMessageBox>
#include <QPushButton>
#include <QScreen>
#include <QScrollArea>
#include <QScrollBar>
#include <QStyle>
#include <QVBoxLayout>

#include <memory>

#include "screen_fit.hpp"
#include "theme.hpp"

namespace theme = katana::qt::theme;
using katana::qt::kScreenFitScroll;

namespace {

// The application-wide filter, for one test: deleting it takes it off, so the other
// widget tests keep the dialogs they were written against.
struct FilterGuard {
    QObject* filter = katana::qt::installScreenFit(*qApp);
    ~FilterGuard() { delete filter; }
};

QRect screenRoom()
{
    return QGuiApplication::primaryScreen()->availableGeometry();
}

// A dialog of `rows` buttons, each at least `height` high, one above another - the
// shape of Terrain > Volumes or a manager with a form of fields: the layout's own
// minimum is the sum.
std::unique_ptr<QDialog> tallDialog(int rows, int height, QPushButton** last = nullptr)
{
    auto dialog = std::make_unique<QDialog>();
    dialog->setObjectName("tallDialog");
    auto* layout = new QVBoxLayout(dialog.get());
    QPushButton* button = nullptr;
    for (int i = 0; i < rows; ++i) {
        button = new QPushButton(QString("Row %1").arg(i), dialog.get());
        button->setObjectName(QString("row%1").arg(i));
        button->setMinimumHeight(height);
        layout->addWidget(button);
    }
    if (last != nullptr) {
        *last = button;
    }
    return dialog;
}

// Whether `widget`, scrolled to, lies inside the viewport of `scroll`.
bool reachable(QScrollArea& scroll, QWidget& widget)
{
    scroll.ensureWidgetVisible(&widget, 0, 0);
    QApplication::processEvents();
    const QRect viewport = scroll.viewport()->rect();
    const QRect where(widget.mapTo(scroll.viewport(), QPoint(0, 0)), widget.size());
    return viewport.contains(where);
}

} // namespace

TEST(ScreenFit, ADialogThatFitsIsLeftExactlyAsItWasWritten)
{
    const FilterGuard guard;
    QDialog dialog;
    auto* layout = new QVBoxLayout(&dialog);
    for (int i = 0; i < 3; ++i) {
        layout->addWidget(new QPushButton(QString("Row %1").arg(i), &dialog));
    }
    QLayout* const written = dialog.layout();
    dialog.show();
    QApplication::processEvents();
    EXPECT_EQ(dialog.findChild<QScrollArea*>(kScreenFitScroll), nullptr)
        << "a dialog that fits is given no scroll area";
    EXPECT_EQ(dialog.layout(), written) << "and keeps the layout it was written with";
    EXPECT_FALSE(katana::qt::fitDialogToScreen(dialog));
    EXPECT_LE(dialog.height(), screenRoom().height());
    EXPECT_LE(dialog.width(), screenRoom().width());
}

TEST(ScreenFit, ADialogTallerThanTheScreenScrollsAndEveryControlIsReachable)
{
    const FilterGuard guard;
    QPushButton* last = nullptr;
    // Eighty rows of 40 px: 3,200 px of controls on a 600 px screen.
    const auto dialog = tallDialog(80, 40, &last);
    ASSERT_GT(dialog->minimumSizeHint().height(), 4 * screenRoom().height())
        << "the premise: its controls need far more room than there is";
    dialog->show();
    QApplication::processEvents();

    auto* scroll = dialog->findChild<QScrollArea*>(kScreenFitScroll);
    ASSERT_NE(scroll, nullptr) << "the controls did not fit, so they were given a scroll area";
    // The window is as large as the screen lets it be, and no larger.
    EXPECT_LE(dialog->height(), screenRoom().height());
    EXPECT_LE(dialog->width(), screenRoom().width());
    EXPECT_GE(dialog->height(), screenRoom().height() / 2) << "not collapsed to its title bar";
    EXPECT_GT(scroll->verticalScrollBar()->maximum(), 0) << "there is something to scroll to";

    // Every control: the first, the last, and one in the middle, found as they were -
    // by name in the dialog, whose child they still are through the scroll area.
    for (const char* name : {"row0", "row39", "row79"}) {
        auto* row = dialog->findChild<QPushButton*>(name);
        ASSERT_NE(row, nullptr) << name;
        EXPECT_EQ(row->window(), dialog.get()) << name;
        EXPECT_TRUE(reachable(*scroll, *row)) << name << " cannot be scrolled into view";
    }
    EXPECT_EQ(dialog->findChildren<QScrollArea*>(kScreenFitScroll).size(), 1);
}

TEST(ScreenFit, ADialogWiderThanTheScreenScrollsAcrossAsWell)
{
    const FilterGuard guard;
    QDialog dialog;
    // A field far wider than the screen, and a button at the end of the row: the
    // button is reachable only by scrolling across.
    auto* row = new QHBoxLayout(&dialog);
    auto* wide = new QLabel("a field far wider than the screen", &dialog);
    wide->setMinimumWidth(3000);
    auto* far = new QPushButton("Far", &dialog);
    row->addWidget(wide);
    row->addWidget(far);
    dialog.show();
    QApplication::processEvents();
    auto* scroll = dialog.findChild<QScrollArea*>(kScreenFitScroll);
    ASSERT_NE(scroll, nullptr);
    EXPECT_LE(dialog.width(), screenRoom().width());
    EXPECT_GT(scroll->horizontalScrollBar()->maximum(), 0);
    EXPECT_TRUE(reachable(*scroll, *far));
}

TEST(ScreenFit, ADialogShownAgainIsNotWrappedAgain)
{
    const FilterGuard guard;
    const auto dialog = tallDialog(60, 40);
    for (int i = 0; i < 3; ++i) {
        dialog->show();
        QApplication::processEvents();
        dialog->hide();
    }
    EXPECT_EQ(dialog->findChildren<QScrollArea*>(kScreenFitScroll).size(), 1);
    EXPECT_NE(dialog->findChild<QPushButton*>("row59"), nullptr);
}

TEST(ScreenFit, FittingWithoutTheFilterDoesTheSameAsTheFilterDoes)
{
    // The function is the whole of it; the filter only calls it as a dialog is shown.
    const auto dialog = tallDialog(60, 40);
    EXPECT_TRUE(katana::qt::fitDialogToScreen(*dialog));
    EXPECT_NE(dialog->findChild<QScrollArea*>(kScreenFitScroll), nullptr);
    EXPECT_LE(dialog->height(), screenRoom().height());
}

TEST(ScreenFit, ADialogPlacedOffTheScreenByHandIsBroughtBackOnto)
{
    const FilterGuard guard;
    QDialog dialog;
    auto* layout = new QVBoxLayout(&dialog);
    layout->addWidget(new QPushButton("Close", &dialog));
    const QRect room = screenRoom();
    dialog.move(room.right() + 400, room.bottom() + 400);
    dialog.show();
    QApplication::processEvents();
    EXPECT_TRUE(room.contains(dialog.frameGeometry()))
        << "placed at " << dialog.x() << "," << dialog.y();
}

TEST(ScreenFit, QtsOwnDialogsKeepTheirLayoutsToThemselves)
{
    QMessageBox box;
    box.setText("A message");
    QLayout* const theirs = box.layout();
    EXPECT_FALSE(katana::qt::fitDialogToScreen(box));
    EXPECT_EQ(box.layout(), theirs);
    EXPECT_EQ(box.findChild<QScrollArea*>(kScreenFitScroll), nullptr);
}

TEST(ScreenFit, TheRoomIsInsideTheScreensAvailableAreaWithAMarginRoundIt)
{
    QWidget window;
    const QRect room = katana::qt::roomOnScreen(window);
    const QRect available = screenRoom();
    EXPECT_TRUE(available.contains(room));
    EXPECT_LT(room.width(), available.width());
    EXPECT_LT(room.height(), available.height());
    EXPECT_GT(katana::qt::frameOf(window).height(), 0) << "a title bar is allowed for, shown or not";
}

// ---- menus -------------------------------------------------------------------------

TEST(ScreenFit, AMenuTallerThanTheScreenIsOneScrollingColumn)
{
    // The style and the stylesheet the application has, put on the one menu.
    const std::unique_ptr<QStyle> style(theme::makeStyle());
    ASSERT_EQ(style->styleHint(QStyle::SH_Menu_Scrollable), 1);

    QMenu menu;
    menu.setStyle(style.get());
    menu.setStyleSheet(theme::styleSheet());
    EXPECT_EQ(menu.style()->styleHint(QStyle::SH_Menu_Scrollable, nullptr, &menu), 1)
        << "the stylesheet's rules must leave the hint to the style";
    QAction* first = menu.addAction("Item 0");
    QAction* last = nullptr;
    for (int i = 1; i < 120; ++i) {
        last = menu.addAction(QString("Item %1").arg(i));
    }
    ASSERT_NE(last, nullptr);
    const QRect room = screenRoom();
    menu.popup(room.topLeft() + QPoint(20, 20));
    QApplication::processEvents();

    // As large as the screen lets it be, not as large as its items.
    EXPECT_LE(menu.height(), room.height());
    EXPECT_GT(menu.sizeHint().height(), room.height()) << "the premise: it does not fit";
    // One column: the first and the last item are one above the other, so a reader
    // goes down it and not across a wall of columns.
    EXPECT_EQ(menu.actionGeometry(first).left(), menu.actionGeometry(last).left());
    EXPECT_LT(menu.width(), room.width() / 2);
    // And the last is reachable: the cursor keys, the wheel and the arrows scroll it
    // into the menu.
    menu.setActiveAction(last);
    QApplication::processEvents();
    EXPECT_TRUE(menu.rect().contains(menu.actionGeometry(last)))
        << "the last item is off the menu after scrolling to it";
    menu.setActiveAction(first);
    QApplication::processEvents();
    EXPECT_TRUE(menu.rect().contains(menu.actionGeometry(first)));
    menu.hide();
}

TEST(ScreenFit, AMenuThatFitsIsNotScrolled)
{
    const std::unique_ptr<QStyle> style(theme::makeStyle());
    QMenu menu;
    menu.setStyle(style.get());
    menu.setStyleSheet(theme::styleSheet());
    QAction* first = menu.addAction("One");
    QAction* last = menu.addAction("Two");
    menu.popup(screenRoom().topLeft() + QPoint(20, 20));
    QApplication::processEvents();
    EXPECT_EQ(menu.height(), menu.sizeHint().height());
    EXPECT_TRUE(menu.rect().contains(menu.actionGeometry(first)));
    EXPECT_TRUE(menu.rect().contains(menu.actionGeometry(last)));
    menu.hide();
}
