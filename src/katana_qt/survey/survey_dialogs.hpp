#pragma once

// The dialogs of the Survey menu (PLAN.MD 45 slice 9): Inverse, Forward Point,
// the Angle and Bearing Calculator, Traverse, Level Book and the Coordinate
// Converter.
//
// NOTHING SURVEY-SPECIFIC IS COMPUTED HERE. A dialog gathers text, hands it to
// the functions of katana/cad/survey_tools.hpp and shows the report those
// return - the same report the command line prints for INVERSE, FORWARD and
// AREA - in its own result pane and in the command log. A number that
// appeared only in a dialog would be a second answer nobody tests.
//
// Every dialog is NON-MODAL and kept by the SurveyWorkbench between uses, so
// it can stay open beside the drawing while the selection changes: "Use
// Selection" reads the selected point entities when it is pressed, never
// before. The workbench deletes them before the document goes (see
// ~SurveyWorkbench).
//
// The object names below - of each dialog, its fields and its verb buttons -
// are an interface: the headless --survey-dialog switch in main.cpp fills
// fields and presses buttons by these names, which is how ctest drives a
// dialog the way a person does.
//
//   dialog (action + "Dialog")       fields                         buttons
//   surveyInverseDialog              from to                        useSelection compute
//   surveyForwardDialog              from direction distance        useSelection compute
//                                    heightDifference name          addPoint
//   surveyAngleCalculatorDialog      angle unit                     convert
//   surveyTraverseDialog             name kind start startMark      compute addToDrawing
//                                    startAzimuth legs endStation
//                                    end closingAngle closingMark
//                                    closingAzimuth method
//                                    balanceAngles angleSigma
//                                    distanceSigma distancePpm
//   surveyLevelBookDialog            name startLevel closingLevel   reduce
//                                    book adjustment allowance
//   surveyCoordinateConverterDialog  source target points           useSelection convert

#include <QDialog>

#include <functional>
#include <initializer_list>
#include <string>

#include "katana/core/error.hpp"

class QCheckBox;
class QComboBox;
class QFormLayout;
class QHBoxLayout;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;

namespace katana::cad {
class Document;
}

namespace katana::qt {

class ViewWorkspace;

// What a survey dialog is given by the workbench: the document it reads and
// changes, the views it frames after adding to the drawing, and the command
// log (isError also flashes the status bar).
struct SurveyDialogContext {
    katana::cad::Document* document = nullptr;
    ViewWorkspace* views = nullptr;
    std::function<void(const QString& text, bool isError)> log;
};

// The frame every survey dialog shares: a muted note saying what the tool
// does and what it will not do, a form, a line for what is wrong, a read-only
// result pane in a fixed-width font (the reports are tables), and a row of
// verb buttons ending in Close.
class SurveyToolDialog : public QDialog {
  public:
    // A verb's work: the report to show, or the error saying what is wrong.
    using Verb = std::function<katana::core::Result<std::string>()>;

  protected:
    SurveyToolDialog(SurveyDialogContext context, const QString& objectName, const QString& title,
                     const QString& note, QWidget* parent);

    [[nodiscard]] katana::cad::Document& document() const { return *context_.document; }
    [[nodiscard]] QFormLayout* form() const { return form_; }

    QLineEdit* addField(const QString& label, const QString& objectName,
                        const QString& placeholder, const QString& tip);
    // A field not yet in the form, for a row that holds several.
    QLineEdit* makeField(const QString& objectName, const QString& placeholder,
                         const QString& tip);
    // A form row of fields with a word between each pair: {mark, "or", azimuth}.
    void addRow(const QString& label, std::initializer_list<QWidget*> widgets);
    QPlainTextEdit* addTextField(const QString& label, const QString& objectName,
                                 const QString& placeholder, const QString& tip);
    QComboBox* addChoice(const QString& label, const QString& objectName,
                         const QStringList& items, const QString& tip);
    // A button in the verb row. Its result is shown in the pane and logged;
    // an error is shown on the message line and logged as an error.
    QPushButton* addVerb(const QString& text, const QString& objectName, Verb verb);
    // A button that fills fields rather than computing: its error is shown,
    // its success is silent (what it filled is on screen).
    QPushButton* addFiller(const QString& text, const QString& objectName,
                           std::function<katana::core::Status()> fill);
    // Frames every view on the drawing after an operation added to it.
    void frameViews() const;
    // Shows `text` on the message line in the muted colour (not an error).
    void setNote(const QString& text);

