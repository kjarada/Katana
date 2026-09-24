#include "survey/survey_jobs_dialog.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QDateTime>
#include <QFileDialog>
#include <QFileInfo>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QScrollArea>
#include <QSplitter>
#include <QStackedWidget>
#include <QTableWidget>
#include <QTimer>
#include <QTreeWidget>
#include <QVBoxLayout>

#include <algorithm>
#include <cmath>
#include <format>
#include <map>
#include <string_view>
#include <unordered_map>
#include <utility>

#include "katana/cad/survey_job.hpp"
#include "katana/cad/survey_points.hpp"
#include "katana/surveyio/reader.hpp"
#include "survey/reduction_options_widget.hpp"
#include "survey/reduction_report_view.hpp"
#include "survey/survey_dialogs.hpp"
#include "survey/survey_job_support.hpp"
#include "survey/survey_task.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace cad = katana::cad;
namespace survey = katana::survey;
namespace surveyio = katana::surveyio;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

QString qs(std::string_view text)
{
    return QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size()));
}

QLabel* mutedLabel(const QString& text, QWidget* parent)
{
    auto* label = new QLabel(text, parent);
    label->setWordWrap(true);
    label->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
    return label;
}

QPushButton* button(const QString& text, const char* name, const QString& tip, QWidget* parent)
{
    auto* b = new QPushButton(text, parent);
    b->setObjectName(name);
    b->setToolTip(tip);
    b->setAutoDefault(false);
    return b;
}

// Bytes of a stored report the view lays out; the export writes it whole.
constexpr std::size_t kViewedReportBytes = 4u << 20;

// A shift a person notices on a drawing, and one they must: 1 mm and 10 mm.
constexpr double kShiftNoticed = 0.001;
constexpr double kShiftLarge = 0.010;

// "2026-09-24 10:15" from an ISO 8601 time; the text as it is otherwise.
QString shortTime(const std::string& iso)
{
    if (iso.size() >= 16 && iso[10] == 'T') {
        return qs(iso.substr(0, 10)) + " " + qs(iso.substr(11, 5)) +
               (iso.ends_with('Z') ? " UTC" : "");
    }
    return qs(iso);
}

std::vector<survey::ComputedPoint> previousOf(const cad::SurveyJob& job)
{
    // As ReadjustSurveyJobCommand fills it: the job's points as it last
    // placed them, not where they are now.
    std::vector<survey::ComputedPoint> previous;
    previous.reserve(job.placedPoints.size());
    for (const cad::SurveyJobPoint& placed : job.placedPoints) {
        survey::ComputedPoint point;
        point.id = placed.pointId;
        point.northing = placed.northing;
        point.easting = placed.easting;
        point.elevation = placed.elevation;
        previous.push_back(std::move(point));
    }
    return previous;
}

} // namespace

std::string viewableReportHtml(const std::string& html, std::size_t maxBytes)
{
    if (html.size() <= maxBytes) {
        return html;
    }
    const std::size_t row = std::string_view(html).substr(0, maxBytes).rfind("</tr>");
    const std::size_t cut = row == std::string_view::npos ? maxBytes : row + 5;
    return html.substr(0, cut) +
           "</tbody></table></section>\n<p class=\"empty\">The report goes on past what this "
           "view shows: export it (Export Report) to read it whole.</p></body></html>\n";
}

