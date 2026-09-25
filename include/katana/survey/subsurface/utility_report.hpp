#pragma once

// Plain-text reports of a subsurface utility investigation, for the command
// line and for a person to read beside the drawing.
//
// Numbers are metres to three decimals unless the report says otherwise. Each
// report ends with its findings - the lines a reviewer has to act on - so that
// a report with none says so in words rather than by omission.

#include <optional>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/survey/subsurface/clearance.hpp"
#include "katana/survey/subsurface/utility_network.hpp"
#include "katana/survey/subsurface/verification.hpp"

namespace katana::survey::subsurface {

// Every line graded (vertices, segments, lengths at each quality level), depth
// of cover where it can be computed, a summary of length by utility type and
// quality level, and the findings: over-claimed quality levels, cover below
// `minimumCover`, levels that are not qualified, and the attributes AS 5488
// asks for that were not recorded (type, owner, status, material, size).
[[nodiscard]] core::Result<std::string>
renderInvestigationReport(const std::vector<UtilityLine>& lines,
                          const GradingSettings& settings = {},
                          std::optional<double> minimumCover = std::nullopt);

// The counts by status, then every segment that is not clear, worst first,
// and where to pothole next.
[[nodiscard]] std::string renderClearanceReport(const DesignAlignment& design,
                                                const std::vector<ClearanceResult>& results,
                                                const ClearanceRequirement& requirement);

// Each comparison, the pass rates and the RMS, and a conclusion.
[[nodiscard]] std::string renderVerificationReport(const VerificationReport& report,
                                                   const GradingSettings& settings = {});

} // namespace katana::survey::subsurface
