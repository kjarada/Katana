#include "screen_fit.hpp"

#include <QAbstractButton>
#include <QApplication>
#include <QBoxLayout>
#include <QColorDialog>
#include <QDialog>
#include <QDialogButtonBox>
#include <QEvent>
#include <QFileDialog>
#include <QFontDialog>
#include <QFrame>
#include <QGuiApplication>
#include <QInputDialog>
#include <QLayoutItem>
#include <QLineEdit>
#include <QMessageBox>
#include <QObject>
#include <QPoint>
#include <QScreen>
#include <QScrollArea>
#include <QStyle>
#include <QVBoxLayout>
#include <QWidget>

#include <algorithm>

namespace katana::qt {

namespace {

// What is kept free round a window on every side of its screen's available area, so
// that its frame never touches the task bar or the edge of the display.
constexpr int kMargin = 8;

// A desktop window's frame - its title bar and borders - when Qt cannot say yet, as
// it cannot before the window has been shown, and not at all offscreen. Windows 10
// and 11: 31 px of title bar, a border of 8 each side (the invisible resize border
// counts); other desktops' are smaller, so this errs towards a window a few pixels
// smaller than it could be, never one that runs off.
constexpr QSize kFrameFallback{16, 40};

// How small a scrolling dialog may be made: a scroll area has no minimum of its own
// worth the name, and a dialog the size of its title bar is no use to anyone. Held
// to the room there is, on a screen smaller than this.
constexpr QSize kLeastScrolling{360, 240};

constexpr char kContentsName[] = "screenFitContents";

// The most of the room (half, as a divisor) a footer may need and still be lifted out of
// the scroll area: a Close row, a reply box and a status line are some 170 px of a
// 700 px room, and more than half would leave too little to scroll the rest in.
constexpr int kFooterShare = 2;

QScrollArea* scrollOf(QDialog& dialog)
{
    return dialog.findChild<QScrollArea*>(kScreenFitScroll, Qt::FindDirectChildrenOnly);
}

// Qt's own dialogs lay themselves out, from a text and some buttons; their layouts are
// not ours to take apart, and what makes one too large is a text too long for a
// screen, which scrolls in its own box.
bool isStandardDialog(const QDialog& dialog)
{
    return qobject_cast<const QMessageBox*>(&dialog) != nullptr ||
           qobject_cast<const QFileDialog*>(&dialog) != nullptr ||
           qobject_cast<const QColorDialog*>(&dialog) != nullptr ||
           qobject_cast<const QFontDialog*>(&dialog) != nullptr ||
           qobject_cast<const QInputDialog*>(&dialog) != nullptr;
}

// Whether `item` is a row of buttons: a QDialogButtonBox, or a horizontal box layout
// holding buttons and nothing else but spacing and read-only fields - at least one
// button, no label, no editable field, no layout inside it. A read-only field is the
// line a dialog will run, shown beside its Preview and Run: a caption, not an input.
// A lone QPushButton laid straight into the dialog's layout is not a row, and a row
// with an editable field in it (a path and its Browse button) is a form's own.
bool isButtonRow(QLayoutItem* item)
{
    if (item == nullptr) {
        return false;
    }
    if (const QWidget* widget = item->widget()) {
        return qobject_cast<const QDialogButtonBox*>(widget) != nullptr;
    }
    const auto* row = qobject_cast<QBoxLayout*>(item->layout());
    if (row == nullptr || (row->direction() != QBoxLayout::LeftToRight &&
                           row->direction() != QBoxLayout::RightToLeft)) {
        return false;
    }
    int buttons = 0;
    for (int i = 0; i < row->count(); ++i) {
        QLayoutItem* each = row->itemAt(i);
        if (each->spacerItem() != nullptr) {
            continue;
        }
        const QWidget* widget = each->widget();
        if (const auto* field = qobject_cast<const QLineEdit*>(widget);
            field != nullptr && field->isReadOnly()) {
            continue;
        }
        if (qobject_cast<const QAbstractButton*>(widget) == nullptr &&
            qobject_cast<const QDialogButtonBox*>(widget) == nullptr) {
            return false;
        }
        ++buttons;
    }
    return buttons > 0;
}

// The index in `box` where the dialog's footer begins, or -1 for none. The footer is
// the LAST button row and every item after it: a dialog shows what its buttons
// answered - a reply, a status line - under them, and a person who pressed one needs
// that in sight with the button. Only a vertical box has one: a dialog laid out in a
// grid or a form, or in a row, keeps its buttons where it put them.
int footerStart(const QBoxLayout& box)
{
    if (box.direction() != QBoxLayout::TopToBottom) {
        return -1;
    }
    for (int i = box.count() - 1; i >= 0; --i) {
        if (isButtonRow(box.itemAt(i))) {
            return i;
        }
    }
    return -1;
}

// What the footer of `box` from `start` needs at the least.
int footerMinimumHeight(const QBoxLayout& box, int start)
{
    int height = 0;
    for (int i = start; i < box.count(); ++i) {
        height += box.itemAt(i)->minimumSize().height();
    }
    return height;
}

// Takes the dialog's footer out of its layout into a widget of its own, which the
// caller puts below the scroll area, and returns it; null when there is none. A footer
// that would take more than half the room is not lifted - a button row followed by a
// table is a dialog whose buttons are in its middle - and the whole dialog scrolls, as
// it did before footers.
QWidget* liftFooter(QDialog& dialog, QLayout& laidOut, const QSize& client)
{
    auto* box = qobject_cast<QBoxLayout*>(&laidOut);
    if (box == nullptr) {
        return nullptr;
    }
    const int start = footerStart(*box);
    if (start < 0 || footerMinimumHeight(*box, start) > client.height() / kFooterShare) {
        return nullptr;
    }
    auto* footer = new QWidget(&dialog);
    footer->setObjectName(kScreenFitFooter);
    auto* footerBox = new QVBoxLayout(footer);
    // The margins and spacing the dialog's own layout has, so the footer lines up with
    // what scrolls above it.
    footerBox->setContentsMargins(box->contentsMargins());
    footerBox->setSpacing(box->spacing());
    while (box->count() > start) {
        QLayoutItem* item = box->takeAt(start);
        if (QWidget* widget = item->widget()) {
            footerBox->addWidget(widget);
            delete item;
        } else if (QLayout* row = item->layout()) {
            // takeAt left the row without a parent, which addLayout asks for.
            footerBox->addLayout(row);
        } else {
            footerBox->addItem(item);
        }
    }
    return footer;
}

// Puts everything the dialog lays out into a scroll area that fills it, but for its
// footer (above), which stays below the scroll area where it is always in sight. The
// layout moves as it is - QWidget::setLayout takes a layout off the widget that has it
// and brings the widgets it lays out along - so each control keeps its place in it, its
// object name and its parent chain below the scroll area (or the footer): whatever
// finds a control by name in the dialog finds it still.
bool wrapInScrollArea(QDialog& dialog, const QSize& client)
{
    QLayout* laidOut = dialog.layout();
    if (laidOut == nullptr) {
        return false;
    }
    // Before the layout moves: what is taken out of it here is not taken into the
    // scroll area with the rest.
    QWidget* const footer = liftFooter(dialog, *laidOut, client);
    auto* scroll = new QScrollArea(&dialog);
    scroll->setObjectName(kScreenFitScroll);
    scroll->setFrameShape(QFrame::NoFrame);
    scroll->setWidgetResizable(true);
    auto* contents = new QWidget;
    contents->setObjectName(kContentsName);
    contents->setLayout(laidOut);
    scroll->setWidget(contents);
    auto* outer = new QVBoxLayout(&dialog);
    outer->setContentsMargins(0, 0, 0, 0);
    outer->setSpacing(0);
    outer->addWidget(scroll, 1);
    if (footer != nullptr) {
        outer->addWidget(footer);
    }
    // The old layout had forced the dialog to be as large as its controls need - that
    // is what did not fit - and a minimum a person or a layout set explicitly stays
    // until it is told otherwise.
    dialog.setMinimumSize(std::min(kLeastScrolling.width(), client.width()),
                          std::min(kLeastScrolling.height(), client.height()));
    outer->activate();
    return true;
}

class ScreenFitFilter final : public QObject {
  public:
    explicit ScreenFitFilter(QObject* parent) : QObject(parent) {}