SurveyJobsDialog::SurveyJobsDialog(const SurveyDialogContext& context, QWidget* parent)
    : QDialog(parent), document_(*context.document), log_(context.log)
{
    setObjectName("surveyJobsDialog");
    setWindowTitle("Survey Jobs");
    setModal(false);
    auto* layout = new QVBoxLayout(this);
    layout->addWidget(mutedLabel(
        "The field files imported as survey jobs. A job keeps its file as it was read, so it "
        "can be reduced and adjusted again with other settings: Edit Adjustment, Preview, "
        "Apply - one undo step.",
        this));

    jobs_ = new QTreeWidget(this);
    jobs_->setObjectName("jobs");
    jobs_->setHeaderLabels({"Job", "File", "Format", "Imported", "Method", "Points",
                            "Variance factor", "Global test"});
    jobs_->setRootIsDecorated(false);
    jobs_->setSelectionMode(QAbstractItemView::SingleSelection);
    jobs_->setMaximumHeight(120);
    jobs_->header()->setStretchLastSection(true);
    layout->addWidget(jobs_);
    jobSummary_ = mutedLabel({}, this);
    jobSummary_->setObjectName("jobSummary");
    layout->addWidget(jobSummary_);

    auto* verbs = new QHBoxLayout();
    auto* view = button("View Report", "viewReport", "The job's last reduction report", this);
    auto* edit = button("Edit Adjustment", "editAdjustment",
                        "Change the job's reduction and adjustment and re-run it", this);
    auto* remove = button("Remove Job...", "removeJob",
                          "Take the job off the list, with or without its points", this);
    exportFormat_ = new QComboBox(this);
    exportFormat_->setObjectName("exportFormat");
    exportFormat_->addItems({"HTML", "plain text", "PDF"});
    exportFormat_->setToolTip("The format, when the file name has no extension saying it");
    exportFile_ = new QLineEdit(this);
    exportFile_->setObjectName("exportFile");
    exportFile_->setPlaceholderText("file to export the report to");
    auto* browse = button("...", "browseExport", "Choose the file", this);
    browse->setMaximumWidth(36);
    auto* exportButton = button("Export Report", "exportReport",
                                "Write the job's report: HTML as kept, plain text, or PDF", this);
    verbs->addWidget(view);
    verbs->addWidget(edit);
    verbs->addWidget(remove);
    verbs->addSpacing(16);
    verbs->addWidget(exportFormat_);
    verbs->addWidget(exportFile_, 1);
    verbs->addWidget(browse);
    verbs->addWidget(exportButton);
    layout->addLayout(verbs);

    // Removal is confirmed here, on the page: a box would stop a headless run.
    removeRow_ = new QWidget(this);
    auto* removeLayout = new QHBoxLayout(removeRow_);
    removeLayout->setContentsMargins(0, 0, 0, 0);
    removeQuestion_ = new QLabel(removeRow_);
    removeQuestion_->setObjectName("removeQuestion");
    removeQuestion_->setWordWrap(true);
    removeQuestion_->setStyleSheet(QString("color: %1").arg(theme::error().name()));
    removeWithPoints_ = new QCheckBox(removeRow_);
    removeWithPoints_->setObjectName("removeWithPoints");
    auto* confirm = button("Remove", "confirmRemove", "Remove the job", removeRow_);
    auto* cancel = button("Keep It", "cancelRemove", "Leave the job as it is", removeRow_);
    removeLayout->addWidget(removeQuestion_, 1);
    removeLayout->addWidget(removeWithPoints_);
    removeLayout->addWidget(confirm);
    removeLayout->addWidget(cancel);
    removeRow_->setVisible(false);
    layout->addWidget(removeRow_);

    pages_ = new QStackedWidget(this);
    jobReport_ = new ReductionReportView("jobReport", pages_);
    pages_->addWidget(jobReport_);

    auto* editPage = new QSplitter(Qt::Horizontal, pages_);
    auto* scroll = new QScrollArea(editPage);
    scroll->setWidgetResizable(true);
    scroll->setFrameShape(QFrame::NoFrame);
    options_ = new ReductionOptionsWidget(scroll);
    scroll->setWidget(options_);
    auto* right = new QWidget(editPage);
    auto* rightLayout = new QVBoxLayout(right);
    rightLayout->setContentsMargins(0, 0, 0, 0);
    auto* applyRow = new QHBoxLayout();
    previewButton_ = button("Preview", "previewAdjustment",
                            "Read the stored file again, reduce and adjust it with these "
                            "settings, and show the new report beside the last one",
                            right);
    handEdits_ = new QComboBox(right);
    handEdits_->setObjectName("handEdits");
    handEdits_->addItem("keep the points changed by hand", static_cast<int>(cad::HandEditPolicy::Keep));
    handEdits_->addItem("overwrite them with the new coordinates",
                        static_cast<int>(cad::HandEditPolicy::Overwrite));
    handEdits_->setToolTip("What Apply does with the job's points moved, re-levelled or deleted "
                           "by hand since the job placed them");
    applyButton_ = button("Apply", "applyAdjustment",
                          "Re-adjust the job with these settings and move its points: one "
                          "undo step",
                          right);
    applyRow->addWidget(previewButton_);
    applyRow->addWidget(new QLabel("Points changed by hand:", right));
    applyRow->addWidget(handEdits_, 1);
    applyRow->addWidget(applyButton_);
    rightLayout->addLayout(applyRow);
    handEditsFound_ = mutedLabel({}, right);
    handEditsFound_->setObjectName("handEditsFound");
    rightLayout->addWidget(handEditsFound_);
    shiftSummary_ = mutedLabel("Press Preview to see how far each point would move.", right);
    shiftSummary_->setObjectName("shiftSummary");
    rightLayout->addWidget(shiftSummary_);
    shifts_ = new QTableWidget(0, 5, right);
    shifts_->setObjectName("shifts");
    shifts_->setHorizontalHeaderLabels(
        {"Point", "Shift N (mm)", "Shift E (mm)", "Shift H (mm)", "Horizontal (mm)"});
    shifts_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    shifts_->verticalHeader()->setVisible(false);
    shifts_->horizontalHeader()->setStretchLastSection(true);
    shifts_->setMaximumHeight(150);
    rightLayout->addWidget(shifts_);
    auto* reports = new QSplitter(Qt::Horizontal, right);
    auto* oldPane = new QWidget(reports);
    auto* oldLayout = new QVBoxLayout(oldPane);
    oldLayout->setContentsMargins(0, 0, 0, 0);
    oldLayout->addWidget(mutedLabel("The job's last run", oldPane));
    oldReport_ = new ReductionReportView("oldReport", oldPane);
    oldLayout->addWidget(oldReport_, 1);
    auto* newPane = new QWidget(reports);
    auto* newLayout = new QVBoxLayout(newPane);
    newLayout->setContentsMargins(0, 0, 0, 0);
    newLayout->addWidget(mutedLabel("This preview", newPane));
    newReport_ = new ReductionReportView("newReport", newPane);
    newLayout->addWidget(newReport_, 1);
    reports->addWidget(oldPane);
    reports->addWidget(newPane);
    rightLayout->addWidget(reports, 1);
    editPage->addWidget(scroll);
    editPage->addWidget(right);
    editPage->setStretchFactor(0, 0);
    editPage->setStretchFactor(1, 1);
    editPage->setSizes({430, 900});
    pages_->addWidget(editPage);
    layout->addWidget(pages_, 1);

    task_ = new SurveyTaskBar(this);
    layout->addWidget(task_);
    message_ = new QLabel(this);
    message_->setObjectName("message");
    message_->setWordWrap(true);
    message_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    layout->addWidget(message_);
    auto* closeRow = new QHBoxLayout();
    closeRow->addStretch(1);
    auto* close = button("Close", "close", "Close; the jobs stay with the drawing", this);
    closeRow->addWidget(close);
    layout->addLayout(closeRow);

    connect(jobs_, &QTreeWidget::currentItemChanged, this, [this] { jobChosen(); });
    connect(view, &QPushButton::clicked, this, [this] { viewReport(); });
    connect(edit, &QPushButton::clicked, this, [this] { editAdjustment(); });
    connect(remove, &QPushButton::clicked, this, [this] { askRemove(); });
    connect(confirm, &QPushButton::clicked, this, [this] { confirmRemove(); });
    connect(cancel, &QPushButton::clicked, this, [this] { removeRow_->setVisible(false); });
    connect(exportButton, &QPushButton::clicked, this, [this] { exportReport(); });
    connect(browse, &QPushButton::clicked, this, [this] {
        const QString path = QFileDialog::getSaveFileName(
            this, "Export Reduction Report", exportFile_->text(),
            "HTML (*.html);;Plain text (*.txt);;PDF (*.pdf)");
        if (!path.isEmpty()) {
            exportFile_->setText(path);
        }
    });
    connect(previewButton_, &QPushButton::clicked, this, [this] { previewAdjustment({}); });
    connect(applyButton_, &QPushButton::clicked, this, [this] { applyAdjustment(); });
    connect(close, &QPushButton::clicked, this, [this] { hide(); });
    options_->onChanged = [this] {
        if (outcome_ != nullptr && !previewIsCurrent()) {
            newReport_->showNote("The settings have changed since this preview: press Preview "
                                 "again.");
        }
    };
    task_->onBusyChanged = [this](bool busy) {
        previewButton_->setEnabled(!busy);
        applyButton_->setEnabled(!busy);
    };
    task_->onCancelled = [this] { showMessage("Cancelled: nothing was changed."); };

    listener_ = document_.addListener([this] { scheduleRefresh(); });
    refresh();
    resize(1180, 780);
}

