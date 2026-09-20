#include "katana/survey/data_model.hpp"

#include <cmath>
#include <initializer_list>
#include <utility>

#include "katana/math/numerics.hpp"

namespace katana::survey {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;
using katana::math::kHalfPi;
using katana::math::kPi;
using katana::math::normalizeAngle;

namespace {

template <typename... Visitors> struct Overloaded : Visitors... {
    using Visitors::operator()...;
};
template <typename... Visitors> Overloaded(Visitors...) -> Overloaded<Visitors...>;

std::string describe(const Observation& observation)
{
    std::string text = observationKindName(observation);
    const char* separator = " ";
    for (const std::string& id : referencedPoints(observation)) {
        text += separator;
        text += id.empty() ? "<empty>" : id;
        separator = " -> ";
    }
    return text;
}

Status invalid(const Observation& observation, std::string message)
{
    return makeError(ErrorCode::InvalidSurveyObservation, std::move(message),
                     describe(observation));
}

// Empty string when every check passes, otherwise the reason.
std::string checkFinite(std::initializer_list<std::pair<const char*, double>> values)
{
    for (const auto& [name, value] : values) {
        if (!std::isfinite(value)) {
            return std::string(name) + " is not finite";
        }
    }
    return {};
}

std::string checkSigmas(std::initializer_list<std::pair<const char*, double>> sigmas)
{
    for (const auto& [name, sigma] : sigmas) {
        if (!std::isfinite(sigma) || !(sigma > 0.0)) {
            return std::string(name) + " must be a positive finite standard deviation";
        }
    }
    return {};
}

std::string checkStations(const std::vector<std::string>& ids)
{
    for (std::size_t i = 0; i < ids.size(); ++i) {
        if (ids[i].empty()) {
            return "station id is empty";
        }
        for (std::size_t j = i + 1; j < ids.size(); ++j) {
            if (ids[i] == ids[j]) {
                return "stations of an observation must be distinct";
            }
        }
    }
    return {};
}

// First non-empty reason wins.
std::string firstOf(std::initializer_list<std::string> reasons)
{
    for (const std::string& reason : reasons) {
        if (!reason.empty()) {
            return reason;
        }
    }
    return {};
}

std::string valueProblem(const Observation& observation)
{
    return std::visit(
        Overloaded{
            [](const DistanceObservation& o) {
                return firstOf({checkFinite({{"distance", o.distance},
                                             {"instrument height", o.instrumentHeight},
                                             {"target height", o.targetHeight}}),
                                checkSigmas({{"sigma", o.sigma}}),
                                o.distance > 0.0 ? std::string{}
                                                 : std::string("distance must be positive")});
            },
            [](const HorizontalAngleObservation& o) {
                return firstOf({checkFinite({{"angle", o.angle}}), checkSigmas({{"sigma", o.sigma}})});
            },
            [](const VerticalAngleObservation& o) {
                return firstOf(
                    {checkFinite({{"angle", o.angle},
                                  {"instrument height", o.instrumentHeight},
                                  {"target height", o.targetHeight}}),
                     checkSigmas({{"sigma", o.sigma}}),
                     std::abs(o.angle) <= kHalfPi
                         ? std::string{}
                         : std::string("vertical angle must lie in [-pi/2, pi/2]")});
            },
            [](const ZenithAngleObservation& o) {
                return firstOf({checkFinite({{"angle", o.angle},
                                             {"instrument height", o.instrumentHeight},
                                             {"target height", o.targetHeight}}),
                                checkSigmas({{"sigma", o.sigma}}),
                                (o.angle >= 0.0 && o.angle <= kPi)
                                    ? std::string{}
                                    : std::string("zenith angle must lie in [0, pi]")});
            },
            [](const AzimuthObservation& o) {
                return firstOf(
                    {checkFinite({{"azimuth", o.azimuth}}), checkSigmas({{"sigma", o.sigma}})});
            },
            [](const GnssBaselineObservation& o) {
                return firstOf({checkFinite({{"delta northing", o.deltaNorthing},
                                             {"delta easting", o.deltaEasting},
                                             {"delta up", o.deltaUp}}),
                                checkSigmas({{"sigma northing", o.sigmaNorthing},
                                             {"sigma easting", o.sigmaEasting},
                                             {"sigma up", o.sigmaUp}})});
            },
            [](const GnssPositionObservation& o) {
                return firstOf({checkFinite({{"northing", o.northing},
                                             {"easting", o.easting},
                                             {"elevation", o.elevation}}),
                                checkSigmas({{"sigma northing", o.sigmaNorthing},
                                             {"sigma easting", o.sigmaEasting},
                                             {"sigma elevation", o.sigmaElevation}})});
            },
            [](const LevelDifferenceObservation& o) {
                return firstOf({checkFinite({{"height difference", o.heightDifference},
                                             {"length", o.length}}),
                                checkSigmas({{"sigma", o.sigma}}),
                                o.length >= 0.0 ? std::string{}
                                                : std::string("length must not be negative")});
            },
        },
        observation);
}

} // namespace

std::string observationKindName(const Observation& observation)
{
    return std::visit(
        Overloaded{
            [](const DistanceObservation& o) {
                return std::string(o.kind == DistanceKind::Horizontal ? "horizontal distance"
                                                                      : "slope distance");
            },
            [](const HorizontalAngleObservation&) { return std::string("horizontal angle"); },
            [](const VerticalAngleObservation&) { return std::string("vertical angle"); },
            [](const ZenithAngleObservation&) { return std::string("zenith angle"); },
            [](const AzimuthObservation&) { return std::string("azimuth"); },
            [](const GnssBaselineObservation&) { return std::string("GNSS baseline"); },
            [](const GnssPositionObservation&) { return std::string("GNSS position"); },
            [](const LevelDifferenceObservation&) { return std::string("level difference"); },
        },
        observation);
}

std::vector<std::string> referencedPoints(const Observation& observation)
{
    using Ids = std::vector<std::string>;
    return std::visit(
        Overloaded{
            [](const HorizontalAngleObservation& o) { return Ids{o.at, o.from, o.to}; },
            [](const GnssPositionObservation& o) { return Ids{o.point}; },
            [](const auto& o) { return Ids{o.from, o.to}; },
        },
        observation);
}

Status validateObservation(const Observation& observation)
{
    if (std::string reason = checkStations(referencedPoints(observation)); !reason.empty()) {
        return invalid(observation, std::move(reason));
    }
    if (std::string reason = valueProblem(observation); !reason.empty()) {
        return invalid(observation, std::move(reason));
    }
    return {};
}

Observation normalizedObservation(Observation observation)
{
    if (auto* angle = std::get_if<HorizontalAngleObservation>(&observation)) {
        angle->angle = normalizeAngle(angle->angle);
    } else if (auto* azimuth = std::get_if<AzimuthObservation>(&observation)) {
        azimuth->azimuth = normalizeAngle(azimuth->azimuth);
    }
    return observation;
}

// ---- ControlPoint ------------------------------------------------------------

ControlPoint ControlPoint::fixedHorizontal(std::string pointId)
{
    ControlPoint control;
    control.pointId = std::move(pointId);
    control.northing.constraint = ControlConstraint::Fixed;
    control.easting.constraint = ControlConstraint::Fixed;
    return control;
}

ControlPoint ControlPoint::fixedVertical(std::string pointId)
{
    ControlPoint control;
    control.pointId = std::move(pointId);
    control.elevation.constraint = ControlConstraint::Fixed;
    return control;
}

ControlPoint ControlPoint::fixed3d(std::string pointId)
{
    ControlPoint control = fixedHorizontal(std::move(pointId));
    control.elevation.constraint = ControlConstraint::Fixed;
    return control;
}

ControlPoint ControlPoint::weightedHorizontal(std::string pointId, double sigmaNorthing,
                                              double sigmaEasting)
{
    ControlPoint control;
    control.pointId = std::move(pointId);
    control.northing = ControlComponent{ControlConstraint::Weighted, sigmaNorthing};
    control.easting = ControlComponent{ControlConstraint::Weighted, sigmaEasting};
    return control;
}

ControlPoint ControlPoint::weightedVertical(std::string pointId, double sigma)
{
    ControlPoint control;
    control.pointId = std::move(pointId);
    control.elevation = ControlComponent{ControlConstraint::Weighted, sigma};
    return control;
}

} // namespace katana::survey
