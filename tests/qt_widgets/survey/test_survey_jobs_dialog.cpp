// Survey > Survey Jobs, driven by its object names: a job imported with a
// network adjustment is edited, previewed (the new report beside the old,
// each point's shift listed), applied as one undo step and undone; its report
// is exported as HTML, text and PDF; it is removed after a confirmation on
// the page; and the list reads a job's last adjustment back from its report.

#include <gtest/gtest.h>

#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QFile>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QTableWidget>
#include <QTemporaryDir>
#include <QTreeWidget>

#include <algorithm>
#include <filesystem>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/survey_job.hpp"
#include "katana/survey/reduction_report.hpp"
#include "katana/surveyio/reader.hpp"
#include "survey/reduction_report_view.hpp"
#include "survey/survey_dialogs.hpp"
#include "survey/survey_job_support.hpp"
#include "survey/survey_jobs_dialog.hpp"

namespace cad = katana::cad;
namespace survey = katana::survey;
namespace surveyio = katana::surveyio;
using katana::cad::Document;
using katana::qt::SurveyJobsDialog;

namespace {

template <typename T> T* child(QWidget& parent, const char* name)
{
    T* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << "no " << name;
    return found;
}

void click(QWidget& parent, const char* name)
{
    auto* button = child<QPushButton>(parent, name);
    ASSERT_NE(button, nullptr);
    ASSERT_TRUE(button->isEnabled()) << name << " is disabled";
    button->click();
    QApplication::processEvents();
}

void choose(QWidget& parent, const char* name, const QString& item)
{
    auto* box = child<QComboBox>(parent, name);
    ASSERT_NE(box, nullptr);
    const int index = box->findText(item);
    ASSERT_GE(index, 0) << name << " has no " << item.toStdString();
    box->setCurrentIndex(index);
}

std::string readFile(const std::string& path)
{
    QFile file(QString::fromStdString(path));
    EXPECT_TRUE(file.open(QIODevice::ReadOnly)) << path;
    const QByteArray bytes = file.readAll();
    return std::string(bytes.constData(), static_cast<std::size_t>(bytes.size()));
}

// network_metres.rw5 imported as a job: a setup on A backsighting B and a
// setup on B backsighting A, each shooting P and Q, with A and B held fixed
// in a horizontal network - what the wizard does, through the same command.
std::string importNetworkJob(Document& document)
{
    const std::string name = "network_metres.rw5";
    std::string bytes = readFile(std::string(KATANA_SURVEY_UI_DATA) + "/" + name);
    auto read = surveyio::readSurvey(surveyio::formatRegistry(), "tds-rw5", bytes, name);
    EXPECT_TRUE(read.ok()) << (read ? "" : read.error().describe());
    cad::SurveyJobImport request;
    request.job.name = name;
    request.job.formatId = read->formatId;
    request.job.parserVersion = read->parserVersion;
    request.job.sourceFileName = name;
    request.job.sourceBytes = bytes;
    request.job.settings.method = survey::AdjustmentMethod::Network;
    request.job.settings.control = {
        {survey::ControlPoint::fixed3d("A"), survey::ControlOrigin::File},
        {survey::ControlPoint::fixed3d("B"), survey::ControlOrigin::File}};
    request.job.layer = request.importOptions.layer;
    request.raw = read->project;
    auto context = cad::reductionContextFor(document);
    EXPECT_TRUE(context.ok());
    request.context = std::move(*context);
    request.context.input = katana::qt::reportInputFor(*read, name);
    request.context.createdUtc = "2026-09-24T10:00:00Z";
    auto command = std::make_unique<cad::ImportSurveyJobCommand>(document, std::move(request));
    const cad::ImportSurveyJobCommand* import = command.get();
    const auto status = document.execute(std::move(command));
    EXPECT_TRUE(status.ok()) << (status ? "" : status.error().describe());
    return import->jobId();
}

struct Session {
    Document document;
    std::vector<QString> log;
    std::unique_ptr<SurveyJobsDialog> dialog;
    std::string jobId;

    Session()
    {
        jobId = importNetworkJob(document);
        katana::qt::SurveyDialogContext context{&document, nullptr,
                                                [this](const QString& text, bool) {
                                                    log.push_back(text);
                                                }};
        dialog = std::make_unique<SurveyJobsDialog>(context, nullptr);
        dialog->show();
        QApplication::processEvents();
    }

    [[nodiscard]] bool logged(const QString& pattern) const
    {
        return std::ranges::any_of(log, [&](const QString& line) { return line.contains(pattern); });
    }
};

} // namespace

