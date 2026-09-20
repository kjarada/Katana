#pragma once

// Classical traverse computation (PLAN.MD Phase 12): azimuth propagation,
// angular misclosure and balancing, latitudes and departures, linear
// misclosure, relative precision and the compass (Bowditch) or transit rule.
//
// See data_model.hpp for the meaning of the Traverse fields per kind. Nothing is
// printed; every intermediate quantity is reported in TraverseResult.
//
// Sign conventions
//   * misclosure = computed - known (angular and linear); corrections have the
//     opposite sign.
//   * latitude = d * cos(azimuth) (north positive), departure = d * sin(azimuth)
//     (east positive).

#include <optional>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/survey/coordinate.hpp"
#include "katana/survey/data_model.hpp"

namespace katana::survey {

enum class TraverseAdjustment {
    None,    // report misclosures only
    Compass, // Bowditch: corrections proportional to leg length
    Transit, // corrections proportional to |latitude| and |departure|
};

struct TraverseOptions {
    // Distribute the angular misclosure equally over the measured angles before
    // computing azimuths. Ignored where no angular check exists.
    bool balanceAngles = true;
    // Ignored for open traverses, which have no linear misclosure.
    TraverseAdjustment adjustment = TraverseAdjustment::Compass;
};

struct TraverseLegResult {
    std::string fromId;
    std::string toId;
    double azimuth = 0.0;  // from balanced angles when balancing is on
    double distance = 0.0; // as observed
    double latitude = 0.0;
    double departure = 0.0;
    double latitudeCorrection = 0.0;
    double departureCorrection = 0.0;
};

struct TraverseStationResult {
    std::string id;
    Coordinate2 unadjusted; // from (balanced) azimuths and observed distances
    Coordinate2 adjusted;   // after the linear adjustment; equals `unadjusted` for None / Open
};

struct LinearMisclosure {
    double latitude = 0.0;  // computed - known closing northing
    double departure = 0.0; // computed - known closing easting
    double length = 0.0;    // hypot(latitude, departure)
    // length / total traverse length; 0 for a perfect closure.
    double relativePrecision = 0.0;
    // The N of "1 : N"; nullopt when the closure is exact.
    std::optional<double> precisionDenominator;
};

struct TraverseResult {
    TraverseKind kind = TraverseKind::Open;
    std::size_t angleCount = 0; // angles taking part in the angular check
    // computed - expected closing direction, in (-pi, pi]; nullopt without an
    // angular check (open traverse, link traverse without closing angle).
    std::optional<double> angularMisclosure;
    double angleCorrection = 0.0; // added to every measured angle
    std::vector<TraverseLegResult> legs;
    // One entry per station in traverse order, the start first. For a closed
    // loop the start is not repeated at the end; for open and link traverses the
    // last entry is the end station.
    std::vector<TraverseStationResult> stations;
    double totalLength = 0.0;
    std::optional<LinearMisclosure> linearMisclosure; // nullopt for open traverses
};

// InvalidArgument when the traverse is malformed: no setups (fewer than 3 for a
// loop), empty or repeated station ids, non-finite values, non-positive
// distances, or fields that the kind requires are missing.
[[nodiscard]] katana::core::Status validateTraverse(const Traverse& traverse);

[[nodiscard]] katana::core::Result<TraverseResult>
computeTraverse(const Traverse& traverse, const TraverseOptions& options = {});

} // namespace katana::survey