SurveyJobsDialog::~SurveyJobsDialog() = default;

void SurveyJobsDialog::scheduleRefresh()
{
    // On the event loop, never inside the document's listener: the list is
    // rebuilt, and a listener must not change what called it.
    if (refreshPending_) {
        return;
    }
    refreshPending_ = true;
    QTimer::singleShot(0, this, [this] {
        refreshPending_ = false;
        refresh();
    });
}

void SurveyJobsDialog::refresh()
{
    if (document_.surveyJobsGeneration() == shownGeneration_) {
        return;
    }
    shownGeneration_ = document_.surveyJobsGeneration();
    const QString chosen =
        jobs_->currentItem() != nullptr ? jobs_->currentItem()->text(0) : QString();
    jobs_->blockSignals(true);
    jobs_->clear();
    QTreeWidgetItem* again = nullptr;
    for (const cad::SurveyJob& job : document_.surveyJobs()) {
        auto* item = new QTreeWidgetItem(jobs_);
        const auto descriptor = surveyio::formatRegistry().find(job.formatId);
        const JobAdjustmentLine last = adjustmentFromReportText(job.reportText);
        item->setText(0, qs(job.id));
        item->setText(1, qs(job.sourceFileName));
        item->setText(2, descriptor ? qs(descriptor->humanName) : qs(job.formatId));
        item->setText(3, shortTime(job.importedUtc));
        item->setText(4, qs(methodText(job.settings)));
        item->setText(5, QString::number(job.placedPoints.size()));
        item->setText(6, last.varianceFactor ? qs(*last.varianceFactor) : QString("-"));
        item->setText(7, last.globalTest ? qs(*last.globalTest) : QString("-"));
        if (last.globalTest && last.globalTest->find("FAILED") != std::string::npos) {
            item->setForeground(7, theme::error());
        }
        if (item->text(0) == chosen) {
            again = item;
        }
    }
    jobs_->blockSignals(false);
    for (int column = 0; column < jobs_->columnCount() - 1; ++column) {
        jobs_->resizeColumnToContents(column);
    }
    if (again == nullptr && jobs_->topLevelItemCount() > 0) {
        again = jobs_->topLevelItem(jobs_->topLevelItemCount() - 1);
    }
    if (again != nullptr) {
        jobs_->setCurrentItem(again);
    }
    // A job being edited that is gone (an undo of its import) ends the edit.
    if (!editingJob_.empty() && document_.findSurveyJob(editingJob_) == nullptr) {
        editingJob_.clear();
        read_.reset();
        outcome_.reset();
        pages_->setCurrentIndex(0);
    }
    jobChosen();
}

