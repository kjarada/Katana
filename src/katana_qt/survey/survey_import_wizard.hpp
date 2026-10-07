#pragma once

// Survey > Import Survey Points... (PLAN.MD 45 slice 10): a paged dialog. A
// delimited coordinate file takes the six steps it always has; a field file
// with a reader of its own (Leica GSI, Trimble JobXML, Topcon and Carlson
// RW5, Topcon GTS-7, RINEX ...) takes the instrument path, which reads the
// file whole, reduces and adjusts it, and keeps it as a SURVEY JOB that can
// be adjusted again later (Survey > Survey Jobs).
//
//   1 File        the file to read.
//   2 Format      surveyio's detection: EVERY registered format, the
//                 candidates first, each with its evidence and its
//                 FormatDescriptor record (what the format carries, the
//                 parser's version). When the detection is not Identified -
//                 and a delimited text file never is, because its probe is
//                 weak by design - the person MUST choose: the page starts on
//                 "(choose the format)" and Next refuses it. An unknown file
//                 is never handed to a parser silently.
//   3 Layout      delimited files only: proposeLayout's reading of the header,
//                 its candidates, presets, a role box per column of the list
//                 as typed (kept while a change such as swapping northing and
//                 easting passes through a list that does not validate), the delimiter,
//                 header lines, comment prefix and quoting, saved templates
//                 (survey_templates.hpp), and a preview of the first points
//                 with the parser's error - line and column - inline. When the
//                 proposal is Uncertain the page says so and Next needs the
//                 person to tick that they have checked the order of northing
//                 and easting (PLAN.MD 45.5: the order is never guessed).
//   3 Content     instrument files only, in place of Layout: the file read
//                 with surveyio::readSurvey - setups, observations by kind,
//                 points with and without coordinates, coded features, the
//                 control the file declares, GNSS sessions, the other files
//                 found beside it (a DBX job's files, a RINEX navigation
//                 file), what the file does not carry, and every warning with
//                 its record. A large file is read on a pool thread, with a
//                 busy bar and Cancel (survey_task.hpp).
//   4 System      the unit the numbers are in (no default - the reader
//                 refuses Unknown), the system the file is in (unknown unless
//                 stated) and, only when BOTH a source and a target EPSG code
//                 are given, a transformation through katana::geodesy
//                 (cad::transformSurveyProject). The parser never transforms.
//                 For an instrument file the units are the file's own, stated
//                 by its reader, and nothing is transformed: raw observations
//                 are reduced into the drawing's system (the page says which).
//   5 Reduction   instrument files only: the reduction and adjustment
//     & Adjustment options (reduction_options_widget.hpp) - basic ones shown,
//                 advanced ones folded - and Preview, which runs the reduction
//                 (on a pool thread for a large file) and shows its report
//                 inline (reduction_report_view.hpp).
//   6 Options     the layer, a layer per code, applying the loaded
//                 customisation's survey codes after the import (the window's
//                 own Apply Survey Codes action, on the imported points), and
//                 what to do with ids the drawing already has
//                 (cad::ExistingPointPolicy). Its note says what loads the
//                 codes: the CUSTOMISE line, of a Katana customisation file.
//   7 Report      records read, every warning as a sentence, the error that
//                 blocks the import or the points and layers it will create;
//                 for an instrument file the reduction report as well.
//                 Import makes ONE undoable command - cad::importSurveyPoints
//                 for a delimited file, cad::ImportSurveyJobCommand for an
//                 instrument file - frames the views and logs a summary.
//
// Nothing survey-specific is computed here: the pages gather text and hand it
// to surveyio (detection, proposal, reading, templates), survey (the
// reduction) and cad (the policy, the transformation, the commands, the
// report).
//
// The object names below are an interface: the headless --survey-dialog
// switch in main.cpp fills fields and presses buttons by them.
//   dialog  surveyImportDialog
//   fields  file | format | candidate preset template templateName columns
//           delimiter headerLines comment quoting confirmOrder | unit declared
//           target | the reduction options (reduction_options_widget.hpp:
//           method, atmospheric, controlPick, advanced, ...) | layer
//           layerPerCode existing applyCodes
//   shown   step candidates formatRecord proposal columnRoles preview
//           parseError | content contentSummary | systemSummary |
//           previewReport (and previewReportSections, previewReportBrowser,
//           previewReportSummary) | optionsNote | report importReport (and its
//           Sections, Browser, Summary) | task taskStatus | message
//   buttons browse | saveTemplate deleteTemplate | addControl removeControl
//           advanced previewReduction | cancelTask | back next import close

#include <QDialog>

#include <cstddef>
#include <cstdint>
#include <functional>
#include <map>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/survey/data_model.hpp"
#include "katana/survey/reduction.hpp"
#include "katana/surveyio/delimited_points.hpp"
#include "katana/surveyio/detect.hpp"
#include "katana/surveyio/format.hpp"

