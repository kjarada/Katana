#include "katana/survey/traverse.hpp"

#include <cmath>
#include <set>
#include <utility>

#include "katana/math/numerics.hpp"

namespace katana::survey {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::math::kPi;
using katana::math::normalizeAngle;
using katana::math::normalizeAngleSigned;

namespace {

Status malformed(const Traverse& traverse, std::string message)
{
    return makeError(ErrorCode::InvalidArgument, std::move(message),
                     "traverse '" + traverse.name + "'");
}

bool isFinite(const Coordinate2& c)
{
    return std::isfinite(c.northing) && std::isfinite(c.easting);
}

// Angular misclosure from the raw angles: azimuth carried through every
// station minus the direction it has to arrive at.
std::optional<double> angularMisclosure(const Traverse& traverse)
{
    const auto& setups = traverse.setups;
    if (traverse.kind == TraverseKind::ClosedLoop) {
        // Carrying the first-leg azimuth once around the loop adds every angle
        // plus pi per station and must reproduce it: sum + n*pi = 0 (mod 2*pi).
        // Holds for interior (counter-clockwise loop) and exterior angles alike.
        double carried = 0.0;
        for (const TraverseSetup& setup : setups) {
            carried = normalizeAngle(carried + setup.angle + kPi);
        }
        return normalizeAngleSigned(carried);
    }
    if (traverse.kind == TraverseKind::Link && traverse.closingAngle) {
        double azimuth = traverse.startAzimuth + setups.front().angle;
        for (std::size_t k = 1; k < setups.size(); ++k) {
            azimuth = normalizeAngle(azimuth + kPi + setups[k].angle);
        }
        const double closing = azimuth + kPi + traverse.closingAngle->angle;
        return normalizeAngleSigned(closing - traverse.closingAngle->referenceAzimuth);
    }
    return std::nullopt;
}

std::vector<double> legAzimuths(const Traverse& traverse, double angleCorrection)
{
    const auto& setups = traverse.setups;
    std::vector<double> azimuths(setups.size());
    azimuths[0] = traverse.kind == TraverseKind::ClosedLoop
                      ? normalizeAngle(traverse.startAzimuth)
                      : normalizeAngle(traverse.startAzimuth + setups[0].angle + angleCorrection);
    for (std::size_t k = 1; k < setups.size(); ++k) {
        azimuths[k] = normalizeAngle(azimuths[k - 1] + kPi + setups[k].angle + angleCorrection);
    }
    return azimuths;
}

} // namespace

Status validateTraverse(const Traverse& traverse)
{
    const auto& setups = traverse.setups;
    const bool isLoop = traverse.kind == TraverseKind::ClosedLoop;
    if (setups.empty()) {
        return malformed(traverse, "a traverse needs at least one setup");
    }
    if (isLoop && setups.size() < 3) {
        return malformed(traverse, "a closed loop needs at least 3 setups");
    }
    if (!isFinite(traverse.start) || !std::isfinite(traverse.startAzimuth)) {
        return malformed(traverse, "start coordinates or start azimuth are not finite");
    }

    std::set<std::string> seen;
    for (const TraverseSetup& setup : setups) {
        if (setup.stationId.empty()) {
            return malformed(traverse, "a setup has an empty station id");
        }
        if (!seen.insert(setup.stationId).second) {
            return malformed(traverse, "station id '" + setup.stationId + "' is repeated");
        }
        if (!std::isfinite(setup.angle)) {
            return malformed(traverse, "angle at '" + setup.stationId + "' is not finite");
        }
        if (!std::isfinite(setup.distance) || !(setup.distance > 0.0)) {
            return malformed(traverse,
                             "distance from '" + setup.stationId + "' must be positive and finite");
        }
    }
    if (isLoop) {
        return {};
    }

    if (traverse.endStationId.empty()) {
        return malformed(traverse, "open and link traverses need an end station id");
    }
    // A link traverse may close on its own start (a loop oriented on an external
    // reference mark); any other repetition is an error.
    const bool closesOnStart = traverse.kind == TraverseKind::Link && setups.size() >= 2 &&
                               traverse.endStationId == setups.front().stationId;
    if (seen.count(traverse.endStationId) != 0 && !closesOnStart) {
        return malformed(traverse, "end station id '" + traverse.endStationId + "' is repeated");
    }
    if (traverse.kind == TraverseKind::Link) {
        if (!isFinite(traverse.end)) {
            return malformed(traverse, "end coordinates are not finite");
        }
        if (closesOnStart && !(traverse.end == traverse.start)) {
            return malformed(traverse,
                             "the end station is the start station but their coordinates differ");
        }
        if (traverse.closingAngle && (!std::isfinite(traverse.closingAngle->angle) ||
                                      !std::isfinite(traverse.closingAngle->referenceAzimuth))) {
            return malformed(traverse, "closing angle or its reference azimuth is not finite");
        }
    }
    return {};
}

Result<TraverseResult> computeTraverse(const Traverse& traverse, const TraverseOptions& options)
{
    if (Status status = validateTraverse(traverse); !status) {
        return status.error();
    }
    const auto& setups = traverse.setups;
    const std::size_t legCount = setups.size();
    const bool isLoop = traverse.kind == TraverseKind::ClosedLoop;

    TraverseResult result;
    result.kind = traverse.kind;

    // ---- angles ----
    result.angularMisclosure = angularMisclosure(traverse);
    if (result.angularMisclosure) {
        result.angleCount = isLoop ? legCount : legCount + 1;
        if (options.balanceAngles) {
            result.angleCorrection =
                -*result.angularMisclosure / static_cast<double>(result.angleCount);
        }
    }
    const std::vector<double> azimuths = legAzimuths(traverse, result.angleCorrection);

    // ---- latitudes and departures ----
    result.legs.resize(legCount);
    double sumLatitude = 0.0;
    double sumDeparture = 0.0;
    double sumAbsLatitude = 0.0;
    double sumAbsDeparture = 0.0;
    for (std::size_t k = 0; k < legCount; ++k) {
        TraverseLegResult& leg = result.legs[k];
        leg.fromId = setups[k].stationId;
        if (k + 1 < legCount) {
            leg.toId = setups[k + 1].stationId;
        } else {
            leg.toId = isLoop ? setups.front().stationId : traverse.endStationId;
        }
        leg.azimuth = azimuths[k];
        leg.distance = setups[k].distance;
        leg.latitude = leg.distance * std::cos(leg.azimuth);
        leg.departure = leg.distance * std::sin(leg.azimuth);
        sumLatitude += leg.latitude;
        sumDeparture += leg.departure;
        sumAbsLatitude += std::abs(leg.latitude);
        sumAbsDeparture += std::abs(leg.departure);
        result.totalLength += leg.distance;
    }

    // ---- linear misclosure and its distribution ----
    if (traverse.kind != TraverseKind::Open) {
        LinearMisclosure misclosure;
        // The control difference is formed first: it is a small, (nearly) exact
        // number, whereas start + sum would round at the size of the coordinates.
        const double knownLatitude = isLoop ? 0.0 : traverse.end.northing - traverse.start.northing;
        const double knownDeparture = isLoop ? 0.0 : traverse.end.easting - traverse.start.easting;
        misclosure.latitude = sumLatitude - knownLatitude;
        misclosure.departure = sumDeparture - knownDeparture;
        misclosure.length = std::hypot(misclosure.latitude, misclosure.departure);
        misclosure.relativePrecision = misclosure.length / result.totalLength;
        if (misclosure.length > 0.0) {
            misclosure.precisionDenominator = result.totalLength / misclosure.length;
        }
        result.linearMisclosure = misclosure;

        for (TraverseLegResult& leg : result.legs) {
            if (options.adjustment == TraverseAdjustment::Compass) {
                const double share = leg.distance / result.totalLength;
                leg.latitudeCorrection = -misclosure.latitude * share;
                leg.departureCorrection = -misclosure.departure * share;
            } else if (options.adjustment == TraverseAdjustment::Transit) {
                // |sum| <= sum of magnitudes, so a zero denominator implies a zero
                // misclosure in that component and nothing to distribute.
                if (sumAbsLatitude > 0.0) {
                    leg.latitudeCorrection =
                        -misclosure.latitude * std::abs(leg.latitude) / sumAbsLatitude;
                }
                if (sumAbsDeparture > 0.0) {
                    leg.departureCorrection =
                        -misclosure.departure * std::abs(leg.departure) / sumAbsDeparture;
                }
            }
        }
    }

    // ---- coordinates ----
    // Offsets from the start are accumulated first and added to the (possibly
    // UTM sized) start coordinates once per station, so rounding at coordinate
    // magnitude does not accumulate along the traverse.
    const std::size_t stationCount = isLoop ? legCount : legCount + 1;
    result.stations.resize(stationCount);
    result.stations[0] = TraverseStationResult{setups.front().stationId, traverse.start,
                                               traverse.start};
    double latitude = 0.0;
    double departure = 0.0;
    double adjustedLatitude = 0.0;
    double adjustedDeparture = 0.0;
    for (std::size_t k = 0; k + 1 < stationCount; ++k) {
        const TraverseLegResult& leg = result.legs[k];
        latitude += leg.latitude;
        departure += leg.departure;
        adjustedLatitude += leg.latitude + leg.latitudeCorrection;
        adjustedDeparture += leg.departure + leg.departureCorrection;
        TraverseStationResult& station = result.stations[k + 1];
        station.id = leg.toId;
        station.unadjusted = Coordinate2{traverse.start.northing + latitude,
                                         traverse.start.easting + departure};
        station.adjusted = Coordinate2{traverse.start.northing + adjustedLatitude,
                                       traverse.start.easting + adjustedDeparture};
    }
    // The adjusted end of a link traverse is its control point by construction;
    // state it exactly instead of to within the rounding of the distribution.
    if (traverse.kind == TraverseKind::Link && options.adjustment != TraverseAdjustment::None) {
        result.stations.back().adjusted = traverse.end;
    }
    return result;
}

} // namespace katana::survey