const cad::SurveyJob* SurveyJobsDialog::currentJob() const
{
    const QTreeWidgetItem* item = jobs_->currentItem();
    return item == nullptr ? nullptr : document_.findSurveyJob(item->text(0).toStdString());
}

void SurveyJobsDialog::jobChosen()
{
    const cad::SurveyJob* job = currentJob();
    if (job == nullptr) {
        jobSummary_->setText(document_.surveyJobs().empty()
                                 ? "No survey jobs yet: Survey > Import Survey Points reads a "
                                   "field file as one."
                                 : "Choose a job.");
        jobReport_->showNote("No job chosen.");
        return;
    }
    QString siblings;
    for (const cad::SurveyJobFile& file : job->siblingFiles) {
        siblings += (siblings.isEmpty() ? "" : ", ") + qs(file.name);
    }
    jobSummary_->setText(
        QString("%1: %2 on layer %3, %4 point(s); parser %5; last reduced %6%7.")
            .arg(qs(job->id), qs(job->name), qs(job->layer))
            .arg(job->placedPoints.size())
            .arg(qs(job->parserVersion), shortTime(job->reportCreatedUtc),
                 siblings.isEmpty() ? QString() : "; with " + siblings));
    if (pages_->currentIndex() == 0) {
        jobReport_->setReportHtml(viewableReportHtml(job->reportHtml, kViewedReportBytes));
    }
}

void SurveyJobsDialog::viewReport()
{
    const cad::SurveyJob* job = currentJob();
    if (job == nullptr) {
        showMessage("Choose a job first.");
        return;
    }
    pages_->setCurrentIndex(0);
    jobReport_->setReportHtml(viewableReportHtml(job->reportHtml, kViewedReportBytes));
}

