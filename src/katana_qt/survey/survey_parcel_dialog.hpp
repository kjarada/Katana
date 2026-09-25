#pragma once

// Survey > Coordinate Geometry > Parcel Report... (docs/survey.md, "The Survey
// menu"): a closed polyline's courses, area and deed wording, and bearing and
// distance labels on it.
//
// NOTHING IS COMPUTED HERE. The courses, the summary and the legal description
// come from katana/cad/parcel.hpp - parcelReport, formatParcelReport and
// legalDescription, the functions PARCEL id and PARCEL id LEGAL print from -
// so the dialog and the command line say the same thing. The labels change
// the drawing, so they are not made here either: Label Courses writes the line
// PARCEL <id> LABEL <height> and hands it to the window's one executor
// (command_runner.hpp), which echoes it, keeps it in the history and makes the
// labels one undo step, exactly as if typed.
//
// A SurveyToolDialog like the menu's others: non-modal, kept by the Survey
// workbench, Use Selection reading the selection when pressed. The report
// pane below the tabs shows what PARCEL id prints, and the log gets it too.
//
// Object names (the headless --dialog surveyParcelReport, --fill and --press
// drive it by these):
//   surveyParcelReportDialog   the dialog (the action surveyParcelReport + "Dialog")
//   parcel                     the parcel: a closed polyline's entity id
//   useSelection               fill parcel from the one selected closed polyline
//   compute                    the report: the pane, the log and both tabs
//   parcelTabs                 Courses, Legal Description
//   parcelCourses              course, from E, from N, bearing, distance
//   parcelSummary              area in m2 and ha, perimeter, centroid, direction drawn
//   parcelCopy                 the courses to the clipboard, tab separated
//   parcelCsv                  Save CSV... (no file dialog in a headless run)
//   parcelName                 the name the legal description opens with
//   parcelLegalText            the legal description (read only)
//   parcelLegalSave            Save... as a .txt (no file dialog in a headless run)
//   labelHeight                the label text height, model units (2.5)
//   label                      PARCEL <id> LABEL <height> through the executor

#include <functional>
#include <optional>

#include "command_runner.hpp"
#include "katana/cad/parcel.hpp"
#include "katana/entity/entity.hpp"
#include "survey/survey_dialogs.hpp"

class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QTableWidget;

namespace katana::qt {

class SurveyParcelDialog final : public SurveyToolDialog {
  public:
    // `run` is the window's executor, which Label Courses needs; empty, the
    // dialog says it cannot label. `headless` is true in a session that
    // opens no file dialog; may be empty.
    SurveyParcelDialog(SurveyDialogContext context, CommandRunner run,
                       std::function<bool()> headless, QWidget* parent);

    // What Save CSV and the legal Save write, to `path`, without asking
    // where. InvalidState before a report has been computed.
    [[nodiscard]] katana::core::Status saveCoursesCsv(const QString& path) const;
    [[nodiscard]] katana::core::Status saveLegalText(const QString& path) const;

  private:
    [[nodiscard]] katana::core::Status useSelection();
    [[nodiscard]] katana::core::Result<std::string> compute();
    [[nodiscard]] katana::core::Status label();
    void showLegal();
    void save(const QString& title, const QString& suggested, const QString& filter,
              const std::function<katana::core::Status(const QString&)>& write);

    CommandRunner run_;
    std::function<bool()> headless_;
    QLineEdit* parcel_ = nullptr;
    QTableWidget* courses_ = nullptr;
    QLabel* summary_ = nullptr;
    QLineEdit* name_ = nullptr;
    QPlainTextEdit* legal_ = nullptr;
    QLineEdit* labelHeight_ = nullptr;
    // The last report computed, and the entity it is of.
    std::optional<katana::cad::ParcelReport> report_;
    katana::entity::EntityId reportOf_ = 0;
};

} // namespace katana::qt
