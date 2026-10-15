#pragma once

// Keeping the window's own windows reachable on the screen they are shown on.
//
// A dialog is laid out for the controls it holds, not for the screen it is shown
// on: Terrain > Volumes was about 1550 pixels tall, the style manager and the road
// template manager are as tall, and a 1366 x 768 laptop leaves a window 728 pixels
// of room under its task bar. A window taller than that has its bottom - the Close
// button, the results - off the screen, and Qt's layouts will not make it smaller
// than the controls need. Every dialog that was written to fit is one more that a
// smaller screen then proves wrong, so this is not left to each dialog:
//
//   * A dialog is shown no larger than its screen's room.
//   * A dialog whose controls cannot be laid out in that room is given a scroll
//     area for them, as it is shown, so that every control is reachable by
//     scrolling and the window is as large as the screen lets it be.
//   * A dialog that fits is left exactly as it was written.
//
// The menus have their half in the application's style (theme.cpp: a menu taller
// than the screen scrolls); the main window's is MainWindow::fitToScreen; the
// toolbars' is the overflow arrow. docs/desktop.md, "Fitting the screen".

#include <QRect>
#include <QSize>

class QApplication;
class QDialog;
class QObject;
class QWidget;

namespace katana::qt {

// The name of the scroll area a dialog that did not fit is given, found as a
// child of the dialog by it.
inline constexpr char kScreenFitScroll[] = "screenFitScroll";

// The rectangle, in screen coordinates, that a top-level window's FRAME may take on
// the screen it is on: the screen's available area (what a task bar and a dock leave
// it) less a small margin on every side. An empty rectangle when there is no screen
// to measure, which a caller takes as no limit.
[[nodiscard]] QRect roomOnScreen(const QWidget& window);

// What a window's frame adds to its client area (the title bar and borders), as
// Qt knows it once the window has been shown, else the commonest desktop's.
[[nodiscard]] QSize frameOf(const QWidget& window);

// Makes `dialog` fit the room of its screen, as above: its size is limited, its
// position is kept on the screen when it was placed by hand, and controls that cannot
// be laid out in the room are put in a scroll area. Only a top-level dialog, and not
// one of Qt's own standard dialogs (QMessageBox, QFileDialog, QColorDialog,
// QFontDialog, QInputDialog), whose layouts are theirs, though their size is limited
// all the same. True when the dialog scrolls (now or from an earlier call).
bool fitDialogToScreen(QDialog& dialog);

// Installs the application-wide filter that calls fitDialogToScreen as each
// top-level dialog is shown, so no dialog has to ask. Returns the filter, owned by
// the application; deleting it takes it off again (a test does).
QObject* installScreenFit(QApplication& application);

} // namespace katana::qt