void SurveyJobsDialog::editAdjustment()
{
    const cad::SurveyJob* job = currentJob();
    if (job == nullptr) {
        showMessage("Choose a job first.");
        return;
    }
    editingJob_ = job->id;
    read_.reset();
    outcome_.reset();
    pages_->setCurrentIndex(1);
    options_->setSettings(job->settings);
    oldReport_->setReportHtml(viewableReportHtml(job->reportHtml, kViewedReportBytes));
    newReport_->showNote("Press Preview to reduce and adjust the job with these settings.");
    shifts_->setRowCount(0);
    shiftSummary_->setText("Press Preview to see how far each point would move.");
    showHandEdits(*job);
    if (auto context = cad::reductionContextFor(document_)) {
        options_->setDrawingPoints(context->drawingPoints);
    } else {
        showError(context.error());
    }

    // The stored bytes, read again as the re-adjustment will read them. The
    // pool thread gets copies: the job itself is the document's, which an
    // undo may change while the thread runs.
    const std::string jobId = job->id;
    const std::string formatId = job->formatId;
    const std::string name = job->sourceFileName;
    auto bytes = std::make_shared<const std::string>(job->sourceBytes);
    std::vector<surveyio::SiblingFile> files;
    for (const cad::SurveyJobFile& file : job->siblingFiles) {
        files.push_back({file.name, file.bytes});
    }
    const std::uint64_t ticket = ++readTicket_;
    task_->run(QString("Reading the stored %1 again").arg(qs(name)),
               static_cast<std::int64_t>(bytes->size()) > kBackgroundReadBytes,
               [this, jobId, formatId, name, bytes, files, ticket]() -> SurveyTaskBar::Finish {
                   surveyio::ReadOptions options;
                   options.siblings = surveyio::siblingsInMemory(files);
                   auto read = surveyio::readSurvey(surveyio::formatRegistry(), formatId, *bytes,
                                                    name, options);
                   if (!read) {
                       return [this, error = read.error()] { showError(error); };
                   }
                   auto result = std::make_shared<const surveyio::ReadResult>(std::move(*read));
                   return [this, result, jobId, ticket] {
                       if (ticket != readTicket_ || editingJob_ != jobId) {
                           return;
                       }
                       read_ = result;
                       options_->setProject(result->project);
                       showMessage(QString("Read the job's stored file again with parser %1: "
                                           "%2 record(s), %3 warning(s).")
                                       .arg(qs(result->parserVersion))
                                       .arg(result->recordsRead)
                                       .arg(result->warnings.size()));
                   };
               });
}

Result<survey::ReductionContext> SurveyJobsDialog::contextFor(const cad::SurveyJob& job) const
{
    auto context = cad::reductionContextFor(document_);
    if (!context) {
        return context.error();
    }
    context->input = reportInputFor(*read_, job.sourceFileName);
    context->createdUtc = QDateTime::currentDateTimeUtc().toString(Qt::ISODate).toStdString();
    context->previous = previousOf(job);
    return context;
}

bool SurveyJobsDialog::previewIsCurrent() const
{
    const cad::SurveyJob* job = document_.findSurveyJob(editingJob_);
    if (outcome_ == nullptr || job == nullptr ||
        outcomeRevision_ != document_.modelRevision() || outcomePrevious_ != previousOf(*job)) {
        return false;
    }
    const auto settings = options_->settings();
    return settings && *settings == outcomeSettings_;
}

