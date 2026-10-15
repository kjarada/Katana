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
#include <QDialogButtonBox>
#include <QGridLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QMenu>
#include <QMessageBox>
#include <QPlainTextEdit>
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
using katana::qt::kScreenFitFooter;
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

// `rows` buttons one above another, 40 px each, laid into `layout`: the body of a dialog
// far taller than the screen, which scrolls.
void addBody(QDialog& dialog, QBoxLayout& layout, int rows, const QString& prefix = "body")
{
    for (int i = 0; i < rows; ++i) {
        auto* button = new QPushButton(QString("%1 %2").arg(prefix).arg(i), &dialog);
        button->setObjectName(QString("%1%2").arg(prefix).arg(i));
        button->setMinimumHeight(40);
        layout.addWidget(button);
    }
}

// Where `widget` is on the dialog, so that one in the footer stays put as the body
// scrolls and one in the body does not.
QRect placeIn(QDialog& dialog, QWidget& widget)
{
    return QRect(widget.mapTo(&dialog, QPoint(0, 0)), widget.size());
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

// ---- the footer: a dialog's own buttons do not scroll ------------------------------------

TEST(ScreenFit, ADialogsButtonBoxStaysBelowTheScrollAreaAndInSightWhateverIsScrolled)
{
    const FilterGuard guard;
    QDialog dialog;
    auto* layout = new QVBoxLayout(&dialog);
    addBody(dialog, *layout, 80);
    auto* buttons = new QDialogButtonBox(&dialog);
    auto* close = buttons->addButton(QDialogButtonBox::Close);
    close->setObjectName("footerClose");
    layout->addWidget(buttons);
    dialog.show();
    QApplication::processEvents();

    auto* scroll = dialog.findChild<QScrollArea*>(kScreenFitScroll);
    ASSERT_NE(scroll, nullptr) << "3,200 px of rows do not fit a screen";
    auto* footer = dialog.findChild<QWidget*>(kScreenFitFooter);
    ASSERT_NE(footer, nullptr) << "the button box is the dialog's footer";
    EXPECT_TRUE(footer->isAncestorOf(close));
    EXPECT_FALSE(scroll->isAncestorOf(footer)) << "the footer is beside the scroll area";
    EXPECT_FALSE(scroll->isAncestorOf(close));
    EXPECT_EQ(close->window(), &dialog);
    EXPECT_EQ(dialog.findChild<QPushButton*>("footerClose"), close)
        << "found by name in the dialog, as before";
    ASSERT_TRUE(close->isVisible());
    EXPECT_TRUE(dialog.rect().contains(placeIn(dialog, *close)))
        << "the button is on the dialog without scrolling";

    // The body scrolls from its top to its bottom and the button does not move.
    const QRect resting = placeIn(dialog, *close);
    ASSERT_GT(scroll->verticalScrollBar()->maximum(), 0);
    for (const int value : {scroll->verticalScrollBar()->maximum(), 0}) {
        scroll->verticalScrollBar()->setValue(value);
        QApplication::processEvents();
        EXPECT_EQ(placeIn(dialog, *close), resting) << "scrolled to " << value;
    }
    // And all of the body is still reached by scrolling.
    for (const char* name : {"body0", "body40", "body79"}) {
        auto* row = dialog.findChild<QPushButton*>(name);
        ASSERT_NE(row, nullptr) << name;
        EXPECT_TRUE(scroll->isAncestorOf(row)) << name;
        EXPECT_TRUE(reachable(*scroll, *row)) << name;
    }
    EXPECT_EQ(dialog.findChildren<QWidget*>(kScreenFitFooter).size(), 1);
}

TEST(ScreenFit, AButtonRowAndTheReplyUnderItAreTheFooterTogether)
{
    // The shape of Terrain > Edit Surface's dialogs: a body, a row of Preview, Run and
    // Close, and under it the box that says what Run did. A reply scrolled out of sight
    // after pressing a button that never scrolls would look like the button did nothing.
    const FilterGuard guard;
    QDialog dialog;
    auto* layout = new QVBoxLayout(&dialog);
    addBody(dialog, *layout, 80);
    auto* row = new QHBoxLayout();
    auto* preview = new QPushButton("Preview", &dialog);
    auto* run = new QPushButton("Run", &dialog);
    auto* close = new QPushButton("Close", &dialog);
    row->addWidget(preview);
    row->addStretch(1);
    row->addWidget(run);
    row->addWidget(close);
    layout->addLayout(row);
    auto* reply = new QPlainTextEdit(&dialog);
    reply->setMinimumHeight(60);
    layout->addWidget(reply);
    dialog.show();
    QApplication::processEvents();

    auto* scroll = dialog.findChild<QScrollArea*>(kScreenFitScroll);
    auto* footer = dialog.findChild<QWidget*>(kScreenFitFooter);
    ASSERT_NE(scroll, nullptr);
    ASSERT_NE(footer, nullptr);
    for (QWidget* widget : std::initializer_list<QWidget*>{preview, run, close, reply}) {
        EXPECT_TRUE(footer->isAncestorOf(widget)) << widget->metaObject()->className();
        EXPECT_FALSE(scroll->isAncestorOf(widget)) << widget->metaObject()->className();
        EXPECT_TRUE(widget->isVisible());
        EXPECT_TRUE(dialog.rect().contains(placeIn(dialog, *widget)))
            << widget->metaObject()->className() << " is off the dialog";
    }
    // Their order is the one they were written in: the row above its reply.
    EXPECT_LT(placeIn(dialog, *run).bottom(), placeIn(dialog, *reply).top());
    EXPECT_LT(placeIn(dialog, *preview).right(), placeIn(dialog, *run).left());
    EXPECT_LT(placeIn(dialog, *run).right(), placeIn(dialog, *close).left());
    // The footer is the bottom of the dialog and the body is above it.
    EXPECT_LT(scroll->geometry().bottom(), footer->geometry().top());
    // A button of the footer still works, by its own signal.
    int pressed = 0;
    QObject::connect(run, &QPushButton::clicked, &dialog, [&pressed] { ++pressed; });
    run->click();
    EXPECT_EQ(pressed, 1);
}

TEST(ScreenFit, OnlyTheLastRowOfButtonsIsTheFooterAndAnEarlierOneScrollsWithTheBody)
{
    const FilterGuard guard;
    QDialog dialog;
    auto* layout = new QVBoxLayout(&dialog);
    auto* tools = new QHBoxLayout();
    auto* add = new QPushButton("Add", &dialog);
    auto* remove = new QPushButton("Remove", &dialog);
    tools->addWidget(add);
    tools->addWidget(remove);
    layout->addLayout(tools);
    addBody(dialog, *layout, 80);
    auto* last = new QHBoxLayout();
    auto* ok = new QPushButton("OK", &dialog);
    auto* cancel = new QPushButton("Cancel", &dialog);
    last->addStretch(1);
    last->addWidget(ok);
    last->addWidget(cancel);
    layout->addLayout(last);
    dialog.show();
    QApplication::processEvents();

    auto* scroll = dialog.findChild<QScrollArea*>(kScreenFitScroll);
    auto* footer = dialog.findChild<QWidget*>(kScreenFitFooter);
    ASSERT_NE(scroll, nullptr);
    ASSERT_NE(footer, nullptr);
    EXPECT_TRUE(footer->isAncestorOf(ok));
    EXPECT_TRUE(footer->isAncestorOf(cancel));
    EXPECT_TRUE(scroll->isAncestorOf(add)) << "the first row is the body's, not the footer's";
    EXPECT_TRUE(scroll->isAncestorOf(remove));
    EXPECT_TRUE(reachable(*scroll, *add));
}

TEST(ScreenFit, AReadOnlyLineBesideTheButtonsIsPartOfTheirRow)
{
    // BIM > Road Objects shows the line Add will run in a read-only field at the head
    // of its button row, with the reply and a status line under it.
    const FilterGuard guard;
    QDialog dialog;
    auto* layout = new QVBoxLayout(&dialog);
    addBody(dialog, *layout, 80);
    auto* row = new QHBoxLayout();
    auto* line = new QLineEdit("ROADOBJECT ADD sign", &dialog);
    line->setReadOnly(true);
    auto* add = new QPushButton("Add", &dialog);
    auto* close = new QPushButton("Close", &dialog);
    row->addWidget(line, 1);
    row->addWidget(add);
    row->addWidget(close);
    layout->addLayout(row);
    auto* status = new QLabel("nothing yet", &dialog);
    layout->addWidget(status);
    dialog.show();
    QApplication::processEvents();

    auto* scroll = dialog.findChild<QScrollArea*>(kScreenFitScroll);
    auto* footer = dialog.findChild<QWidget*>(kScreenFitFooter);
    ASSERT_NE(scroll, nullptr);
    ASSERT_NE(footer, nullptr);
    for (QWidget* widget : std::initializer_list<QWidget*>{line, add, close, status}) {
        EXPECT_TRUE(footer->isAncestorOf(widget)) << widget->metaObject()->className();
        EXPECT_TRUE(dialog.rect().contains(placeIn(dialog, *widget)))
            << widget->metaObject()->className();
    }
}

TEST(ScreenFit, ADialogWithNoRowOfButtonsHasNoFooterAndEverythingScrolls)
{
    // The tall dialog the other tests use: a lone button laid into the layout is a
    // control, not a row of them, even as the last item.
    const FilterGuard guard;
    QPushButton* last = nullptr;
    const auto dialog = tallDialog(80, 40, &last);
    dialog->show();
    QApplication::processEvents();
    auto* scroll = dialog->findChild<QScrollArea*>(kScreenFitScroll);
    ASSERT_NE(scroll, nullptr);
    EXPECT_EQ(dialog->findChild<QWidget*>(kScreenFitFooter), nullptr);
    EXPECT_TRUE(scroll->isAncestorOf(last));
}

TEST(ScreenFit, AButtonRowThatIsNotLastAndIsFollowedByABigListIsNoFooter)
{
    // Buttons in the middle of a dialog, a long list under them: a footer would be the
    // list and more than the room can spare, so the dialog scrolls as a whole.
    const FilterGuard guard;
    QDialog dialog;
    auto* layout = new QVBoxLayout(&dialog);
    addBody(dialog, *layout, 10, "head");
    auto* row = new QHBoxLayout();
    auto* ok = new QPushButton("OK", &dialog);
    row->addWidget(ok);
    row->addStretch(1);
    layout->addLayout(row);
    addBody(dialog, *layout, 60, "list");
    dialog.show();
    QApplication::processEvents();

    auto* scroll = dialog.findChild<QScrollArea*>(kScreenFitScroll);
    ASSERT_NE(scroll, nullptr);
    EXPECT_EQ(dialog.findChild<QWidget*>(kScreenFitFooter), nullptr)
        << "60 rows of 40 px are no footer";
    EXPECT_TRUE(scroll->isAncestorOf(ok));
    EXPECT_TRUE(reachable(*scroll, *ok));
    auto* lastRow = dialog.findChild<QPushButton*>("list59");
    ASSERT_NE(lastRow, nullptr);
    EXPECT_TRUE(reachable(*scroll, *lastRow));
}

TEST(ScreenFit, ARowWithAFieldInItIsAFormsOwnAndNotTheFooter)
{
    // A path and its Browse button: the row ends the dialog but holds a field, so it is
    // not the dialog's buttons.
    const FilterGuard guard;
    QDialog dialog;
    auto* layout = new QVBoxLayout(&dialog);
    addBody(dialog, *layout, 80);
    auto* row = new QHBoxLayout();
    auto* path = new QLineEdit(&dialog);
    auto* browse = new QPushButton("Browse", &dialog);
    row->addWidget(path);
    row->addWidget(browse);
    layout->addLayout(row);
    dialog.show();
    QApplication::processEvents();
    auto* scroll = dialog.findChild<QScrollArea*>(kScreenFitScroll);
    ASSERT_NE(scroll, nullptr);
    EXPECT_EQ(dialog.findChild<QWidget*>(kScreenFitFooter), nullptr);
    EXPECT_TRUE(scroll->isAncestorOf(browse));
}

TEST(ScreenFit, ADialogLaidOutInAGridKeepsItsButtonsInTheScrollArea)
{
    // Only a vertical box has a footer: in a grid the buttons are where the grid put
    // them, in a cell the controls above share.
    const FilterGuard guard;
    QDialog dialog;
    auto* grid = new QGridLayout(&dialog);
    for (int i = 0; i < 80; ++i) {
        auto* button = new QPushButton(QString("Cell %1").arg(i), &dialog);
        button->setMinimumHeight(40);
        grid->addWidget(button, i, 0);
    }
    auto* buttons = new QDialogButtonBox(QDialogButtonBox::Close, &dialog);
    grid->addWidget(buttons, 80, 0);
    dialog.show();
    QApplication::processEvents();
    auto* scroll = dialog.findChild<QScrollArea*>(kScreenFitScroll);
    ASSERT_NE(scroll, nullptr);
    EXPECT_EQ(dialog.findChild<QWidget*>(kScreenFitFooter), nullptr);
    EXPECT_TRUE(scroll->isAncestorOf(buttons));
}

TEST(ScreenFit, ADialogThatFitsWithAButtonRowKeepsItsLayoutAndHasNoFooter)
{
    const FilterGuard guard;
    QDialog dialog;
    auto* layout = new QVBoxLayout(&dialog);
    layout->addWidget(new QLabel("A short form", &dialog));
    layout->addWidget(
        new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel, &dialog));
    QLayout* const written = dialog.layout();
    dialog.show();
    QApplication::processEvents();
    EXPECT_EQ(dialog.layout(), written);
    EXPECT_EQ(dialog.findChild<QScrollArea*>(kScreenFitScroll), nullptr);
    EXPECT_EQ(dialog.findChild<QWidget*>(kScreenFitFooter), nullptr);
}

TEST(ScreenFit, ADialogShownAgainKeepsOneFooter)
{
    const FilterGuard guard;
    QDialog dialog;
    auto* layout = new QVBoxLayout(&dialog);
    addBody(dialog, *layout, 80);
    auto* buttons = new QDialogButtonBox(QDialogButtonBox::Close, &dialog);
    layout->addWidget(buttons);
    for (int i = 0; i < 3; ++i) {
        dialog.show();
        QApplication::processEvents();
        dialog.hide();
    }
    EXPECT_EQ(dialog.findChildren<QWidget*>(kScreenFitFooter).size(), 1);
    EXPECT_EQ(dialog.findChildren<QScrollArea*>(kScreenFitScroll).size(), 1);
    EXPECT_EQ(buttons->parentWidget(), dialog.findChild<QWidget*>(kScreenFitFooter));
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
