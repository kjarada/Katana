#pragma once

// Checking QL-B detections against QL-A exposures.
//
// AS 5488 leaves a detection's QL-B grade resting on the locator's stated
// uncertainty. The only independent test of that statement is to dig: expose
// the service at a detected position, survey it, and compare. An investigation
// that has done so can say how its detections actually performed; one that
// has not can only repeat what the equipment claims.
//
// An exposure names the detection it checks through UtilityVertex::verifies.
// The comparison is between the two surveyed positions of the SAME service,
// so both levels are brought to the top of the service first
// (utility_network.hpp, topLevel) - a detection recorded to the centre and an
// exposure recorded to the crown differ by half a diameter that is not error.

#include <cstddef>
#include <optional>
#include <string>
#include <vector>

#include "katana/survey/subsurface/utility_network.hpp"

namespace katana::survey::subsurface {

struct VerificationResult {
    std::string lineId;
    std::string detectedId;
    std::string exposedId;
    // Plan distance between the two, metres.
    double horizontalDeviation = 0.0;
    // Exposed top minus detected top; nullopt when either has no level.
    std::optional<double> verticalDeviation;
    // Within the QL-B tolerances.
    bool horizontalWithin = false;
    std::optional<bool> verticalWithin;
};

struct VerificationReport {
    std::vector<VerificationResult> results;
    // Exposures that could not be used, one sentence each: the detection they
    // name does not exist, or the exposure is not itself QL-A and so is not a
    // check of anything.
    std::vector<std::string> problems;

    std::size_t horizontalPassed = 0;
    std::size_t verticalChecked = 0;
    std::size_t verticalPassed = 0;
    double maximumHorizontal = 0.0;
    double rmsHorizontal = 0.0;
    std::optional<double> maximumVertical; // largest |vertical deviation|
    std::optional<double> rmsVertical;
};

// Every exposure in `lines` that names a detection is compared with it. The
// detection is looked for on the exposure's own line first, then on any line,
// so a check dug where two services were confused is still made - and then
// reported against the line the detection belongs to.
[[nodiscard]] VerificationReport verifyDetections(const std::vector<UtilityLine>& lines,
                                                  const GradingSettings& settings = {});

} // namespace katana::survey::subsurface
