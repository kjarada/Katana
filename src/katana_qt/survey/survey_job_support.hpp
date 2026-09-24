#pragma once

// What the import wizard and the Survey Jobs dialog share about survey jobs:
// the reader bound to a job's stored bytes, the words for an adjustment's
// outcome, a job's last adjustment read back from its stored report, the
// report written to a file, and the reduction run for a preview.
//
// Here and not in cad because the reader is surveyio's, which cad may not
// see (tools/check_layering.cmake): the application is where the two meet.

#include <cstddef>
#include <cstdint>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "katana/cad/survey_job.hpp"
#include "katana/core/error.hpp"
#include "katana/survey/reduction.hpp"
#include "katana/surveyio/format.hpp"

class QString;

namespace katana::qt {

// Inputs no larger than these are read and reduced on the GUI thread, at
// once; larger ones on a pool thread with a Cancel button (SurveyTaskBar).
// Chosen so the GUI thread never spends more than a blink on one: a
// megabyte of field file reads in well under 0.1 s even in a Debug build.
inline constexpr std::int64_t kBackgroundReadBytes = std::int64_t{1} << 20;
inline constexpr std::size_t kBackgroundObservations = 20'000;
// Rows per table of a report on screen (displayReport); the job keeps, and
// Export writes, every row.
inline constexpr std::size_t kReportRowsOnScreen = 2'000;

// Observations and points of a project: what decides whether its reduction
// runs in the background.
[[nodiscard]] std::size_t surveySize(const katana::survey::SurveyProject& project);

// The SurveyJobReader the application gives cad: the job's stored bytes read
// again with the reader its format names, its sibling files served from
// memory - the folder they came from is not needed, nor looked at.
[[nodiscard]] katana::cad::SurveyJobReader storedJobReader();

// The version of the reader a format has in this build, for
// SurveyJobReadjustment::parserVersion; empty for an unknown format.
[[nodiscard]] std::string parserVersionOf(const std::string& formatId);

// What the reduction's report input says about a file that was read.
[[nodiscard]] katana::survey::ReportInput reportInputFor(const katana::surveyio::ReadResult& read,
                                                         const std::string& fileName);

// "network least squares (horizontal): variance factor 1.023, global test
// passed" / "radiation: nothing adjusted" - an outcome in a line.
[[nodiscard]] std::string adjustmentSummary(const katana::survey::ReductionReport& report);
// How many observations the report says were rejected.
[[nodiscard]] std::size_t rejectedObservations(const katana::survey::ReductionReport& report);

// The adjustment's method as a person reads it ("network least squares,
// horizontal").
[[nodiscard]] std::string methodText(const katana::survey::ReductionSettings& settings);

// The last adjustment of a job as its stored plain-text report states it.
// The job keeps the report as text, not as values (storage::SurveyJob), so
// the list of jobs reads these two lines of it back.
struct JobAdjustmentLine {
    std::optional<std::string> varianceFactor; // "1.023"
    std::optional<std::string> globalTest;     // "passed" or "FAILED"
};
[[nodiscard]] JobAdjustmentLine adjustmentFromReportText(const std::string& reportText);

// Writes a job's report: .html as the job keeps it, .txt its plain text, .pdf
// the HTML laid out on A4 pages (QTextDocument onto a QPdfWriter).
enum class ReportFileFormat { Html, Text, Pdf };
[[nodiscard]] katana::core::Status exportReport(const katana::cad::SurveyJob& job,
                                                const QString& path, ReportFileFormat format);
// The format a path's extension names; nullopt for another.
[[nodiscard]] std::optional<ReportFileFormat> reportFormatOf(const QString& path);

// A reduction that has already run (on a pool thread, for a preview), handed
// to a command as its ReductionFunction so the command does not run it a
// second time on the GUI thread. Used only when the command asks with the
// very settings, previous coordinates and drawing points the outcome was made
// with; anything else runs survey::reduceAndAdjust. The raw project is the
// caller's to keep the same: the one the outcome was made from.
struct PrecomputedReduction {
    std::shared_ptr<const katana::survey::ReductionOutcome> outcome;
    katana::survey::ReductionSettings settings;
    std::vector<katana::survey::ComputedPoint> previous;
    std::vector<katana::survey::SurveyPoint> drawingPoints;
};
[[nodiscard]] katana::cad::ReductionFunction precomputedReduction(PrecomputedReduction done);

// A SurveyJobReader that answers for `jobId` with `project` - read once
// already, off the GUI thread - and for any other job reads the stored bytes
// (storedJobReader). A job's bytes never change, so its project read by the
// same reader never does either.
[[nodiscard]] katana::cad::SurveyJobReader
cachedJobReader(std::string jobId, std::shared_ptr<const katana::survey::SurveyProject> project);

} // namespace katana::qt
