#pragma once

// Differential levelling (PLAN.MD Phase 12).
//
// A level run starts on a benchmark of known elevation. At every setup the
// height of instrument is HI = elevation(back point) + backsight, and the
// forward point gets elevation = HI - foresight. When the run closes on a
// benchmark (the start itself for a loop) the misclosure
//     misclosure = computed closing elevation - known closing elevation
// is distributed proportionally to the number of setups or to the distance
// levelled. Intermediate sights are not modelled; record them as separate runs.

#include <optional>
#include <string>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::survey {

struct LevelSetup {
    std::string foresightPointId; // point that receives an elevation; may be empty
    double backsight = 0.0;       // staff reading on the point of known elevation, metres
    double foresight = 0.0;       // staff reading on the forward point, metres
    double distance = 0.0;        // backsight + foresight sight lengths, metres; 0 when unknown
};

struct LevelRun {
    std::string name;
    double startElevation = 0.0; // opening benchmark
    std::vector<LevelSetup> setups;
    std::optional<double> closingElevation; // known elevation of the last forward point
};

enum class LevelAdjustment {
    None,
    BySetups,   // equal correction per setup
    ByDistance, // correction proportional to the distance levelled
};

struct LevelPointResult {
    std::string pointId;
    double heightOfInstrument = 0.0;
    double elevation = 0.0;  // unadjusted
    double correction = 0.0; // cumulative correction applied to this point
    double adjustedElevation = 0.0;
};

struct LevelRunResult {
    std::vector<LevelPointResult> points; // one per setup, in order
    double sumBacksights = 0.0;
    double sumForesights = 0.0;
    double totalDistance = 0.0;
    std::optional<double> misclosure; // nullopt without a closing elevation
};

// InvalidArgument for an empty run, non-finite readings, negative distances, an
// adjustment without a closing elevation, or ByDistance with zero total distance.
[[nodiscard]] katana::core::Result<LevelRunResult>
computeLevelRun(const LevelRun& run, LevelAdjustment adjustment = LevelAdjustment::None);

// Allowable misclosure k * sqrt(K) of a level line of `lengthMetres`, with K in
// kilometres. `k` is in metres per sqrt(km): 0.012 for the common 12 mm * sqrt(K)
// third-order criterion. The result is in metres.
[[nodiscard]] katana::core::Result<double> allowableLevelMisclosure(double k, double lengthMetres);

// A-priori standard deviation of a levelled height difference over
// `lengthMetres`: sigmaPerSqrtKm * sqrt(K). Metres.
[[nodiscard]] katana::core::Result<double> levelDifferenceSigma(double sigmaPerSqrtKm,
                                                                double lengthMetres);

} // namespace katana::survey
