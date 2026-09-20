#pragma once

// Survey data model (PLAN.MD Phase 10).
//
// Surveying entities are plain values, kept apart from graphical CAD entities.
//
// Conventions
//   * Lengths are metres, angles radians, standard deviations in the unit of the
//     value they describe. Every observation carries an a-priori standard
//     deviation `sigma` > 0.
//   * Azimuths and horizontal angles are clockwise, in [0, 2*pi).
//   * Points are referenced by their string id. Containers that are iterated for
//     output are ordered (vector / std::map), never hash based, so results do not
//     depend on hashing.

#include <map>
#include <optional>
#include <string>
#include <variant>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/survey/coordinate.hpp"

namespace katana::survey {

struct SurveyPoint {
    std::string id; // unique within a network; doubles as the point name
    double northing = 0.0;
    double easting = 0.0;
    double elevation = 0.0;
    std::string code;        // feature code, e.g. "IP", "EOP"
    std::string description; // free text
    std::map<std::string, std::string> metadata;

    [[nodiscard]] Coordinate2 position() const { return Coordinate2{northing, easting}; }

    friend bool operator==(const SurveyPoint&, const SurveyPoint&) = default;
};

// ---- Observations ------------------------------------------------------------

enum class DistanceKind { Horizontal, Slope };

// Distance from `from` to `to`. Slope distances carry the instrument and target
// heights needed to reduce them; both are ignored for horizontal distances.
struct DistanceObservation {
    std::string from;
    std::string to;
    double distance = 0.0; // metres, > 0
    double sigma = 0.0;    // metres
    DistanceKind kind = DistanceKind::Horizontal;
    double instrumentHeight = 0.0;
    double targetHeight = 0.0;

    friend bool operator==(const DistanceObservation&, const DistanceObservation&) = default;
};

// Clockwise angle measured at `at`, turning from the backsight `from` to the
// foresight `to`: azimuth(at->to) - azimuth(at->from), wrapped into [0, 2*pi).
struct HorizontalAngleObservation {
    std::string at;
    std::string from;
    std::string to;
    double angle = 0.0;
    double sigma = 0.0; // radians

    friend bool operator==(const HorizontalAngleObservation&,
                           const HorizontalAngleObservation&) = default;
};

// Elevation angle above (+) or below (-) the horizon, in [-pi/2, pi/2].
struct VerticalAngleObservation {
    std::string from;
    std::string to;
    double angle = 0.0;
    double sigma = 0.0; // radians
    double instrumentHeight = 0.0;
    double targetHeight = 0.0;

    friend bool operator==(const VerticalAngleObservation&,
                           const VerticalAngleObservation&) = default;
};

// Angle from the zenith, in [0, pi]; pi/2 is horizontal.
struct ZenithAngleObservation {
    std::string from;
    std::string to;
    double angle = 0.0;
    double sigma = 0.0; // radians
    double instrumentHeight = 0.0;
    double targetHeight = 0.0;

    friend bool operator==(const ZenithAngleObservation&,
                           const ZenithAngleObservation&) = default;
};

// Grid azimuth of the line `from` -> `to` (gyro, astronomic or a known bearing).
struct AzimuthObservation {
    std::string from;
    std::string to;
    double azimuth = 0.0;
    double sigma = 0.0; // radians

    friend bool operator==(const AzimuthObservation&, const AzimuthObservation&) = default;
};

// GNSS baseline `from` -> `to`, already reduced to grid components.
struct GnssBaselineObservation {
    std::string from;
    std::string to;
    double deltaNorthing = 0.0;
    double deltaEasting = 0.0;
    double deltaUp = 0.0;
    double sigmaNorthing = 0.0;
    double sigmaEasting = 0.0;
    double sigmaUp = 0.0;

    friend bool operator==(const GnssBaselineObservation&,
                           const GnssBaselineObservation&) = default;
};

// GNSS position of `point` in the grid system of the network.
struct GnssPositionObservation {
    std::string point;
    double northing = 0.0;
    double easting = 0.0;
    double elevation = 0.0;
    double sigmaNorthing = 0.0;
    double sigmaEasting = 0.0;
    double sigmaElevation = 0.0;

    friend bool operator==(const GnssPositionObservation&,
                           const GnssPositionObservation&) = default;
};

// Levelled height difference H(to) - H(from). `length` is the length of the
// level line in metres (0 when unknown); it is informational, the weight always
// comes from `sigma`.
struct LevelDifferenceObservation {
    std::string from;
    std::string to;
    double heightDifference = 0.0;
    double sigma = 0.0; // metres
    double length = 0.0;