void SurveyJobsDialog::previewAdjustment(std::function<void()> then)
{
    const cad::SurveyJob* job = document_.findSurveyJob(editingJob_);
    if (job == nullptr) {
        showMessage("Choose a job and press Edit Adjustment first.");
        return;
    }
    if (read_ == nullptr) {
        showMessage("The job's stored file is still being read.");
        return;
    }
    const auto settings = options_->settings();
    if (!settings) {
        showError(settings.error());
        return;
    }
    auto context = contextFor(*job);
    if (!context) {
        showError(context.error());
        return;
    }
    const std::shared_ptr<const survey::SurveyProject> raw(read_, &read_->project);
    const std::size_t size = surveySize(*raw);
    const std::string jobId = job->id;
    const std::uint64_t revision = document_.modelRevision();
    std::vector<survey::ComputedPoint> previous = context->previous;
    std::vector<survey::SurveyPoint> drawingPoints = context->drawingPoints;
    newReport_->showNote("Reducing and adjusting...");
    task_->run(
        QString("Reducing and adjusting %1 observation(s) and point(s) of %2")
            .arg(size)
            .arg(qs(job->sourceFileName)),
        size > kBackgroundObservations,
        [this, raw, settings = *settings, context = std::move(*context), jobId, revision,
         previous = std::move(previous), drawingPoints = std::move(drawingPoints),
         then = std::move(then)]() mutable -> SurveyTaskBar::Finish {
            auto outcome = survey::reduceAndAdjust(*raw, settings, context);
            if (!outcome) {
                return [this, error = outcome.error()] {
                    newReport_->showNote("The reduction could not run: " + qs(error.describe()));
                    showError(error);
                };
            }
            auto shared = std::make_shared<const survey::ReductionOutcome>(std::move(*outcome));
            std::string html =
                survey::renderHtml(displayReport(shared->report, kReportRowsOnScreen));
            return [this, shared, html = std::move(html), settings, jobId, revision,
                    previous = std::move(previous), drawingPoints = std::move(drawingPoints),
                    then = std::move(then)]() mutable {
                if (editingJob_ != jobId) {
                    return;
                }
                outcome_ = shared;
                outcomeSettings_ = settings;
                outcomePrevious_ = std::move(previous);
                outcomeDrawingPoints_ = std::move(drawingPoints);
                outcomeRevision_ = revision;
                newReport_->setReportHtml(html);
                showShifts(shared->report);
                showMessage(QString("Preview: %1 point(s); %2; %3 observation(s) rejected.")
                                .arg(shared->points.size())
                                .arg(qs(adjustmentSummary(shared->report)))
                                .arg(rejectedObservations(shared->report)));
                if (then) {
                    then();
                }
            };
        });
}

void SurveyJobsDialog::applyAdjustment()
{
    const cad::SurveyJob* job = document_.findSurveyJob(editingJob_);
    if (job == nullptr) {
        showMessage("Choose a job and press Edit Adjustment first.");
        return;
    }
    // Apply is what the preview showed: without a current one, it is run
    // first (on a pool thread when large) and Apply follows it.
    if (!previewIsCurrent()) {
        previewAdjustment([this] { applyAdjustment(); });
        return;
    }
    auto context = contextFor(*job);
    if (!context) {
        showError(context.error());
        return;
    }
    const std::string jobId = job->id;
    const std::string file = job->sourceFileName;
    const auto policy = static_cast<cad::HandEditPolicy>(handEdits_->currentData().toInt());
    cad::SurveyJobReadjustment request;
    request.jobId = jobId;
    request.settings = outcomeSettings_;
    request.context = std::move(*context);
    request.handEdits = policy;
    request.parserVersion = read_->parserVersion;
    auto command = std::make_unique<cad::ReadjustSurveyJobCommand>(
        document_, std::move(request),
        cachedJobReader(jobId, std::shared_ptr<const survey::SurveyProject>(read_, &read_->project)),
        precomputedReduction({outcome_, outcomeSettings_, outcomePrevious_, outcomeDrawingPoints_}));
    const cad::ReadjustSurveyJobCommand* readjust = command.get();
    if (const auto status = document_.execute(std::move(command)); !status) {
        showError(status.error());
        return;
    }
    // The command is on the undo stack now, alive, and says what it did.
    const cad::SurveyJobChanges& changes = readjust->changes();
    const survey::ReductionReport* report = readjust->report();
    const QString policyText = policy == cad::HandEditPolicy::Keep ? "kept" : "overwritten";
    log_(QString("Re-adjusted survey job %1 (%2): %3; %4 point(s) moved, %5 created, %6 removed, "
                 "%7 changed by hand and %8 deleted by hand (%9), %10 not drawn. One command - "
                 "Undo puts the job and its points back.")
             .arg(qs(jobId), qs(file),
                  report != nullptr ? qs(adjustmentSummary(*report)) : QString("-"))
             .arg(changes.moved.size())
             .arg(changes.created.size())
             .arg(changes.removed.size())
             .arg(changes.editedByHand.size())
             .arg(changes.deletedByHand.size())
             .arg(policyText)
             .arg(changes.notDrawn.size()),
         false);
    showMessage("Applied: the job's points and report are the preview's now. Undo puts them "
                "back.");
    if (const cad::SurveyJob* after = document_.findSurveyJob(jobId)) {
        oldReport_->setReportHtml(viewableReportHtml(after->reportHtml, kViewedReportBytes));
        showHandEdits(*after);
    }
    outcome_.reset();
    newReport_->showNote("Applied. Change the settings and press Preview to try others.");
}

