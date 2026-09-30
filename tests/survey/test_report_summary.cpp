// A reduction's outcome in a line (survey/report_summary.hpp): the words the
// wizard and the Survey Jobs dialog show, and the rejected count SURVEY
// IMPORT's reply shares with them. Every report here is built by hand.

#include <gtest/gtest.h>

#include <optional>
#include <string>
#include <utility>

#include "katana/survey/report_summary.hpp"

using namespace katana::survey;

namespace {

AdjustmentReport adjustment(std::string method, std::optional<double> varianceFactor,
                            std::optional<bool> passed)
{
    AdjustmentReport report;
    report.method = std::move(method);
    report.varianceFactor = varianceFactor;
    if (passed) {
        report.globalTest = ReportGlobalTest{};
        report.globalTest->passed = *passed;
    }
    return report;
}

} // namespace

TEST(ReportSummary, EachAdjustmentIsALineOfItsOutcomeAndNoneIsSaidAsSuch)
{
    ReductionReport report;
    EXPECT_EQ(adjustmentSummary(report), "radiation: nothing adjusted");
    report.settings.method = AdjustmentMethod::Traverse;
    EXPECT_EQ(adjustmentSummary(report), "traverse, Bowditch: no adjustment was run");

    AdjustmentReport horizontal = adjustment("network least squares (horizontal)", 0.3214, false);
    horizontal.flaggedOutliers = {"a", "b", "c"};
    report.adjustments = {horizontal,
                          adjustment("network least squares (levels)", 1.0, true),
                          adjustment("network least squares (levels)", std::nullopt, std::nullopt)};
    EXPECT_EQ(adjustmentSummary(report),
              "network least squares (horizontal): variance factor 0.321, global test FAILED, "
              "3 flagged outlier(s); network least squares (levels): variance factor 1.000, "
              "global test passed; network least squares (levels): no redundancy, so no "
              "variance factor");
}

// The rows the report marks rejected - three here - and the outliers each
// adjustment rejected - one and two: six.
TEST(ReportSummary, TheRejectedCountIsTheRowsMarkedRejectedAndTheOutliersRejected)
{
    ReductionReport report;
    EXPECT_EQ(rejectedObservations(report), 0U);
    ReportObservation kept;
    ReportObservation rejected;
    rejected.rejected = true;
    report.observations = {kept, rejected, kept, rejected, rejected};
    AdjustmentReport first;
    first.rejectedOutliers = {"x"};
    AdjustmentReport second;
    second.rejectedOutliers = {"y", "z"};
    report.adjustments = {first, second};
    EXPECT_EQ(rejectedObservations(report), 6U);
}

TEST(ReportSummary, TheMethodIsTheSettingsInWords)
{
    ReductionSettings settings;
    EXPECT_EQ(methodText(settings), "radiation");
    settings.method = AdjustmentMethod::Traverse;
    const std::pair<TraverseRule, const char*> rules[] = {
        {TraverseRule::Bowditch, "traverse, Bowditch"},
        {TraverseRule::Transit, "traverse, Transit"},
        {TraverseRule::LeastSquares, "traverse, least squares"}};
    for (const auto& [rule, words] : rules) {
        settings.traverseRule = rule;
        EXPECT_EQ(methodText(settings), words);
    }
    settings.method = AdjustmentMethod::Network;
    const std::pair<NetworkDimension, const char*> dimensions[] = {
        {NetworkDimension::Horizontal, "network, horizontal"},
        {NetworkDimension::Levels, "network, levels"},
        {NetworkDimension::HorizontalAndLevels, "network, horizontal and levels"}};
    for (const auto& [dimension, words] : dimensions) {
        settings.networkDimension = dimension;
        EXPECT_EQ(methodText(settings), words);
    }
}
