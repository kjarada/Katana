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
//
// The menu, in sections:
//   Coordinate Geometry     Inverse..., Forward Point..., Area of Selection,
//                           Angle and Bearing Calculator...
//   Traverse and Levelling  Traverse..., Level Book...
//   Coordinates             Coordinate Converter...
//   Survey Coding           the window's two customisation actions
// Each action's object name (surveyInverse, surveyForward, surveyArea,
// surveyAngleCalculator, surveyTraverse, surveyLevelBook,
// surveyCoordinateConverter) is what --action and --survey-dialog know it by.
// A dialog's object name is its action's plus "Dialog" (survey_dialogs.hpp).
//
// The computing is katana::cad's (survey_tools.hpp); this class and its
// dialogs only gather input and show the reports.

#include <functional>

#include <QKeySequence>
#include <QPointer>
#include <QString>

#include "icons.hpp"

class QAction;
class QDialog;
class QMainWindow;
class QMenu;
class QToolBar;

namespace katana::cad {
class Document;
}

namespace katana::qt {

class ViewWorkspace;
struct SurveyDialogContext;

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
    // Deletes the dialogs. They are the window's children only so that they
    // float over it; they hold the document, which the window destroys
    // BEFORE its children (members go before the QWidget base), so they must
    // go with this object - which the window destroys before the document.
    ~SurveyWorkbench();

    SurveyWorkbench(const SurveyWorkbench&) = delete;
    SurveyWorkbench& operator=(const SurveyWorkbench&) = delete;

  private:
    [[nodiscard]] SurveyDialogContext dialogContext() const;
    // Shows the dialog in `slot`, making it the first time: one of each,
    // kept between uses so that what was typed survives closing it.
    void open(QPointer<QDialog>& slot, const std::function<QDialog*()>& make);
    // Survey > Area of Selection: no dialog - the report goes to the log.
    void areaOfSelection();

    QMainWindow& window_;
    SurveyServices services_;
    QPointer<QDialog> inverse_;
    QPointer<QDialog> forward_;
    QPointer<QDialog> angle_;
    QPointer<QDialog> traverse_;
    QPointer<QDialog> levelBook_;
    QPointer<QDialog> converter_;
};

} // namespace katana::qt
