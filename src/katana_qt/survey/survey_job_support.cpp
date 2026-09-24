#include "survey/survey_job_support.hpp"

#include <QFile>
#include <QFileInfo>
#include <QMarginsF>
#include <QPageLayout>
#include <QPageSize>
#include <QPdfWriter>
#include <QString>
#include <QTextDocument>

#include <format>
#include <string_view>
#include <utility>
#include <variant>

#include "katana/core/text.hpp"
#include "katana/surveyio/reader.hpp"

namespace katana::qt {

namespace cad = katana::cad;
namespace survey = katana::survey;
namespace surveyio = katana::surveyio;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

std::size_t surveySize(const survey::SurveyProject& project)
{
    std::size_t size = project.points.size() + project.observations.size();
    for (const survey::SurveyStation& station : project.stations) {
        size += station.observations.size();
    }
    return size;
}

cad::SurveyJobReader storedJobReader()
{
    return [](const cad::SurveyJob& job) -> Result<survey::SurveyProject> {
        std::vector<surveyio::SiblingFile> files;
        files.reserve(job.siblingFiles.size());
        for (const cad::SurveyJobFile& file : job.siblingFiles) {
            files.push_back({file.name, file.bytes});
        }
        surveyio::ReadOptions options;
        options.siblings = surveyio::siblingsInMemory(std::move(files));
        auto read = surveyio::readSurvey(surveyio::formatRegistry(), job.formatId, job.sourceBytes,
                                         job.sourceFileName, options);
        if (!read) {
            return read.error();
        }
        return std::move(read->project);
    };
}

cad::SurveyJobReader cachedJobReader(std::string jobId,
                                     std::shared_ptr<const survey::SurveyProject> project)
{
    return [jobId = std::move(jobId), project = std::move(project),
            read = storedJobReader()](const cad::SurveyJob& job) -> Result<survey::SurveyProject> {
        if (project != nullptr && job.id == jobId) {
            return *project;
        }
        return read(job);
    };
}

std::string parserVersionOf(const std::string& formatId)
{
    const auto descriptor = surveyio::formatRegistry().find(formatId);
    return descriptor ? descriptor->parserVersion : std::string{};
}

survey::ReportInput reportInputFor(const surveyio::ReadResult& read, const std::string& fileName)
{
    survey::ReportInput input;
    input.fileName = fileName;
    for (const surveyio::SiblingFile& sibling : read.siblingsRead) {
        input.siblingFiles.push_back(sibling.name);
    }
    input.formatId = read.formatId;
    const auto descriptor = surveyio::formatRegistry().find(read.formatId);
    input.formatName = descriptor ? descriptor->humanName : read.formatId;
    input.parserVersion = read.parserVersion;
    input.recordsRead = read.recordsRead;
    input.recordsSkipped = read.recordsSkipped;
    input.warnings.reserve(read.warnings.size());
    for (const surveyio::ReadWarning& warning : read.warnings) {
        survey::SourceRecord source;
        source.format = input.formatName;
        source.fileName = warning.fileName.empty() ? fileName : warning.fileName;
        source.recordNumber = warning.record;
        input.warnings.push_back({warning.message, std::move(source)});
    }
    input.notCarried = read.notCarried;
    return input;
}

std::string adjustmentSummary(const survey::ReductionReport& report)
{
    if (report.adjustments.empty()) {
        return report.settings.method == survey::AdjustmentMethod::None
                   ? "radiation: nothing adjusted"
                   : methodText(report.settings) + ": no adjustment was run";
    }
    std::string text;
    for (const survey::AdjustmentReport& adjustment : report.adjustments) {
        std::string line = adjustment.method + ": ";
        line += adjustment.varianceFactor
                    ? std::format("variance factor {:.3f}", *adjustment.varianceFactor)
                    : std::string("no redundancy, so no variance factor");
        if (adjustment.globalTest) {
            line += adjustment.globalTest->passed ? ", global test passed"
                                                  : ", global test FAILED";
        }
        if (!adjustment.flaggedOutliers.empty()) {
            line += std::format(", {} flagged outlier(s)", adjustment.flaggedOutliers.size());
        }
        text += text.empty() ? line : "; " + line;
    }
    return text;
}

std::size_t rejectedObservations(const survey::ReductionReport& report)
{
    std::size_t rejected = 0;
    for (const survey::ReportObservation& observation : report.observations) {
        rejected += observation.rejected ? 1 : 0;
    }
    for (const survey::AdjustmentReport& adjustment : report.adjustments) {
        rejected += adjustment.rejectedOutliers.size();
    }
    return rejected;
}

std::string methodText(const survey::ReductionSettings& settings)
{
    switch (settings.method) {
    case survey::AdjustmentMethod::None:
        return "radiation";
    case survey::AdjustmentMethod::Traverse:
        switch (settings.traverseRule) {
        case survey::TraverseRule::Bowditch:
            return "traverse, Bowditch";
        case survey::TraverseRule::Transit:
            return "traverse, Transit";
        case survey::TraverseRule::LeastSquares:
            return "traverse, least squares";
        }
        return "traverse";
    case survey::AdjustmentMethod::Network:
        switch (settings.networkDimension) {
        case survey::NetworkDimension::Horizontal:
            return "network, horizontal";
        case survey::NetworkDimension::Levels:
            return "network, levels";
        case survey::NetworkDimension::HorizontalAndLevels:
            return "network, horizontal and levels";
        }
        return "network";
    }
    return "radiation";
}

JobAdjustmentLine adjustmentFromReportText(const std::string& reportText)
{
    // The text renderer's key-value rows: two spaces, the key padded, the
    // value (survey::renderText). A report with two adjustments (horizontal
    // and levels) has each row twice; both are given, " / " between.
    JobAdjustmentLine line;
    const auto add = [](std::optional<std::string>& target, std::string_view value) {
        target = target ? *target + " / " + std::string(value) : std::string(value);
    };
    for (const std::string_view row : katana::core::splitLines(reportText)) {
        const std::string_view trimmed = katana::core::trimmed(row);
        constexpr std::string_view kVariance = "Variance factor";
        constexpr std::string_view kGlobal = "Chi-square global test";
        if (trimmed.starts_with(kVariance)) {
            std::string_view value = katana::core::trimmed(trimmed.substr(kVariance.size()));
            value = value.substr(0, value.find(" ("));
            add(line.varianceFactor, value);
        } else if (trimmed.starts_with(kGlobal)) {
            const auto colon = trimmed.rfind(": ");
            if (colon != std::string_view::npos) {
                add(line.globalTest, trimmed.substr(colon + 2));
            }
        }
    }
    return line;
}

std::optional<ReportFileFormat> reportFormatOf(const QString& path)
{
    const QString suffix = QFileInfo(path).suffix().toLower();
    if (suffix == "html" || suffix == "htm") {
        return ReportFileFormat::Html;
    }
    if (suffix == "txt" || suffix == "text") {
        return ReportFileFormat::Text;
    }
    if (suffix == "pdf") {
        return ReportFileFormat::Pdf;
    }
    return std::nullopt;
}

Status exportReport(const cad::SurveyJob& job, const QString& path, ReportFileFormat format)
{
    if (path.trimmed().isEmpty()) {
        return makeError(ErrorCode::InvalidArgument, "choose a file to export the report to");
    }
    if (job.reportHtml.empty() && job.reportText.empty()) {
        return makeError(ErrorCode::InvalidState, "the survey job has no reduction report",
                         job.id);
    }
    if (format == ReportFileFormat::Pdf) {
        QTextDocument document;
        document.setHtml(QString::fromStdString(job.reportHtml));
        QPdfWriter writer(path);
        writer.setPageSize(QPageSize(QPageSize::A4));
        writer.setPageMargins(QMarginsF(15, 15, 15, 15), QPageLayout::Millimeter);
        writer.setTitle(QString::fromStdString("Reduction report - " + job.sourceFileName));
        writer.setCreator("Katana");
        document.print(&writer);
    } else {
        const std::string& bytes =
            format == ReportFileFormat::Html ? job.reportHtml : job.reportText;
        QFile file(path);
        if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
            return makeError(ErrorCode::FileExportFailure, "the report cannot be written",
                             path.toStdString() + ": " + file.errorString().toStdString());
        }
        if (file.write(bytes.data(), static_cast<qint64>(bytes.size())) !=
            static_cast<qint64>(bytes.size())) {
            return makeError(ErrorCode::FileExportFailure, "the report was not written whole",
                             path.toStdString() + ": " + file.errorString().toStdString());
        }
    }
    // QPdfWriter reports nothing: whether a file is there is the check.
    if (QFileInfo(path).size() <= 0) {
        return makeError(ErrorCode::FileExportFailure, "the report file is empty",
                         path.toStdString());
    }
    return {};
}

cad::ReductionFunction precomputedReduction(PrecomputedReduction done)
{
    return [done = std::move(done)](const survey::SurveyProject& raw,
                                    const survey::ReductionSettings& settings,
                                    const survey::ReductionContext& context)
               -> Result<survey::ReductionOutcome> {
        if (done.outcome != nullptr && settings == done.settings &&
            context.previous == done.previous && context.drawingPoints == done.drawingPoints) {
            survey::ReductionOutcome outcome = *done.outcome;
            // The command's own time and input, as a fresh run would have.
            outcome.report.createdUtc = context.createdUtc;
            outcome.report.input = context.input;
            return outcome;
        }
        return survey::reduceAndAdjust(raw, settings, context);
    };
}

} // namespace katana::qt
