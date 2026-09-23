#pragma once

// The Survey menu and toolbar (PLAN.MD 45 slice 9; the owner's request of
// 2026-09-23: "add survey menus and toolbars and wire the functionality").
//
// Every survey action and dialog belongs to this class. MainWindow only makes
// the menu and the toolbar, hands them over with SurveyServices and keeps the
// object alive; everything the workbench needs from the window arrives through
// those callbacks, so it never includes main_window.hpp. That keeps the survey
// work - which will grow a wizard, a point manager and reports - out of a
// window source file that is already 3 500 lines, and lets it be written while
// other work is changing that file.

#include <functional>

#include <QKeySequence>
#include <QString>

#include "icons.hpp"

class QAction;
class QMainWindow;
class QMenu;
class QToolBar;

namespace katana::cad {
class Document;
}

namespace katana::qt {

class ViewWorkspace;

struct SurveyServices {
    katana::cad::Document* document = nullptr;
    // The window's views, for framing what an operation added and for the
    // plan view a tool works in.
    ViewWorkspace* views = nullptr;
    // The window's action factory, so a survey action looks and reads like
    // every other: icon, status tip, and a tooltip naming its shortcut.
    std::function<QAction*(Icon icon, const QString& text, const QString& tip,
                           const QKeySequence& shortcut, const QString& objectName)>
        makeAction;
    // The command log. isError also flashes the message in the status bar.
    std::function<void(const QString& text, bool isError)> log;
    // Actions the window already owns and shows under File; the Survey menu
    // shows the same objects, so the two menus cannot drift apart.
    QAction* loadCustomisation = nullptr;
    QAction* applySurveyCodes = nullptr;
};

class SurveyWorkbench {
  public:
    // Fills `menu` and `toolBar`, which the window has made and placed.
    SurveyWorkbench(QMainWindow& window, SurveyServices services, QMenu& menu,
                    QToolBar& toolBar);
    ~SurveyWorkbench();

    SurveyWorkbench(const SurveyWorkbench&) = delete;
    SurveyWorkbench& operator=(const SurveyWorkbench&) = delete;

  private:
    QMainWindow& window_;
    SurveyServices services_;
};

} // namespace katana::qt
