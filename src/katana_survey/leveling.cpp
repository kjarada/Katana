#include "katana/survey/leveling.hpp"

#include <cmath>
#include <utility>

#include "katana/core/text.hpp"

namespace katana::survey {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

constexpr double kMetresPerKilometre = 1000.0;

katana::core::Error malformed(const LevelRun& run, std::string message)
{
    return makeError(ErrorCode::InvalidArgument, std::move(message),
                     "level run '" + run.name + "'");
}

Result<double> scaledBySqrtKilometres(double factor, double lengthMetres, const char* factorName)
{
    if (!std::isfinite(factor) || factor < 0.0) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string(factorName) + " must be finite and not negative");
    }
    if (!std::isfinite(lengthMetres) || lengthMetres < 0.0) {
        return makeError(ErrorCode::InvalidArgument, "length must be finite and not negative",
                         "length " + katana::core::formatExactReal(lengthMetres));
    }
    return factor * std::sqrt(lengthMetres / kMetresPerKilometre);
}

} // namespace

Result<LevelRunResult> computeLevelRun(const LevelRun& run, LevelAdjustment adjustment)
{
    if (run.setups.empty()) {
        return malformed(run, "a level run needs at least one setup");
    }
    if (!std::isfinite(run.startElevation)) {
        return malformed(run, "start elevation is not finite");
    }
    if (run.closingElevation && !std::isfinite(*run.closingElevation)) {
        return malformed(run, "closing elevation is not finite");
    }
    for (std::size_t i = 0; i < run.setups.size(); ++i) {
        const LevelSetup& setup = run.setups[i];
        if (!std::isfinite(setup.backsight) || !std::isfinite(setup.foresight)) {
            return malformed(run, "staff reading of setup " + std::to_string(i) + " is not finite");
        }
        if (!std::isfinite(setup.distance) || setup.distance < 0.0) {
            return malformed(run, "distance of setup " + std::to_string(i) +
                                      " must be finite and not negative");
        }
    }
    if (adjustment != LevelAdjustment::None && !run.closingElevation) {
        return malformed(run, "an adjustment needs a closing elevation");
    }

    LevelRunResult result;
    result.points.reserve(run.setups.size());
    double elevation = run.startElevation;
    for (const LevelSetup& setup : run.setups) {
        LevelPointResult point;
        point.pointId = setup.foresightPointId;
        point.heightOfInstrument = elevation + setup.backsight;
        point.elevation = point.heightOfInstrument - setup.foresight;
        point.adjustedElevation = point.elevation;
        elevation = point.elevation;
        result.sumBacksights += setup.backsight;
        result.sumForesights += setup.foresight;
        result.totalDistance += setup.distance;
        result.points.push_back(std::move(point));
    }
    if (run.closingElevation) {
        result.misclosure = elevation - *run.closingElevation;
    }
    if (adjustment == LevelAdjustment::None) {
        return result;
    }
    if (adjustment == LevelAdjustment::ByDistance && !(result.totalDistance > 0.0)) {
        return malformed(run, "adjustment by distance needs setup distances");
    }

    // The correction grows along the run and reaches -misclosure at the closing
    // point, so the adjusted closing elevation equals the known one.
    const double misclosure = *result.misclosure;
    const double setupCount = static_cast<double>(run.setups.size());
    double distance = 0.0;
    for (std::size_t i = 0; i < result.points.size(); ++i) {
        distance += run.setups[i].distance;
        const double fraction = adjustment == LevelAdjustment::BySetups
                                    ? static_cast<double>(i + 1) / setupCount
                                    : distance / result.totalDistance;
        LevelPointResult& point = result.points[i];
        point.correction = -misclosure * fraction;
        point.adjustedElevation = point.elevation + point.correction;
    }
    return result;
}

Result<double> allowableLevelMisclosure(double k, double lengthMetres)
{
    return scaledBySqrtKilometres(k, lengthMetres, "k");
}

Result<double> levelDifferenceSigma(double sigmaPerSqrtKm, double lengthMetres)
{
    return scaledBySqrtKilometres(sigmaPerSqrtKm, lengthMetres, "sigma per sqrt(km)");
}

} // namespace katana::survey
