// Placeholder bodies for the reduction contract (reduction.hpp and
// reduction_report.hpp), so that everything built against the contract links
// before the reduction exists. The reduction builder REPLACES this file: each
// function here fails loudly, never with a plausible-looking empty answer.

#include "katana/survey/reduction.hpp"

namespace katana::survey {

katana::core::Result<ReductionOutcome> reduceAndAdjust(const SurveyProject& /*raw*/,
                                                       const ReductionSettings& /*settings*/,
                                                       const ReductionContext& /*context*/)
{
    return katana::core::makeError(
        katana::core::ErrorCode::Unsupported,
        "reducing and adjusting raw observations is not available in this build yet");
}

std::string renderText(const ReductionReport& /*report*/)
{
    return "The reduction report cannot be shown: report rendering is not available in this "
           "build yet.\n";
}

std::string renderHtml(const ReductionReport& /*report*/)
{
    return "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Reduction report</title>"
           "</head><body><p>The reduction report cannot be shown: report rendering is not "
           "available in this build yet.</p></body></html>\n";
}

} // namespace katana::survey