class QAction;
class QCheckBox;
class QComboBox;
class QHBoxLayout;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class QSpinBox;
class QSplitter;
class QStackedWidget;
class QTableWidget;
class QTreeWidget;

namespace katana::cad {
class Document;
}

namespace katana::qt {

class ReductionOptionsWidget;
class ReductionReportView;
class SurveyTaskBar;
class ViewWorkspace;

// What a field file holds, counted for the Content step: worked out on the
// thread that read the file, so the GUI thread only shows it.
struct SurveyContent {
    std::size_t setups = 0;
    std::size_t observations = 0; // on setups and loose
    std::map<std::string, std::size_t> observationsByKind{};
    std::size_t positionedPoints = 0;
    std::size_t unpositionedPoints = 0;
    std::size_t features = 0;
    std::size_t controlPoints = 0;
    std::size_t gnssSessions = 0;
    std::size_t traverses = 0;
};
[[nodiscard]] SurveyContent surveyContentOf(const katana::survey::SurveyProject& project);

// The File step's Browse filter: every extension a registered format names
// (FormatDescriptor::extensions), so a format added to the registry is
// offered without editing this, and the patterns no descriptor states.
[[nodiscard]] QString surveyFileFilter();

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
    enum Page {
        FilePage,
        FormatPage,
        LayoutPage,
        ContentPage,
        SystemPage,
        ReductionPage,
        OptionsPage,
        ReportPage,
        Pages
    };

    QWidget* buildFilePage();
    QWidget* buildFormatPage();
    QWidget* buildLayoutPage();
    QWidget* buildContentPage();
    QWidget* buildSystemPage();
    QWidget* buildReductionPage();
    QWidget* buildOptionsPage();
    QWidget* buildReportPage();

    // The pages this file goes through, in order: the delimited path or the
    // instrument path (readerPath_).
    [[nodiscard]] std::vector<int> path() const;
    [[nodiscard]] int nextPage(int page) const;
    [[nodiscard]] int previousPage(int page) const;
    void advance();
    void goTo(int page);
    // "Step 3 of 7: What the file holds" for the page on show.
    void showStepTitle();
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

    // The instrument path.
    // Reads the file with its reader - on a pool thread when it is large -
    // and then shows the Content step.
    void readWithReader();
    void showContent();
    void prepareSystemForReader();
    void prepareReduction();
    // Runs the reduction with the options as they stand; `then` follows on
    // the GUI thread once it has succeeded (the Report step, the import).
    void runPreview(std::function<void()> then);
    [[nodiscard]] bool previewIsCurrent() const;
    void prepareReaderReport();
    void importJob();
    [[nodiscard]] katana::core::Result<katana::survey::ReductionContext> contextForReduction() const;
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
    // content
    QLabel* contentSummary_ = nullptr;
    QTreeWidget* content_ = nullptr;
    // system
    QLabel* systemSummary_ = nullptr;
    QWidget* systemFields_ = nullptr; // the unit and systems of a coordinate file
    // reduction
    ReductionOptionsWidget* options_ = nullptr;
    ReductionReportView* previewReport_ = nullptr;
    QSplitter* reductionSplitter_ = nullptr; // the options above, the preview below
    QPushButton* previewButton_ = nullptr;
    // report
    QPlainTextEdit* report_ = nullptr;
    ReductionReportView* importReport_ = nullptr;
    SurveyTaskBar* task_ = nullptr;

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
    // The instrument path's state. The raw project is shared, not copied,
    // with the threads that reduce it.
    bool readerPath_ = false;
    bool bytesComplete_ = false; // bytes_ holds the whole file, not only its head
    std::string formatId_;
    std::shared_ptr<const std::string> sourceBytes_;
    std::shared_ptr<const katana::surveyio::ReadResult> read_;
    std::shared_ptr<const katana::survey::SurveyProject> raw_;
    SurveyContent contentCounts_;
    std::uint64_t readGeneration_ = 0;   // bumped by every read
    std::uint64_t optionsGeneration_ = 0; // the read the options were set up for
    // The last successful preview and what it was made from.
    std::shared_ptr<const katana::survey::ReductionOutcome> outcome_;
    katana::survey::ReductionSettings outcomeSettings_{};
    std::vector<katana::survey::SurveyPoint> outcomeDrawingPoints_;
    std::uint64_t outcomeRead_ = 0;
    std::uint64_t outcomeRevision_ = 0;
    std::string outcomeSystem_;
    bool previewPending_ = false;
    bool previewFailed_ = true;
    bool settingRoles_ = false; // preview() is mirroring the text onto the role boxes
    bool blocked_ = true;
};

} // namespace katana::qt
