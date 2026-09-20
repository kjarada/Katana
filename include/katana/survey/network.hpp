#pragma once

// SurveyNetwork: the container that owns points, control, setups and
// observations of one survey (PLAN.MD Phase 10).
//
// Invariants, enforced by every mutator (a failed call leaves the network
// unchanged):
//   * point ids are unique and non-empty, coordinates are finite;
//   * every observation, control record and setup refers to existing points;
//   * every stored observation satisfies validateObservation(), with azimuths and
//     horizontal angles normalised into [0, 2*pi).
//
// Iteration order is insertion order, so output derived from a network is
// deterministic. Observations are addressed by index; removing one shifts the
// indices above it.
//
// Threading: a plain value. Concurrent const access is safe, any mutation
// requires external synchronisation.

#include <cstddef>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/survey/data_model.hpp"

namespace katana::survey {

class SurveyNetwork {
  public:
    // ---- points ----
    // AlreadyExists for a duplicate id, InvalidArgument for an empty id or
    // non-finite coordinates.
    katana::core::Status addPoint(SurveyPoint point);
    // Replaces the point with the same id. NotFound / InvalidArgument.
    katana::core::Status updatePoint(SurveyPoint point);
    // InvalidState while an observation, control record or setup still refers to
    // the point; NotFound when absent.
    katana::core::Status removePoint(std::string_view id);

    [[nodiscard]] bool containsPoint(std::string_view id) const;
    [[nodiscard]] std::optional<std::size_t> pointIndex(std::string_view id) const;
    [[nodiscard]] katana::core::Result<SurveyPoint> point(std::string_view id) const;
    [[nodiscard]] const std::vector<SurveyPoint>& points() const { return points_; }

    // ---- control ----
    // NotFound for an unknown point, AlreadyExists when the point already has a
    // control record, InvalidArgument when no component is constrained or a
    // weighted component lacks a positive finite sigma.
    katana::core::Status addControlPoint(ControlPoint control);
    katana::core::Status removeControlPoint(std::string_view pointId);
    [[nodiscard]] std::optional<ControlPoint> controlFor(std::string_view pointId) const;
    [[nodiscard]] const std::vector<ControlPoint>& controlPoints() const { return control_; }

    // ---- setups ----
    katana::core::Status addStation(Station station);
    katana::core::Status removeStation(std::string_view stationId);
    [[nodiscard]] const std::vector<Station>& stations() const { return stations_; }

    // ---- observations ----
    // Returns the index of the stored observation. InvalidSurveyObservation for
    // bad values, NotFound for an unknown station.
    katana::core::Result<std::size_t> addObservation(Observation observation);
    katana::core::Status removeObservation(std::size_t index);
    [[nodiscard]] const std::vector<Observation>& observations() const { return observations_; }

  private:
    [[nodiscard]] bool isReferenced(std::string_view pointId) const;
    void rebuildIndex();

    std::vector<SurveyPoint> points_;
    std::map<std::string, std::size_t, std::less<>> index_; // id -> position in points_
    std::vector<ControlPoint> control_;
    std::vector<Station> stations_;
    std::vector<Observation> observations_;
};

} // namespace katana::survey