TEST(SurveyJobsDialog, TheListShowsEachJobWithItsMethodPointsAndLastAdjustment)
{
    Session session;
    auto* jobs = child<QTreeWidget>(*session.dialog, "jobs");
    ASSERT_EQ(jobs->topLevelItemCount(), 1);
    const QTreeWidgetItem* row = jobs->topLevelItem(0);
    EXPECT_EQ(row->text(0).toStdString(), session.jobId);
    EXPECT_EQ(row->text(1), "network_metres.rw5");
    EXPECT_EQ(row->text(4), "network, horizontal");
    EXPECT_EQ(row->text(5), "4"); // A, B, P, Q
    // The variance factor and global test as the job's own report states them.
    const auto& job = session.document.surveyJobs().front();
    const auto last = katana::qt::adjustmentFromReportText(job.reportText);
    ASSERT_TRUE(last.varianceFactor.has_value());
    EXPECT_EQ(row->text(6).toStdString(), *last.varianceFactor);
    EXPECT_EQ(row->text(7).toStdString(), last.globalTest.value_or("-"));
}

TEST(SurveyJobsDialog, EditPreviewApplyIsOneUndoStepAndTheNewReportIsShownBesideTheOld)
{
    Session session;
    QWidget& d = *session.dialog;
    const cad::SurveyJob before = session.document.surveyJobs().front();
    click(d, "editAdjustment");
    // B no longer fixed but weighted 20 mm: the network is free to move it.
    choose(d, "controlPick", "B");
    choose(d, "controlHorizontal", "weighted");
    child<QLineEdit>(d, "controlSigmaHorizontal")->setText("20");
    click(d, "addControl");
    click(d, "previewAdjustment");

    auto* shifts = child<QTableWidget>(d, "shifts");
    EXPECT_EQ(shifts->rowCount(), 4) << "a shift for each of A, B, P and Q";
    const QString summary = child<QLabel>(d, "shiftSummary")->text();
    EXPECT_TRUE(summary.contains("against the job's last run")) << summary.toStdString();
    auto* newSections = d.findChild<QWidget*>("newReportSections");
    auto* oldSections = d.findChild<QWidget*>("oldReportSections");
    ASSERT_NE(newSections, nullptr);
    ASSERT_NE(oldSections, nullptr);
    // The preview changes nothing yet.
    EXPECT_EQ(session.document.surveyJobs().front(), before);

    click(d, "applyAdjustment");
    const cad::SurveyJob after = session.document.surveyJobs().front();
    ASSERT_EQ(after.settings.control.size(), 2u);
    EXPECT_EQ(after.settings.control[1].point.northing.constraint,
              survey::ControlConstraint::Weighted);
    EXPECT_EQ(after.settings.control[1].point.northing.sigma, 0.020);
    EXPECT_NE(after.reportText, before.reportText);
    EXPECT_TRUE(session.logged("Re-adjusted survey job " + QString::fromStdString(session.jobId)))
        << (session.log.empty() ? "" : session.log.back().toStdString());

    // One undo puts the settings, the report and the points back.
    ASSERT_TRUE(session.document.undo().ok());
    EXPECT_EQ(session.document.surveyJobs().front(), before);
    QApplication::processEvents();
    QApplication::processEvents();
    const auto last = katana::qt::adjustmentFromReportText(before.reportText);
    EXPECT_EQ(child<QTreeWidget>(d, "jobs")->topLevelItem(0)->text(6).toStdString(),
              last.varianceFactor.value_or("-"));
}

TEST(SurveyJobsDialog, TheReportIsExportedAsHtmlTextAndPdf)
{
    Session session;
    QWidget& d = *session.dialog;
    QTemporaryDir folder;
    ASSERT_TRUE(folder.isValid());
    const cad::SurveyJob& job = session.document.surveyJobs().front();
    child<QLineEdit>(d, "exportFile")->setText(folder.filePath("report.html"));
    click(d, "exportReport");
    EXPECT_EQ(readFile(folder.filePath("report.html").toStdString()), job.reportHtml);
    // No extension: the format chosen beside it names one.
    child<QLineEdit>(d, "exportFile")->setText(folder.filePath("report"));
    choose(d, "exportFormat", "plain text");
    click(d, "exportReport");
    EXPECT_EQ(readFile(folder.filePath("report.txt").toStdString()), job.reportText);
    child<QLineEdit>(d, "exportFile")->setText(folder.filePath("report.pdf"));
    click(d, "exportReport");
    const std::string pdf = readFile(folder.filePath("report.pdf").toStdString());
    EXPECT_TRUE(pdf.starts_with("%PDF-")) << pdf.substr(0, 16);
    EXPECT_TRUE(session.logged("as PDF to report.pdf"));
}

