#include "katana/survey/report_summary.hpp"

#include <format>

namespace katana::survey {

std::string adjustmentSummary(const ReductionReport& report)
{
    if (report.adjustments.empty()) {
        return report.settings.method == AdjustmentMethod::None
                   ? "radiation: nothing adjusted"
                   : methodText(report.settings) + ": no adjustment was run";
    }
    std::string text;
    for (const AdjustmentReport& adjustment : report.adjustments) {
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

std::size_t rejectedObservations(const ReductionReport& report)
{
    std::size_t rejected = 0;
    for (const ReportObservation& observation : report.observations) {
        rejected += observation.rejected ? 1 : 0;
    }
    for (const AdjustmentReport& adjustment : report.adjustments) {
        rejected += adjustment.rejectedOutliers.size();
    }
    return rejected;
}

std::string methodText(const ReductionSettings& settings)
{
    switch (settings.method) {
    case AdjustmentMethod::None:
        return "radiation";
    case AdjustmentMethod::Traverse:
        switch (settings.traverseRule) {
        case TraverseRule::Bowditch:
            return "traverse, Bowditch";
        case TraverseRule::Transit:
            return "traverse, Transit";
        case TraverseRule::LeastSquares:
            return "traverse, least squares";
        }
        return "traverse";
    case AdjustmentMethod::Network:
        switch (settings.networkDimension) {
        case NetworkDimension::Horizontal:
            return "network, horizontal";
        case NetworkDimension::Levels:
            return "network, levels";
        case NetworkDimension::HorizontalAndLevels:
            return "network, horizontal and levels";
        }
        return "network";
    }
    return "radiation";
}

} // namespace katana::survey
