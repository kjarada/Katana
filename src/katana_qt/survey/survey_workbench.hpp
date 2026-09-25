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
//   Survey Points           Import Survey Points..., Survey Jobs...,
//                           Export Survey Points..., Point Manager (a dock),
//                           Point Report...
//   Coordinate Geometry     Inverse..., Forward Point..., Area of Selection,
//                           Angle and Bearing Calculator...
//   Traverse and Levelling  Traverse..., Level Book...
//   Coordinates             Coordinate Converter...
//   Survey Coding           the window's customisation actions and the
//                           Format menu's Survey Code Manager
// Each action's object name (surveyImport, surveyJobs, surveyExport,
// surveyPointManager,
// surveyPointReport, surveyInverse, surveyForward, surveyArea,
// surveyAngleCalculator, surveyTraverse, surveyLevelBook,
// surveyCoordinateConverter) is what --action and --survey-dialog know it by.
// A dialog's object name is its action's plus "Dialog" (survey_dialogs.hpp,
// survey_import_wizard.hpp, survey_jobs_dialog.hpp, survey_points_ui.hpp); the
// Point Manager is the
// dock SurveyPointsDock, which --survey-dock knows it by.
//
// The computing is katana::cad's (survey_tools.hpp); this class and its
// dialogs only gather input and show the reports.

#include <functional>

#include <QKeySequence>
#include <QPointer>
#include <QString>

#include "command_runner.hpp"
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

class DockChrome;
class SurveyPointsDock;
class ViewWorkspace;
struct SurveyDialogContext;

struct SurveyServices {
    katana::cad::Document* document = nullptr;
    // The window's views, for framing what an operation added and for the
    // plan view a tool works in.
    ViewWorkspace* views = nullptr;
    // The window's dock chrome, so the Point Manager gets the title bar -
    // Minimise, Float, Close - every other panel has. May be null.
    DockChrome* chrome = nullptr;
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
    QAction* replaceCustomisation = nullptr;
    QAction* applySurveyCodes = nullptr;
    // The Format menu's Survey Code Manager (CustomisationWorkbench), shown
    // under Survey Coding as well: where a surveyor looks for the code
    // library. The same object again.
    QAction* codeManager = nullptr;
    // The window's one executor (command_runner.hpp), for a dialog that
    // changes the drawing through a verb line. May be empty (a test).
    CommandRunner run;
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
    // Survey > Point Manager: made the first time it is shown.
    void showPointManager(bool show);

    QMainWindow& window_;
    SurveyServices services_;
    QPointer<QDialog> inverse_;
    QPointer<QDialog> forward_;
    QPointer<QDialog> angle_;
    QPointer<QDialog> traverse_;
    QPointer<QDialog> levelBook_;
    QPointer<QDialog> converter_;
    QPointer<QDialog> import_;
    QPointer<QDialog> jobs_;
    QPointer<QDialog> export_;
    QPointer<QDialog> pointReport_;
    QPointer<SurveyPointsDock> pointManager_;
    QAction* pointManagerAction_ = nullptr;
};

} // namespace katana::qt
