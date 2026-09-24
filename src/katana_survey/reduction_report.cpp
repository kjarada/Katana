#include "katana/survey/reduction_report.hpp"

namespace katana::survey {

const char* toString(CorrectionKind kind)
{
    switch (kind) {
    case CorrectionKind::PrismConstant:
        return "prism constant";
    case CorrectionKind::Atmospheric:
        return "atmospheric";
    case CorrectionKind::FaceMean:
        return "face left / face right mean";
    case CorrectionKind::SlopeToHorizontal:
        return "slope to horizontal";
    case CorrectionKind::CurvatureRefraction:
        return "curvature and refraction";
    case CorrectionKind::HeightReduction:
        return "height reduction";
    case CorrectionKind::GridScale:
        return "grid scale factor";
    case CorrectionKind::CombinedFactor:
        return "combined scale factor";
    case CorrectionKind::Orientation:
        return "orientation";
    case CorrectionKind::InstrumentAndTargetHeight:
        return "instrument and target heights";
    case CorrectionKind::Other:
        return "other";
    }
    return "other";
}

const char* toString(ComputationMethod method)
{
    switch (method) {
    case ComputationMethod::Control:
        return "control";
    case ComputationMethod::Radiation:
        return "radiation";
    case ComputationMethod::TraverseBowditch:
        return "traverse (Bowditch)";
    case ComputationMethod::TraverseTransit:
        return "traverse (Transit)";
    case ComputationMethod::TraverseLeastSquares:
        return "traverse (least squares)";
    case ComputationMethod::NetworkLeastSquares:
        return "network least squares";
    case ComputationMethod::Gnss:
        return "GNSS";
    }
    return "radiation";
}

} // namespace katana::survey