    bool eventFilter(QObject* watched, QEvent* event) override
    {
        // Before the dialog's own handler, so that its centring over its parent
        // (QDialog::showEvent) works with the size it will have. A show that is the
        // window coming back from being minimised is the same window as before.
        if (event->type() == QEvent::Show && !event->spontaneous() && watched->isWidgetType()) {
            if (auto* dialog = qobject_cast<QDialog*>(watched); dialog != nullptr) {
                fitDialogToScreen(*dialog);
            }
        }
        return false;
    }
};

} // namespace

QSize frameOf(const QWidget& window)
{
    const QSize known = window.frameSize() - window.size();
    if (known.width() <= 0 && known.height() <= 0) {
        return kFrameFallback;
    }
    return known;
}

QRect roomOnScreen(const QWidget& window)
{
    const QScreen* screen = window.screen();
    if (screen == nullptr) {
        screen = QGuiApplication::primaryScreen();
    }
    if (screen == nullptr) {
        return {};
    }
    return screen->availableGeometry().adjusted(kMargin, kMargin, -kMargin, -kMargin);
}

bool fitDialogToScreen(QDialog& dialog)
{
    if (!dialog.isWindow()) {
        return false;
    }
    const QRect room = roomOnScreen(dialog);
    if (room.isEmpty()) {
        return false;
    }
    const QSize frame = frameOf(dialog);
    const QSize client(std::max(0, room.width() - frame.width()),
                       std::max(0, room.height() - frame.height()));

    bool scrolls = scrollOf(dialog) != nullptr;
    if (!scrolls && !isStandardDialog(dialog)) {
        // The least the dialog can be made: what its layout needs, or a minimum it
        // was given. Over the room in either direction, its controls cannot all be
        // on the screen at once, so they scroll.
        const QSize least = dialog.minimumSizeHint().expandedTo(dialog.minimumSize());
        if (least.width() > client.width() || least.height() > client.height()) {
            scrolls = wrapInScrollArea(dialog, client);
        }
    }

    QSize wanted = dialog.size();
    if (scrolls && wanted.height() > client.height()) {
        // A vertical scroll bar will take its width from the controls: leave room.
        wanted.rwidth() += dialog.style()->pixelMetric(QStyle::PM_ScrollBarExtent, nullptr, &dialog);
    }
    wanted = wanted.boundedTo(client);
    if (wanted != dialog.size()) {
        dialog.resize(wanted);
    }

    // A dialog that was put somewhere by hand stays where it was put, brought back
    // onto the screen if it was not on it. One that was not is centred by the dialog
    // itself, over its parent and inside the screen, when it is shown.
    if (dialog.testAttribute(Qt::WA_Moved)) {
        const QSize outer = wanted + frame;
        QPoint at = dialog.pos();
        at.setX(std::clamp(at.x(), room.left(), std::max(room.left(), room.right() + 1 - outer.width())));
        at.setY(std::clamp(at.y(), room.top(), std::max(room.top(), room.bottom() + 1 - outer.height())));
        if (at != dialog.pos()) {
            dialog.move(at);
        }
    }
    return scrolls;
}

QObject* installScreenFit(QApplication& application)
{
    auto* filter = new ScreenFitFilter(&application);
    application.installEventFilter(filter);
    return filter;
}

} // namespace katana::qt
