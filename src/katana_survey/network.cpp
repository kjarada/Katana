#include "katana/survey/network.hpp"

#include <algorithm>
#include <cmath>
#include <utility>

namespace katana::survey {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

Status checkPointValues(const SurveyPoint& point)
{
    if (point.id.empty()) {
        return makeError(ErrorCode::InvalidArgument, "point id is empty");
    }
    if (!std::isfinite(point.northing) || !std::isfinite(point.easting) ||
        !std::isfinite(point.elevation)) {
        return makeError(ErrorCode::InvalidArgument, "point coordinates are not finite",
                         "point " + point.id);
    }
    return {};
}

Status checkControlComponent(const ControlComponent& component, const std::string& pointId,
                             const char* name)
{
    if (component.constraint != ControlConstraint::Weighted) {
        return {};
    }
    if (!std::isfinite(component.sigma) || !(component.sigma > 0.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string("weighted control needs a positive finite sigma for its ") +
                             name,
                         "point " + pointId);
    }
    return {};
}

} // namespace

// ---- points ------------------------------------------------------------------

Status SurveyNetwork::addPoint(SurveyPoint point)
{
    if (Status status = checkPointValues(point); !status) {
        return status;
    }
    if (containsPoint(point.id)) {
        return makeError(ErrorCode::AlreadyExists, "a point with this id already exists",
                         "point " + point.id);
    }
    index_.emplace(point.id, points_.size());
    points_.push_back(std::move(point));
    return {};
}

Status SurveyNetwork::updatePoint(SurveyPoint point)
{
    if (Status status = checkPointValues(point); !status) {
        return status;
    }
    const auto index = pointIndex(point.id);
    if (!index) {
        return makeError(ErrorCode::NotFound, "no point with this id", "point " + point.id);
    }
    points_[*index] = std::move(point);
    return {};
}

Status SurveyNetwork::removePoint(std::string_view id)
{
    const auto index = pointIndex(id);
    if (!index) {
        return makeError(ErrorCode::NotFound, "no point with this id",
                         "point " + std::string(id));
    }
    if (isReferenced(id)) {
        return makeError(ErrorCode::InvalidState,
                         "point is still referenced by an observation, control record or setup",
                         "point " + std::string(id));
    }
    points_.erase(points_.begin() + static_cast<std::ptrdiff_t>(*index));
    rebuildIndex();
    return {};
}

bool SurveyNetwork::containsPoint(std::string_view id) const
{
    return index_.find(id) != index_.end();
}

std::optional<std::size_t> SurveyNetwork::pointIndex(std::string_view id) const
{
    const auto found = index_.find(id);
    if (found == index_.end()) {
        return std::nullopt;
    }
    return found->second;
}

Result<SurveyPoint> SurveyNetwork::point(std::string_view id) const
{
    const auto index = pointIndex(id);
    if (!index) {
        return makeError(ErrorCode::NotFound, "no point with this id",
                         "point " + std::string(id));
    }
    return points_[*index];
}

// ---- control -----------------------------------------------------------------

Status SurveyNetwork::addControlPoint(ControlPoint control)
{
    if (!containsPoint(control.pointId)) {
        return makeError(ErrorCode::NotFound, "control refers to an unknown point",
                         "point " + control.pointId);
    }
    if (controlFor(control.pointId)) {
        return makeError(ErrorCode::AlreadyExists, "the point already has a control record",
                         "point " + control.pointId);
    }
    if (control.northing.constraint == ControlConstraint::Free &&
        control.easting.constraint == ControlConstraint::Free &&
        control.elevation.constraint == ControlConstraint::Free) {
        return makeError(ErrorCode::InvalidArgument,
                         "a control record must constrain at least one component",
                         "point " + control.pointId);
    }
    if (Status status = checkControlComponent(control.northing, control.pointId, "northing");
        !status) {
        return status;
    }
    if (Status status = checkControlComponent(control.easting, control.pointId, "easting");
        !status) {
        return status;
    }
    if (Status status = checkControlComponent(control.elevation, control.pointId, "elevation");
        !status) {
        return status;
    }
    control_.push_back(std::move(control));
    return {};
}