TEST(SurveyJobsDialog, RemovingAJobIsAskedOnThePageAndIsOneUndoStep)
{
    Session session;
    QWidget& d = *session.dialog;
    const std::size_t entities = session.document.surveyJobs().front().placedPoints.size();
    click(d, "removeJob");
    auto* question = child<QLabel>(d, "removeQuestion");
    EXPECT_TRUE(question->isVisible());
    EXPECT_TRUE(question->text().contains(QString::fromStdString(session.jobId)));
    // Keep It changes nothing.
    click(d, "cancelRemove");
    EXPECT_FALSE(question->isVisible());
    EXPECT_EQ(session.document.surveyJobs().size(), 1u);

    click(d, "removeJob");
    child<QCheckBox>(d, "removeWithPoints")->setChecked(true);
    click(d, "confirmRemove");
    EXPECT_TRUE(session.document.surveyJobs().empty());
    EXPECT_TRUE(session.logged("Removed survey job " + QString::fromStdString(session.jobId) +
                               " (network_metres.rw5) with its points"));
    QApplication::processEvents();
    EXPECT_EQ(child<QTreeWidget>(d, "jobs")->topLevelItemCount(), 0);
    ASSERT_TRUE(session.document.undo().ok());
    ASSERT_EQ(session.document.surveyJobs().size(), 1u);
    EXPECT_EQ(session.document.surveyJobs().front().placedPoints.size(), entities);
}

TEST(SurveyJobsDialog, TheLastAdjustmentIsReadBackFromTheReportsOwnText)
{
    // A report with one adjustment, rendered by the report's own renderer:
    // variance factor 1.25 (exact in binary) prints to three places as 1.250,
    // and 0.5 as 0.500; the global test as passed or FAILED.
    survey::ReductionReport report;
    survey::AdjustmentReport adjustment;
    adjustment.method = "network least squares (horizontal)";
    adjustment.redundancy = 4;
    adjustment.varianceFactor = 1.25;
    adjustment.globalTest = survey::ReportGlobalTest{5.0, 0.484, 11.143, 0.05, true};
    report.adjustments.push_back(adjustment);
    const auto line = katana::qt::adjustmentFromReportText(survey::renderText(report));
    EXPECT_EQ(line.varianceFactor, "1.250");
    EXPECT_EQ(line.globalTest, "passed");

    report.adjustments.front().globalTest->passed = false;
    report.adjustments.push_back(adjustment);
    report.adjustments.back().varianceFactor = 0.5;
    const auto two = katana::qt::adjustmentFromReportText(survey::renderText(report));
    EXPECT_EQ(two.varianceFactor, "1.250 / 0.500");
    EXPECT_EQ(two.globalTest, "FAILED / passed");
    // Radiation: nothing to read back.
    const auto none = katana::qt::adjustmentFromReportText(survey::renderText({}));
    EXPECT_FALSE(none.varianceFactor.has_value());
    EXPECT_FALSE(none.globalTest.has_value());
}

TEST(SurveyJobsDialog, AResectionsLeastSquaresIsNotReadBackAsTheJobsAdjustment)
{
    // A job reduced by radiation whose reduction resected one setup: the
    // resection's variance factor and global test are its own, not the job's
    // last adjustment, so the list reads none back; and the screen's cut of
    // the report keeps the resection, with its residuals.
    survey::ReductionReport report;
    survey::ResectionReport resection;
    resection.stationId = "S1";
    resection.pointId = "R";
    resection.targets = {"A", "B", "C"};
    resection.horizontal.method = "resection at S1 (horizontal)";
    resection.horizontal.redundancy = 3;
    resection.horizontal.varianceFactor = 1.25;
    resection.horizontal.globalTest = survey::ReportGlobalTest{3.75, 0.216, 9.348, 0.05, true};
    resection.horizontal.residuals.resize(6);
    report.resections.push_back(resection);
    const std::string text = survey::renderText(report);
    EXPECT_NE(text.find("resection at S1 (horizontal)"), std::string::npos);
    const auto line = katana::qt::adjustmentFromReportText(text);
    EXPECT_FALSE(line.varianceFactor.has_value());
    EXPECT_FALSE(line.globalTest.has_value());

    const survey::ReductionReport shown = katana::qt::displayReport(report, 4);
    ASSERT_EQ(shown.resections.size(), 1u);
    EXPECT_EQ(shown.resections[0].horizontal.residuals.size(), 6u);
}

TEST(SurveyJobsDialog, AStoredReportTooLongToLayOutIsCutAtARowAndSaysSo)
{
    std::string html = "<html><body><table><tbody>";
    for (int i = 0; i < 1000; ++i) {
        html += "<tr><td>row</td></tr>";
    }
    html += "</tbody></table></body></html>";
    const std::string cut = katana::qt::viewableReportHtml(html, 1000);
    EXPECT_LT(cut.size(), 1400u);
    // Cut after a whole row, then closed, with a line saying what to do.
    const std::size_t end = cut.find("</tbody>");
    ASSERT_NE(end, std::string::npos);
    EXPECT_EQ(cut.substr(end - 5, 5), "</tr>");
    EXPECT_NE(cut.find("export it"), std::string::npos);
    EXPECT_EQ(katana::qt::viewableReportHtml(html, html.size()), html);
}
