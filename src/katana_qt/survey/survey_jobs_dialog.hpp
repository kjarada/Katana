#pragma once

// Survey > Survey Jobs...: the field files imported as survey jobs, and going
// back to one to change how it was reduced and adjusted.
//
//   the list      every job with its format, file, when it was imported, its
//                 method, its points and its last adjustment's variance factor
//                 and global test (read back from its stored report).
//   View Report   the job's last reduction report (reduction_report_view.hpp).
//   Edit          the job's settings in the same options the import wizard
//   Adjustment    showed (reduction_options_widget.hpp). Preview reads the
//                 job's STORED bytes again through surveyio - the folder they
//                 came from is not needed - reduces them with the new
//                 settings and shows the new report beside the old one, with
//                 every point's shift from the job's last run listed and the
//                 ones that move highlighted. Apply runs
//                 cad::ReadjustSurveyJobCommand: one undo step. What to do
//                 with the job's points the person has changed by hand is
//                 asked on the page, beside Apply - never in a box, so a
//                 headless run is never stopped by one.
//   Export Report the job's report as HTML (as kept), plain text, or PDF
//                 (the HTML laid out on A4 pages), to the file named beside it.
//   Remove Job    asks on the page, with or without the job's points, then
//                 cad::RemoveSurveyJobCommand: one undo step.
//
// NON-MODAL and one of it: the workbench keeps it, and deletes it before the
// Document goes (~SurveyWorkbench). The list follows the document - an undo,
// a job imported meanwhile - through a Document listener, refreshed on the
// event loop.
//
// Reading and reducing a large job run on a pool thread (survey_task.hpp).
//
// Object names (an interface: the headless --dialog, --fill and --press):
//   dialog   surveyJobsDialog
//   list     jobs (column 0 is the job id: --fill jobs=job-1 chooses it)
//   buttons  viewReport editAdjustment exportReport browseExport removeJob
//            confirmRemove cancelRemove previewAdjustment applyAdjustment
//            close cancelTask
//   fields   exportFormat exportFile removeWithPoints handEdits and the
//            options (reduction_options_widget.hpp)
//   shown    jobSummary removeQuestion handEditsFound shifts shiftSummary
//            jobReport oldReport newReport (each with its Sections, Browser
//            and Summary) task taskStatus message

#include <QDialog>

#include <cstdint>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/survey_job.hpp"
#include "katana/survey/reduction.hpp"
#include "katana/surveyio/format.hpp"

class QCheckBox;
class QComboBox;
class QLabel;
class QLineEdit;
class QPushButton;
class QStackedWidget;
class QTableWidget;
class QTreeWidget;
class QWidget;

namespace katana::qt {

class ReductionOptionsWidget;
class ReductionReportView;
class SurveyTaskBar;
struct SurveyDialogContext;

class SurveyJobsDialog final : public QDialog {
  public:
    SurveyJobsDialog(const SurveyDialogContext& context, QWidget* parent);
    ~SurveyJobsDialog() override;

  private:
    void refresh();
    void scheduleRefresh();
    [[nodiscard]] const katana::cad::SurveyJob* currentJob() const;
    void jobChosen();
    void viewReport();
    void editAdjustment();
    void previewAdjustment(std::function<void()> then);
    void applyAdjustment();
    void exportReport();
    void askRemove();
    void confirmRemove();
    void showHandEdits(const katana::cad::SurveyJob& job);
    void showShifts(const katana::survey::ReductionReport& report);
    [[nodiscard]] bool previewIsCurrent() const;
    [[nodiscard]] katana::core::Result<katana::survey::ReductionContext>
    contextFor(const katana::cad::SurveyJob& job) const;
    void showError(const katana::core::Error& error);
    void showMessage(const QString& text);

    katana::cad::Document& document_;
    std::function<void(const QString&, bool)> log_;

    QTreeWidget* jobs_ = nullptr;
    QLabel* jobSummary_ = nullptr;
    QComboBox* exportFormat_ = nullptr;
    QLineEdit* exportFile_ = nullptr;
    QWidget* removeRow_ = nullptr;
    QLabel* removeQuestion_ = nullptr;
    QCheckBox* removeWithPoints_ = nullptr;
    QStackedWidget* pages_ = nullptr;
    ReductionReportView* jobReport_ = nullptr;
    ReductionOptionsWidget* options_ = nullptr;
    QComboBox* handEdits_ = nullptr;
    QLabel* handEditsFound_ = nullptr;
    QPushButton* previewButton_ = nullptr;
    QPushButton* applyButton_ = nullptr;
    QLabel* shiftSummary_ = nullptr;
    QTableWidget* shifts_ = nullptr;
    ReductionReportView* oldReport_ = nullptr;
    ReductionReportView* newReport_ = nullptr;
    SurveyTaskBar* task_ = nullptr;
    QLabel* message_ = nullptr;

    // The job being edited: its id, the project its stored bytes read into
    // and what the reader said about them (for the report's Input), shared
    // with the pool threads that reduce it.
    std::string editingJob_;
    std::shared_ptr<const katana::surveyio::ReadResult> read_;
    // The last preview and what it was made from.
    std::shared_ptr<const katana::survey::ReductionOutcome> outcome_;
    katana::survey::ReductionSettings outcomeSettings_{};
    std::vector<katana::survey::ComputedPoint> outcomePrevious_;
    std::vector<katana::survey::SurveyPoint> outcomeDrawingPoints_;
    std::uint64_t outcomeRevision_ = 0;
    std::uint64_t readTicket_ = 0;

    std::uint64_t shownGeneration_ = ~std::uint64_t{0};
    bool refreshPending_ = false;
    // Last, so it goes first and no listener outlives what it calls.
    katana::cad::Document::ListenerHandle listener_;
};

// The HTML a view may lay out in reasonable time: a stored report longer
// than `maxBytes` is cut at the end of a table row and closed, with a line
// saying the whole report is in the export. Exposed for the tests.
[[nodiscard]] std::string viewableReportHtml(const std::string& html, std::size_t maxBytes);

} // namespace katana::qt