  private:
    void showError(const katana::core::Error& error);

    SurveyDialogContext context_;
    QFormLayout* form_ = nullptr;
    QLabel* message_ = nullptr;
    QPlainTextEdit* result_ = nullptr;
    QHBoxLayout* verbs_ = nullptr;
    bool hasDefaultVerb_ = false;
};

class SurveyInverseDialog final : public SurveyToolDialog {
  public:
    SurveyInverseDialog(SurveyDialogContext context, QWidget* parent);

  private:
    [[nodiscard]] katana::core::Status useSelection();

    QLineEdit* from_ = nullptr;
    QLineEdit* to_ = nullptr;
};

class SurveyForwardDialog final : public SurveyToolDialog {
  public:
    SurveyForwardDialog(SurveyDialogContext context, QWidget* parent);

  private:
    [[nodiscard]] katana::core::Status useSelection();
    [[nodiscard]] katana::core::Result<std::string> compute(bool add);

    QLineEdit* from_ = nullptr;
    QLineEdit* direction_ = nullptr;
    QLineEdit* distance_ = nullptr;
    QLineEdit* heightDifference_ = nullptr;
    QLineEdit* name_ = nullptr;
};

class SurveyAngleDialog final : public SurveyToolDialog {
  public:
    SurveyAngleDialog(SurveyDialogContext context, QWidget* parent);

  private:
    QLineEdit* angle_ = nullptr;
    QComboBox* unit_ = nullptr;
};

class SurveyTraverseDialog final : public SurveyToolDialog {
  public:
    SurveyTraverseDialog(SurveyDialogContext context, QWidget* parent);

  private:
    [[nodiscard]] katana::core::Result<std::string> compute(bool add);
    void kindChanged();

    QLineEdit* name_ = nullptr;
    QComboBox* kind_ = nullptr;
    QLineEdit* start_ = nullptr;
    QLineEdit* startMark_ = nullptr;
    QLineEdit* startAzimuth_ = nullptr;
    QPlainTextEdit* legs_ = nullptr;
    QLineEdit* endStation_ = nullptr;
    QLineEdit* end_ = nullptr;
    QLineEdit* closingAngle_ = nullptr;
    QLineEdit* closingMark_ = nullptr;
    QLineEdit* closingAzimuth_ = nullptr;
    QComboBox* method_ = nullptr;
    QCheckBox* balanceAngles_ = nullptr;
    QLineEdit* angleSigma_ = nullptr;
    QLineEdit* distanceSigma_ = nullptr;
    QLineEdit* distancePpm_ = nullptr;
};

class SurveyLevelBookDialog final : public SurveyToolDialog {
  public:
    SurveyLevelBookDialog(SurveyDialogContext context, QWidget* parent);

  private:
    QLineEdit* name_ = nullptr;
    QLineEdit* startLevel_ = nullptr;
    QLineEdit* closingLevel_ = nullptr;
    QPlainTextEdit* book_ = nullptr;
    QComboBox* adjustment_ = nullptr;
    QLineEdit* allowance_ = nullptr;
};

class SurveyConverterDialog final : public SurveyToolDialog {
  public:
    SurveyConverterDialog(SurveyDialogContext context, QWidget* parent);

  private:
    QLineEdit* source_ = nullptr;
    QLineEdit* target_ = nullptr;
    QPlainTextEdit* points_ = nullptr;
};

} // namespace katana::qt
