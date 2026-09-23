#pragma once

// Survey > Import Survey Points... (PLAN.MD 45 slice 10): a paged dialog in
// six steps.
//
//   1 File        the file to read.
//   2 Format      surveyio's detection: every candidate ranked with its
//                 evidence and its FormatDescriptor record (what the format
//                 carries, the parser's version). When the detection is not
//                 Identified - and a delimited text file never is, because its
//                 probe is weak by design - the person MUST choose: the page
//                 starts on "(choose the format)" and Next refuses it. An
//                 unknown file is never handed to a parser silently.
//   3 Layout      for a delimited file: proposeLayout's reading of the header,
//                 its candidates, presets, a role box per column of the list
//                 as typed (kept while a change such as swapping northing and
//                 easting passes through a list that does not validate), the delimiter,
//                 header lines, comment prefix and quoting, saved templates
//                 (survey_templates.hpp), and a preview of the first points
//                 with the parser's error - line and column - inline. When the
//                 proposal is Uncertain the page says so and Next needs the
//                 person to tick that they have checked the order of northing
//                 and easting (PLAN.MD 45.5: the order is never guessed).
//   4 System      the unit the numbers are in (no default - the reader
//                 refuses Unknown), the system the file is in (unknown unless
//                 stated) and, only when BOTH a source and a target EPSG code
//                 are given, a transformation through katana::geodesy
//                 (cad::transformSurveyProject). The parser never transforms.
//   5 Options     the layer, a layer per code, applying the loaded mapfile's
//                 survey codes after the import (the window's own Apply
//                 Survey Codes action, on the imported points), and what to do
//                 with ids the drawing already has (cad::ExistingPointPolicy).
//   6 Report      records read, every warning as a sentence, the error that
//                 blocks the import or the points and layers it will create.
//                 Import makes ONE undoable command (cad::importSurveyPoints),
//                 frames the views and logs the report.
//
// Nothing survey-specific is computed here: the pages gather text and hand it
// to surveyio (detection, proposal, parsing, templates) and cad (the policy,
// the transformation, the command, the report).
//
// The object names below are an interface: the headless --survey-dialog
// switch in main.cpp fills fields and presses buttons by them.
//   dialog  surveyImportDialog
//   fields  file | format | candidate preset template templateName columns
//           delimiter headerLines comment quoting confirmOrder | unit declared
//           target | layer layerPerCode existing applyCodes
//   shown   step candidates formatRecord proposal columnRoles preview
//           parseError report message
//   buttons browse | saveTemplate deleteTemplate | back next import close

#include <QDialog>

#include <functional>
#include <optional>
#include <string>

#include "katana/core/error.hpp"
#include "katana/survey/data_model.hpp"
#include "katana/surveyio/delimited_points.hpp"
#include "katana/surveyio/detect.hpp"

class QAction;
class QCheckBox;
class QComboBox;
class QHBoxLayout;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class QSpinBox;
class QStackedWidget;
class QTableWidget;
class QTreeWidget;

namespace katana::cad {
class Document;
}

namespace katana::qt {

class ViewWorkspace;

struct SurveyImportContext {
    katana::cad::Document* document = nullptr;
    ViewWorkspace* views = nullptr;
    std::function<void(const QString& text, bool isError)> log;
    // The window's Apply Survey Codes action; null when it has none.
    QAction* applySurveyCodes = nullptr;
};

class SurveyImportWizard final : public QDialog {
  public:
    SurveyImportWizard(SurveyImportContext context, QWidget* parent);

  protected:
    void showEvent(QShowEvent* event) override;

  private:
    enum Page { FilePage, FormatPage, LayoutPage, SystemPage, OptionsPage, ReportPage, Pages };

    QWidget* buildFilePage();
    QWidget* buildFormatPage();
    QWidget* buildLayoutPage();
    QWidget* buildSystemPage();
    QWidget* buildOptionsPage();
    QWidget* buildReportPage();

    void goTo(int page);
    // What leaving `page` forward needs: the page's input checked and the next
    // page's content prepared. An error keeps the person where they are.
    [[nodiscard]] katana::core::Status leave(int page);
    [[nodiscard]] katana::core::Status readFile();
    [[nodiscard]] katana::core::Status chooseFormat();
    void showFormatRecord();
    void applyLayout(const katana::surveyio::DelimitedLayout& layout);
    [[nodiscard]] katana::core::Result<katana::surveyio::DelimitedLayout> layoutFromFields() const;
    // The preview is rebuilt on the event loop, never inside the signal of
    // the widget that asked: the role boxes it rebuilds are among the widgets
    // whose signals ask (docs/cad.md, "Panels refresh on the event loop").
    void schedulePreview();
    void preview();
    // The role box of `column` (0-based) changed: its entry in the column
    // list is rewritten, and the preview follows on the event loop.
    void rolesChanged(int column);
    [[nodiscard]] katana::core::Status parseAndTransform();
    void prepareReport();
    void importNow();
    void showError(const katana::core::Error& error);
    void showMessage(const QString& text);

    SurveyImportContext context_;
    QLabel* step_ = nullptr;
    QStackedWidget* pages_ = nullptr;
    QLabel* message_ = nullptr;
    QPushButton* back_ = nullptr;
    QPushButton* next_ = nullptr;
    QPushButton* import_ = nullptr;

    // 1
    QLineEdit* file_ = nullptr;
    // 2
    QLabel* detectionSummary_ = nullptr;
    QTreeWidget* candidates_ = nullptr;
    QComboBox* format_ = nullptr;
    QLabel* formatRecord_ = nullptr;
    // 3
    QLabel* proposal_ = nullptr;
    QComboBox* candidate_ = nullptr;
    QComboBox* preset_ = nullptr;
    QComboBox* template_ = nullptr;
    QLineEdit* templateName_ = nullptr;
    QLineEdit* columns_ = nullptr;
    QComboBox* delimiter_ = nullptr;
    QSpinBox* headerLines_ = nullptr;
    QLineEdit* comment_ = nullptr;
    QComboBox* quoting_ = nullptr;
    QCheckBox* confirmOrder_ = nullptr;
    QWidget* columnRoles_ = nullptr;
    QHBoxLayout* roleRow_ = nullptr;
    QTableWidget* preview_ = nullptr;
    QLabel* parseError_ = nullptr;
    // 4
    QComboBox* unit_ = nullptr;
    QLineEdit* declared_ = nullptr;
    QLineEdit* target_ = nullptr;
    // 5
    QLineEdit* layer_ = nullptr;
    QCheckBox* layerPerCode_ = nullptr;
    QComboBox* existing_ = nullptr;
    QCheckBox* applyCodes_ = nullptr;
    // 6
    QPlainTextEdit* report_ = nullptr;

    // What the pages have established so far.
    std::string bytes_;
    std::string fileName_; // the NAME only (45.2)
    std::optional<katana::surveyio::Detection> detection_;
    std::optional<katana::surveyio::LayoutProposal> layoutProposal_;
    std::optional<katana::surveyio::ImportResult> parsed_;
    std::optional<katana::survey::SurveyProject> project_;
    std::string layoutText_;
    std::string systemText_;
    std::string transformText_;
    bool previewPending_ = false;
    bool previewFailed_ = true;
    bool settingRoles_ = false; // preview() is mirroring the text onto the role boxes
    bool blocked_ = true;
};

} // namespace katana::qt
