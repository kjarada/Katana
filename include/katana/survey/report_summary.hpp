#pragma once

// A reduction's outcome in a line: the words the import wizard and the Survey
// Jobs dialog show after a preview, an import and a re-adjustment, and the
// count SURVEY IMPORT's reply gives (src/katana_app/survey_verbs.cpp).
//
// Here, below both, because the window and the line report one outcome: the
// wizard's "3186 observation(s) rejected" and the reply's rejected=3186 are
// one count, taken one way. They began in the window
// (src/katana_qt/survey/survey_job_support.cpp), where the line could not
// reach them.

#include <cstddef>
#include <string>

#include "katana/survey/reduction_report.hpp"
#include "katana/survey/reduction_settings.hpp"

namespace katana::survey {

// "network least squares (horizontal): variance factor 1.023, global test
// passed" / "radiation: nothing adjusted" / "traverse, Bowditch: no adjustment
// was run" - each adjustment the report holds, "; " between.
[[nodiscard]] std::string adjustmentSummary(const ReductionReport& report);

// How many observations the report says were rejected: its rows marked
// rejected, and the outliers each adjustment rejected. An outlier an
// adjustment rejects marks its rows too, so it is counted twice: one
// distance rejected reads as 2 (docs/survey.md, "Not done" of "The
// reduction settings of SURVEY IMPORT").
[[nodiscard]] std::size_t rejectedObservations(const ReductionReport& report);

// The settings' adjustment as a person reads it: "radiation", "traverse,
// Bowditch", "network, horizontal and levels".
[[nodiscard]] std::string methodText(const ReductionSettings& settings);

} // namespace katana::survey
