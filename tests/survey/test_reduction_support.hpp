#pragma once

// Builders for the reduction tests: a raw project the way a total-station
// parser hands it over (reduction.hpp's contract), written in degrees and
// metres so the hand working in each test reads like a field book.

#include <cmath>
#include <optional>
#include <string>
#include <vector>

#include "katana/math/numerics.hpp"
#include "katana/survey/reduction.hpp"

namespace reduction_test {

using namespace katana::survey;

inline double deg(double degrees, double minutes = 0.0, double seconds = 0.0)
{
    return (degrees + minutes / 60.0 + seconds / 3600.0) * katana::math::kPi / 180.0;
}

inline double arcSeconds(double seconds)
{
    return seconds * katana::math::kPi / 648000.0;
}

inline SurveyPoint point(std::string id, double northing, double easting,
                         std::optional<double> elevation = std::nullopt,
                         CoordinateSource source = CoordinateSource::Entered)
{
    SurveyPoint p;
    p.id = std::move(id);
    p.northing = northing;
    p.easting = easting;
    p.elevation = elevation;
    p.coordinateSource = source;
    return p;
}

inline UnpositionedPoint unpositioned(std::string id)
{
    UnpositionedPoint p;
    p.id = std::move(id);
    return p;
}

// One pointing: the circle reading, the zenith (FL-equivalent for face right,
// as the model stores it) and the slope distance, any of them absent.
struct Shot {
    std::string target;
    std::size_t index = 0;
    Face face = Face::Unknown;
    std::optional<double> direction{};
    std::optional<double> zenith{};
    std::optional<double> distance{};
    double targetHeight = 0.0;
    DistanceKind kind = DistanceKind::Slope;
};

inline SurveyStation setup(std::string id, std::string pointId, double instrumentHeight,
                           std::string backsight, const std::vector<Shot>& shots)
{
    SurveyStation station;
    station.setup = Station{std::move(id), pointId, instrumentHeight};
    station.backsightPointId = std::move(backsight);
    std::size_t record = 1;
    for (const Shot& shot : shots) {
        const SourceRecord source{"Test", "synthetic", "1", "field.raw", record++};
        const Pointing pointing{shot.index, shot.face};
        if (shot.direction) {
            HorizontalDirectionObservation direction;
            direction.at = pointId;
            direction.to = shot.target;
            direction.direction = *shot.direction;
            direction.sigma = arcSeconds(3.0);
            direction.source = source;
            direction.pointing = pointing;
            station.observations.push_back(direction);
        }
        if (shot.zenith) {
            ZenithAngleObservation zenith;
            zenith.from = pointId;
            zenith.to = shot.target;
            zenith.angle = *shot.zenith;
            zenith.sigma = arcSeconds(3.0);
            zenith.instrumentHeight = instrumentHeight;
            zenith.targetHeight = shot.targetHeight;
            zenith.source = source;
            zenith.pointing = pointing;
            station.observations.push_back(zenith);
        }
        if (shot.distance) {
            DistanceObservation distance;
            distance.from = pointId;
            distance.to = shot.target;
            distance.distance = *shot.distance;
            distance.sigma = 0.002;
            distance.kind = shot.kind;
            distance.instrumentHeight = instrumentHeight;
            distance.targetHeight = shot.targetHeight;
            distance.source = source;
            distance.pointing = pointing;
            station.observations.push_back(distance);
        }
    }
    return station;
}

// Settings with every correction off, so a test can turn on the one it is
// about and nothing else moves the numbers.
inline ReductionSettings bareSettings()
{
    ReductionSettings settings;
    settings.atmospheric = AtmosphericCorrection::None;
    settings.prismConstantPolicy = PrismConstantPolicy::None;
    settings.curvatureAndRefraction = false;
    return settings;
}

inline const ReportObservation* findRow(const ReductionReport& report, const std::string& kind,
                                        const std::string& to, std::size_t nth = 0)
{
    for (const ReportObservation& row : report.observations) {
        if (row.kind == kind && row.to == to) {
            if (nth == 0) {
                return &row;
            }
            --nth;
        }
    }
    return nullptr;
}

inline const AppliedCorrection* findCorrection(const ReportObservation& row, CorrectionKind kind)
{
    for (const AppliedCorrection& correction : row.corrections) {
        if (correction.kind == kind) {
            return &correction;
        }
    }
    return nullptr;
}

inline const ComputedPoint* findPoint(const ReductionOutcome& outcome, const std::string& id)
{
    for (const ComputedPoint& p : outcome.points) {
        if (p.id == id) {
            return &p;
        }
    }
    return nullptr;
}

} // namespace reduction_test