    friend bool operator==(const LevelDifferenceObservation&,
                           const LevelDifferenceObservation&) = default;
};

using Observation =
    std::variant<DistanceObservation, HorizontalAngleObservation, VerticalAngleObservation,
                 ZenithAngleObservation, AzimuthObservation, GnssBaselineObservation,
                 GnssPositionObservation, LevelDifferenceObservation>;

// Short name of the alternative held by `observation`, for diagnostics.
[[nodiscard]] std::string observationKindName(const Observation& observation);

// Ids of the points the observation refers to, in declaration order.
[[nodiscard]] std::vector<std::string> referencedPoints(const Observation& observation);

// Checks the values of an observation on their own: finite numbers, sigma > 0,
// distance > 0, angles in range, distinct non-empty station ids. Does not know
// whether the stations exist; SurveyNetwork::addObservation checks that.
// InvalidSurveyObservation on failure.
[[nodiscard]] katana::core::Status validateObservation(const Observation& observation);

// Copy with azimuths and horizontal angles wrapped into [0, 2*pi). Other
// observations are returned unchanged.
[[nodiscard]] Observation normalizedObservation(Observation observation);

// ---- Setups and control --------------------------------------------------------

// One instrument setup. Target heights belong to the individual observations.
struct Station {
    std::string id;      // setup identifier, unique within a network
    std::string pointId; // occupied point
    double instrumentHeight = 0.0;

    friend bool operator==(const Station&, const Station&) = default;
};

enum class ControlConstraint {
    Free,     // adjusted like any other unknown
    Fixed,    // held at its published value, removed from the unknowns
    Weighted, // published value enters the adjustment as an observation
};

struct ControlComponent {
    ControlConstraint constraint = ControlConstraint::Free;
    double sigma = 0.0; // metres; required > 0 when Weighted, ignored otherwise

    friend bool operator==(const ControlComponent&, const ControlComponent&) = default;
};

// Declares which coordinate components of a point are control. The published
// values are the coordinates of the SurveyPoint itself.
struct ControlPoint {
    std::string pointId;
    ControlComponent northing;
    ControlComponent easting;
    ControlComponent elevation;

    [[nodiscard]] static ControlPoint fixedHorizontal(std::string pointId);
    [[nodiscard]] static ControlPoint fixedVertical(std::string pointId);
    [[nodiscard]] static ControlPoint fixed3d(std::string pointId);
    [[nodiscard]] static ControlPoint weightedHorizontal(std::string pointId, double sigmaNorthing,
                                                         double sigmaEasting);
    [[nodiscard]] static ControlPoint weightedVertical(std::string pointId, double sigma);

    friend bool operator==(const ControlPoint&, const ControlPoint&) = default;
};

// ---- Traverse ------------------------------------------------------------------

enum class TraverseKind {
    Open,       // starts on control, ends on a new point: no closure check
    ClosedLoop, // returns to its first station; interior-angle procedure
    Link,       // runs from one control point to another
};

// One occupied traverse station.
struct TraverseSetup {
    std::string stationId;
    double angle = 0.0;    // clockwise, from the back station to the forward station
    double distance = 0.0; // horizontal, to the forward station; metres, > 0

    friend bool operator==(const TraverseSetup&, const TraverseSetup&) = default;
};

// Angle turned at the end station of a link traverse, from the last traverse
// station to a reference mark whose azimuth is known.
struct TraverseClosingAngle {
    double angle = 0.0;            // clockwise, last traverse station -> reference mark
    double referenceAzimuth = 0.0; // known azimuth end station -> reference mark

    friend bool operator==(const TraverseClosingAngle&, const TraverseClosingAngle&) = default;
};

// A traverse observed with angles and horizontal distances; self-contained.
//
//   Open / Link   `startAzimuth` is the known azimuth from the first station to
//                 its backsight reference; setups[0].angle is turned from that
//                 reference to the second station. The forward station of the
//                 last setup is `endStationId`.
//   ClosedLoop    `startAzimuth` is the known (or assumed) azimuth of the first
//                 leg setups[0] -> setups[1]; setups[0].angle is turned from the
//                 last station to the second one; the last setup sights forward
//                 to setups[0]. `endStationId`, `end` and `closingAngle` are
//                 unused. A loop oriented on an external reference mark is
//                 modelled as a Link traverse whose end equals its start.
//   Link          `end` holds the known coordinates of `endStationId`;
//                 `closingAngle` is optional (without it there is no angular
//                 check, only a linear one).
struct Traverse {
    std::string name;
    TraverseKind kind = TraverseKind::Open;
    Coordinate2 start; // known coordinates of setups[0]
    double startAzimuth = 0.0;
    std::vector<TraverseSetup> setups;
    std::string endStationId;
    Coordinate2 end;
    std::optional<TraverseClosingAngle> closingAngle;

    friend bool operator==(const Traverse&, const Traverse&) = default;
};

} // namespace katana::survey