void SurveyJobsDialog::showHandEdits(const cad::SurveyJob& job)
{
    // The job's points as the drawing has them now, against where the job
    // placed them: what Apply will ask about.
    std::unordered_map<katana::entity::EntityId, const cad::DrawingSurveyPoint*> byEntity;
    const std::vector<cad::DrawingSurveyPoint> drawing = cad::drawingSurveyPoints(document_);
    for (const cad::DrawingSurveyPoint& point : drawing) {
        byEntity.emplace(point.entity, &point);
    }
    QStringList moved;
    QStringList deleted;
    for (const cad::SurveyJobPoint& placed : job.placedPoints) {
        const auto found = byEntity.find(placed.entity);
        if (found == byEntity.end()) {
            deleted << qs(placed.pointId);
            continue;
        }
        const cad::DrawingSurveyPoint& now = *found->second;
        if (now.northing != placed.northing || now.easting != placed.easting ||
            now.elevation != placed.elevation) {
            moved << qs(placed.pointId);
        }
    }
    const auto listed = [](const QStringList& ids) {
        return ids.size() <= 8 ? ids.join(", ") : ids.mid(0, 8).join(", ") + ", ...";
    };
    if (moved.isEmpty() && deleted.isEmpty()) {
        handEditsFound_->setText("None of the job's points has been changed by hand since it "
                                 "placed them.");
        handEditsFound_->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
        return;
    }
    QString text = "Changed by hand since the job placed them: ";
    if (!moved.isEmpty()) {
        text += QString("%1 moved or re-levelled (%2)").arg(moved.size()).arg(listed(moved));
    }
    if (!deleted.isEmpty()) {
        text += QString("%1%2 deleted (%3)")
                    .arg(moved.isEmpty() ? "" : "; ")
                    .arg(deleted.size())
                    .arg(listed(deleted));
    }
    text += ". Choose beside Apply whether the edits stand.";
    handEditsFound_->setText(text);
    handEditsFound_->setStyleSheet(QString("color: %1").arg(theme::accent().name()));
}

void SurveyJobsDialog::showShifts(const survey::ReductionReport& report)
{
    struct Row {
        const survey::CoordinateReport* point;
        double horizontal;
    };
    std::vector<Row> rows;
    std::size_t fresh = 0;
    for (const survey::CoordinateReport& point : report.coordinates) {
        if (!point.shiftNorthing && !point.shiftEasting && !point.shiftElevation) {
            ++fresh;
            continue;
        }
        rows.push_back({&point, std::hypot(point.shiftNorthing.value_or(0.0),
                                           point.shiftEasting.value_or(0.0))});
    }
    // The largest first: what a person needs to see is what moves.
    std::ranges::stable_sort(rows, [](const Row& a, const Row& b) {
        const double va = std::max(a.horizontal, std::abs(a.point->shiftElevation.value_or(0.0)));
        const double vb = std::max(b.horizontal, std::abs(b.point->shiftElevation.value_or(0.0)));
        return va > vb;
    });
    const cad::SurveyJob* job = document_.findSurveyJob(editingJob_);
    std::size_t gone = 0;
    if (job != nullptr) {
        std::unordered_map<std::string_view, bool> computed;
        for (const survey::CoordinateReport& point : report.coordinates) {
            computed.emplace(point.pointId, true);
        }
        for (const cad::SurveyJobPoint& placed : job->placedPoints) {
            gone += computed.contains(placed.pointId) ? 0 : 1;
        }
    }
    shifts_->setRowCount(static_cast<int>(rows.size()));
    std::size_t noticed = 0;
    double largest = 0.0;
    QString largestId;
    const auto mm = [](const std::optional<double>& metres) {
        return metres ? QString::number(*metres * 1000.0, 'f', 1) : QString("-");
    };
    for (std::size_t i = 0; i < rows.size(); ++i) {
        const survey::CoordinateReport& point = *rows[i].point;
        const double vertical = std::abs(point.shiftElevation.value_or(0.0));
        const double worst = std::max(rows[i].horizontal, vertical);
        const QStringList cells{qs(point.pointId), mm(point.shiftNorthing),
                                mm(point.shiftEasting), mm(point.shiftElevation),
                                QString::number(rows[i].horizontal * 1000.0, 'f', 1)};
        for (int column = 0; column < cells.size(); ++column) {
            auto* item = new QTableWidgetItem(cells[column]);
            if (worst >= kShiftNoticed) {
                // Highlighted: the accent for a shift a person would see, the
                // error colour for one they must look at.
                item->setForeground(worst >= kShiftLarge ? theme::error() : theme::accent());
                QFont bold = item->font();
                bold.setBold(true);
                item->setFont(bold);
            }
            shifts_->setItem(static_cast<int>(i), column, item);
        }
        if (worst >= kShiftNoticed) {
            ++noticed;
        }
        if (worst > largest) {
            largest = worst;
            largestId = qs(point.pointId);
        }
    }
    shifts_->resizeColumnsToContents();
    QString text = QString("%1 point(s) against the job's last run: %2 move by 1 mm or more")
                       .arg(rows.size())
                       .arg(noticed);
    if (noticed > 0) {
        text += QString(" (the most %1 mm, %2)").arg(largest * 1000.0, 0, 'f', 1).arg(largestId);
    }
    text += QString("; %1 new; %2 no longer computed.").arg(fresh).arg(gone);
    shiftSummary_->setText(text);
    shiftSummary_->setStyleSheet(QString("color: %1").arg(
        noticed > 0 ? theme::accent().name() : theme::textMuted().name()));
}

