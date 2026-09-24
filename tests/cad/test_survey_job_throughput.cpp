// The cost of a large survey job's commands, measured rather than asserted (a
// shared machine's timing is not a property); what IS asserted is that a job
// of this size still imports, re-adjusts and undoes as one step each.

#include <gtest/gtest.h>

#include <chrono>
#include <cstdio>
#include <memory>
#include <string>

#include "katana/cad/document.hpp"
#include "katana/cad/survey_job.hpp"

using namespace katana::cad;
using katana::core::Result;
namespace survey = katana::survey;

namespace {

constexpr std::size_t kPoints = 50'000;

// A reduction that places point i at a grid position depending on i and on a
// shift, so a re-adjustment with another shift moves every point.
ReductionFunction shiftedGrid(double shift)
{
    return [shift](const survey::SurveyProject&, const survey::ReductionSettings& settings,
                   const survey::ReductionContext&) -> Result<survey::ReductionOutcome> {
        survey::ReductionOutcome outcome;
        outcome.reduced.points.reserve(kPoints);
        for (std::size_t i = 0; i < kPoints; ++i) {
            survey::SurveyPoint point;
            point.id = std::to_string(1000 + i);
            point.northing = 6'250'000.0 + static_cast<double>(i / 250) * 2.0 + shift;
            point.easting = 300'000.0 + static_cast<double>(i % 250) * 2.0 + shift;
            point.elevation = 10.0 + shift;
            point.code = "NS";
            point.coordinateSource = survey::CoordinateSource::Calculated;
            outcome.reduced.points.push_back(std::move(point));
        }
        outcome.report.settings = settings;
        return outcome;
    };
}

double millisecondsSince(std::chrono::steady_clock::time_point start)
{
    return std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - start)
        .count();
}

} // namespace

TEST(SurveyJobThroughput, FiftyThousandPointsImportReadjustAndUndoAsOneStepEach)
{
    Document document;
    SurveyJobImport request;
    request.job.name = "big.gsi";
    request.job.sourceBytes = std::string(8'000'000, 'x'); // about what 50k shots take
    request.job.layer = "survey/big";
    request.importOptions.layer = "survey/big";
    request.context.createdUtc = "2026-09-24T00:00:00Z";

    auto start = std::chrono::steady_clock::now();
    auto import = std::make_unique<ImportSurveyJobCommand>(document, request, shiftedGrid(0.0));
    auto* imported = import.get();
    ASSERT_TRUE(document.execute(std::move(import)).ok());
    const double importMs = millisecondsSince(start);
    ASSERT_EQ(document.surveyJobs().front().placedPoints.size(), kPoints);

    SurveyJobReadjustment readjust;
    readjust.jobId = imported->jobId();
    readjust.context.createdUtc = "2026-09-25T00:00:00Z";
    const SurveyJobReader reader = [](const SurveyJob&) -> Result<survey::SurveyProject> {
        return survey::SurveyProject{};
    };
    start = std::chrono::steady_clock::now();
    auto command =
        std::make_unique<ReadjustSurveyJobCommand>(document, readjust, reader, shiftedGrid(0.01));
    auto* readjusted = command.get();
    ASSERT_TRUE(document.execute(std::move(command)).ok());
    const double readjustMs = millisecondsSince(start);
    EXPECT_EQ(readjusted->changes().moved.size(), kPoints);

    start = std::chrono::steady_clock::now();
    ASSERT_TRUE(document.undo().ok());
    const double undoMs = millisecondsSince(start);
    EXPECT_EQ(document.surveyJobs().front().placedPoints.front().northing, 6'250'000.0);

    start = std::chrono::steady_clock::now();
    ASSERT_TRUE(document.undo().ok());
    const double undoImportMs = millisecondsSince(start);
    EXPECT_TRUE(document.surveyJobs().empty());
    EXPECT_EQ(document.model().entities.size(), 0U);

    std::printf("survey job of %zu points: import %.0f ms (%.0f points/s), re-adjust moving "
                "all %.0f ms (%.0f points/s), undo re-adjust %.0f ms, undo import %.0f ms\n",
                kPoints, importMs, kPoints / (importMs / 1000.0), readjustMs,
                kPoints / (readjustMs / 1000.0), undoMs, undoImportMs);
}