Status SurveyNetwork::removeControlPoint(std::string_view pointId)
{
    const auto found = std::find_if(control_.begin(), control_.end(),
                                    [&](const ControlPoint& c) { return c.pointId == pointId; });
    if (found == control_.end()) {
        return makeError(ErrorCode::NotFound, "the point has no control record",
                         "point " + std::string(pointId));
    }
    control_.erase(found);
    return {};
}

std::optional<ControlPoint> SurveyNetwork::controlFor(std::string_view pointId) const
{
    const auto found = std::find_if(control_.begin(), control_.end(),
                                    [&](const ControlPoint& c) { return c.pointId == pointId; });
    if (found == control_.end()) {
        return std::nullopt;
    }
    return *found;
}

// ---- setups ------------------------------------------------------------------

Status SurveyNetwork::addStation(Station station)
{
    if (station.id.empty()) {
        return makeError(ErrorCode::InvalidArgument, "setup id is empty");
    }
    if (!std::isfinite(station.instrumentHeight)) {
        return makeError(ErrorCode::InvalidArgument, "instrument height is not finite",
                         "setup " + station.id);
    }
    if (!containsPoint(station.pointId)) {
        return makeError(ErrorCode::NotFound, "setup occupies an unknown point",
                         "setup " + station.id + " at point " + station.pointId);
    }
    const bool duplicate = std::any_of(stations_.begin(), stations_.end(),
                                       [&](const Station& s) { return s.id == station.id; });
    if (duplicate) {
        return makeError(ErrorCode::AlreadyExists, "a setup with this id already exists",
                         "setup " + station.id);
    }
    stations_.push_back(std::move(station));
    return {};
}

Status SurveyNetwork::removeStation(std::string_view stationId)
{
    const auto found = std::find_if(stations_.begin(), stations_.end(),
                                    [&](const Station& s) { return s.id == stationId; });
    if (found == stations_.end()) {
        return makeError(ErrorCode::NotFound, "no setup with this id",
                         "setup " + std::string(stationId));
    }
    stations_.erase(found);
    return {};
}

// ---- observations ------------------------------------------------------------

Result<std::size_t> SurveyNetwork::addObservation(Observation observation)
{
    if (Status status = validateObservation(observation); !status) {
        return status.error();
    }
    for (const std::string& id : referencedPoints(observation)) {
        if (!containsPoint(id)) {
            return makeError(ErrorCode::NotFound, "observation refers to an unknown station",
                             observationKindName(observation) + ", station " + id);
        }
    }
    observations_.push_back(normalizedObservation(std::move(observation)));
    return observations_.size() - 1;
}

Status SurveyNetwork::removeObservation(std::size_t index)
{
    if (index >= observations_.size()) {
        return makeError(ErrorCode::NotFound, "observation index out of range",
                         "index " + std::to_string(index) + ", count " +
                             std::to_string(observations_.size()));
    }
    observations_.erase(observations_.begin() + static_cast<std::ptrdiff_t>(index));
    return {};
}

// ---- helpers -----------------------------------------------------------------

bool SurveyNetwork::isReferenced(std::string_view pointId) const
{
    if (controlFor(pointId)) {
        return true;
    }
    if (std::any_of(stations_.begin(), stations_.end(),
                    [&](const Station& s) { return s.pointId == pointId; })) {
        return true;
    }
    return std::any_of(observations_.begin(), observations_.end(), [&](const Observation& o) {
        const auto ids = referencedPoints(o);
        return std::find(ids.begin(), ids.end(), pointId) != ids.end();
    });
}

void SurveyNetwork::rebuildIndex()
{
    index_.clear();
    for (std::size_t i = 0; i < points_.size(); ++i) {
        index_.emplace(points_[i].id, i);
    }
}

} // namespace katana::survey