void SurveyJobsDialog::exportReport()
{
    const cad::SurveyJob* job = currentJob();
    if (job == nullptr) {
        showMessage("Choose a job first.");
        return;
    }
    QString path = exportFile_->text().trimmed();
    std::optional<ReportFileFormat> format = reportFormatOf(path);
    if (!format && !path.isEmpty()) {
        // No extension that says it: the format chosen beside, and its
        // extension added.
        static constexpr ReportFileFormat kChoices[] = {ReportFileFormat::Html,
                                                        ReportFileFormat::Text,
                                                        ReportFileFormat::Pdf};
        static constexpr const char* kExtensions[] = {".html", ".txt", ".pdf"};
        const int index = std::clamp(exportFormat_->currentIndex(), 0, 2);
        format = kChoices[index];
        path += kExtensions[index];
    }
    const auto status = katana::qt::exportReport(*job, path, format.value_or(ReportFileFormat::Html));
    if (!status) {
        showError(status.error());
        return;
    }
    const char* formatName = *format == ReportFileFormat::Html   ? "HTML"
                             : *format == ReportFileFormat::Text ? "plain text"
                                                                 : "PDF";
    log_(QString("Exported the reduction report of survey job %1 (%2) as %3 to %4")
             .arg(qs(job->id), qs(job->sourceFileName), formatName,
                  QFileInfo(path).fileName()),
         false);
    showMessage(QString("Exported to %1.").arg(path));
}

void SurveyJobsDialog::askRemove()
{
    const cad::SurveyJob* job = currentJob();
    if (job == nullptr) {
        showMessage("Choose a job first.");
        return;
    }
    removeQuestion_->setText(QString("Remove survey job %1 (%2)? Its stored file, settings and "
                                     "report go with it; Undo brings them back.")
                                 .arg(qs(job->id), qs(job->sourceFileName)));
    removeWithPoints_->setText(QString("and its %1 point(s) on the drawing")
                                   .arg(job->placedPoints.size()));
    removeWithPoints_->setChecked(false);
    removeRow_->setVisible(true);
}

void SurveyJobsDialog::confirmRemove()
{
    const cad::SurveyJob* job = currentJob();
    if (job == nullptr || !removeRow_->isVisible()) {
        return;
    }
    const std::string id = job->id;
    const std::string file = job->sourceFileName;
    const bool withPoints = removeWithPoints_->isChecked();
    removeRow_->setVisible(false);
    if (const auto status = document_.execute(
            std::make_unique<cad::RemoveSurveyJobCommand>(document_, id, withPoints));
        !status) {
        showError(status.error());
        return;
    }
    log_(QString("Removed survey job %1 (%2)%3. One command - Undo puts it back.")
             .arg(qs(id), qs(file),
                  withPoints ? QString(" with its points")
                             : QString("; its points stay as ordinary survey points")),
         false);
}

void SurveyJobsDialog::showError(const katana::core::Error& error)
{
    QString text = qs(error.message);
    if (!error.context.empty()) {
        text += " - " + qs(error.context);
    }
    message_->setStyleSheet(QString("color: %1").arg(theme::error().name()));
    message_->setText(text);
    log_("Survey Jobs: " + qs(error.describe()), true);
}

void SurveyJobsDialog::showMessage(const QString& text)
{
    message_->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
    message_->setText(text);
}

} // namespace katana::qt
