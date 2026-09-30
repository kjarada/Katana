#pragma once

// Least-squares adjustment of survey networks (PLAN.MD Phase 13).
//
//   adjustHorizontalNetwork  2D, non-linear, Gauss-Newton on observation equations
//   adjustLevelNetwork       1D, linear
//   adjustTraverse           a traverse turned into a network and adjusted rigorously
//
// Functional model of the horizontal adjustment (i = instrument, j = target,
// dN = N_j - N_i, dE = E_j - E_i, d^2 = dN^2 + dE^2):
//   distance   d_ij                                    partials wrt (N_j, E_j): dN/d, dE/d
//   azimuth    atan2(dE, dN)                           partials wrt (N_j, E_j): -dE/d^2, dN/d^2
//   angle      azimuth(at->to) - azimuth(at->from)     difference of two azimuth rows
//   direction  azimuth(at->to) - o_at                  an azimuth row, and -1 wrt o_at
//   GNSS baseline (dN, dE), GNSS position (N, E), weighted control: linear
// Partials with respect to the instrument station have the opposite sign.
// Angular misclosures are wrapped into (-pi, pi], so azimuths near 0 / 2*pi and
// angles near a full turn are handled correctly.
//
// A horizontal direction is a circle reading, which becomes an azimuth only
// with the circle's orientation: every direction read at one point is one SET
// with one orientation unknown o (azimuth = direction + o), started at the mean
// of azimuth less reading over the set and adjusted with the coordinates. The
// set is the point's, not a setup's: the network has no notion of a setup, so
// two setups on one point, whose circles were set apart, must not both be given
// to it as directions (the reduction's network gives them as angles; its
// resection, one setup at a time, as directions). Rejected: the angles from a
// reference pointing that the reduction's network uses. They share that
// pointing's error, which uncorrelated weights ignore, so a short first sight
// - its centring error in every angle - spoils all of them; a set weights each
// direction on its own, as the textbooks' and the field software's
// free-station adjustments do.
//
// Stochastic model: uncorrelated observations, weight 1 / sigma^2, a-priori
// variance factor 1. Observations that carry no horizontal information (slope
// distances, vertical and zenith angles, level differences) are skipped by the
// horizontal adjustment and listed in `unusedObservations`; the level adjustment
// uses level differences only.
//
// Approximate coordinates are taken from the network's points; Gauss-Newton needs
// them roughly right (a traverse computation or a sketch-quality fix suffices).
//
// The functions do not modify the network, hold no state and are safe to call
// concurrently on the same network. Identical input gives bitwise identical
// output.

#include <cstddef>
#include <optional>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/core/log.hpp"
#include "katana/math/numerics.hpp"
#include "katana/survey/data_model.hpp"
#include "katana/survey/error_propagation.hpp"
#include "katana/survey/least_squares.hpp"
#include "katana/survey/matrix.hpp"
#include "katana/survey/network.hpp"

namespace katana::survey {

// Metres. Gauss-Newton stops when the largest coordinate correction of an
// iteration falls below this. It is the project-wide geometric tolerance
// (0.1 micrometre): positions that differ by less are identical by project
// policy, and it still lies ~50 ulp above the resolution of UTM sized doubles.
inline constexpr double kDefaultConvergenceThreshold = katana::math::tolerance::kGeometric;

// Iterations. Gauss-Newton converges quadratically near the solution; from
// sketch-quality approximate coordinates it needs 3 to 6 iterations. Reaching
// this limit means divergence (bad approximate coordinates or gross errors).
inline constexpr std::size_t kDefaultMaxIterations = 25;

struct AdjustmentOptions {
    std::size_t maxIterations = kDefaultMaxIterations;
    double convergenceThreshold = kDefaultConvergenceThreshold; // metres, > 0
    CovarianceScaling covarianceScaling = CovarianceScaling::APosteriori;
    // Significance level alpha of the two-sided global test, in (0, 1).
    double significanceLevel = 0.05;
    // Optional, non-owning. Receives one "survey" record per adjustment with
    // counts and statistics, never coordinates.
    katana::core::Logger* logger = nullptr;
};

// Orientation is not a coordinate: it is the orientation unknown of the
// direction set read at the point (see the functional model above), named by
// that point like its coordinates.
enum class CoordinateComponent { Northing, Easting, Elevation, Orientation };

// Identifies one unknown: row / column k of the cofactor matrix.
struct AdjustedParameter {
    std::string pointId;
    CoordinateComponent component = CoordinateComponent::Northing;

    friend bool operator==(const AdjustedParameter&, const AdjustedParameter&) = default;
};

enum class ResidualSource {
    Observed, // `index` refers to SurveyNetwork::observations()
    Control,  // `index` refers to SurveyNetwork::controlPoints() (weighted components)
};

enum class ResidualComponent { Value, Northing, Easting, Elevation };

struct ResidualRecord {
    ResidualSource source = ResidualSource::Observed;
    std::size_t index = 0;
    // Value for scalar observations; GNSS and control rows name their component.
    ResidualComponent component = ResidualComponent::Value;
    double residual = 0.0; // v = adjusted - observed; metres or radians
    double sigma = 0.0;    // a-priori standard deviation of the observation
    double redundancyNumber = 0.0;
    // v / (sigma * sqrt(r_i)), i.e. with sigma0 = 1; nullopt when r_i ~ 0.
    std::optional<double> standardizedResidual;
};

// Two-sided chi-square test of H0: sigma0^2 = 1 on the statistic v^T P v.
struct GlobalTest {
    double statistic = 0.0;     // v^T P v
    double lowerCritical = 0.0; // chi2(r, alpha/2)
    double upperCritical = 0.0; // chi2(r, 1 - alpha/2)
    double significanceLevel = 0.0;
    bool passed = false; // lowerCritical <= statistic <= upperCritical
};

struct AdjustmentStatistics {
    std::size_t observationCount = 0; // equations, including weighted control
    std::size_t unknownCount = 0;
    std::size_t degreesOfFreedom = 0;
    double weightedSquaredResiduals = 0.0; // v^T P v
    std::optional<double> varianceFactor;  // nullopt when degreesOfFreedom = 0
    double covarianceScale = 1.0;          // factor between cofactor and covariance
    std::optional<GlobalTest> globalTest;  // nullopt when degreesOfFreedom = 0
};

struct AdjustedStation {
    std::string pointId;
    Coordinate2 position;
    // Scaled by covarianceScale; zero for fixed components.
    Covariance2 covariance;
    double sigmaNorthing = 0.0;
    double sigmaEasting = 0.0;
    ErrorEllipse ellipse; // standard ellipse of `covariance`
};

// The orientation of the direction set read at one point.
struct AdjustedOrientation {
    std::string pointId;
    double orientation = 0.0; // radians in [0, 2*pi): azimuth = direction + orientation
    double sigma = 0.0;       // scaled by covarianceScale
};

struct HorizontalAdjustmentResult {
    // Every point that takes part in the adjustment (including fixed control), in
    // network order.
    std::vector<AdjustedStation> stations;
    // One per point directions were read at, in network order.
    std::vector<AdjustedOrientation> orientations;
    std::vector<ResidualRecord> residuals; // observation order, then control
    std::vector<std::size_t> unusedObservations;
    std::vector<AdjustedParameter> parameters; // order of the cofactor matrix
    Matrix cofactor;                           // Qxx
    Matrix covariance;                         // covarianceScale * Qxx
    AdjustmentStatistics statistics;
    std::size_t iterations = 0;    // coordinate updates applied
    double lastCorrection = 0.0;   // largest |correction| of the final update, metres
};

struct AdjustedElevation {
    std::string pointId;
    double elevation = 0.0;
    double sigma = 0.0; // scaled by covarianceScale; zero for fixed benchmarks
};

struct LevelAdjustmentResult {
    std::vector<AdjustedElevation> elevations; // participating points, network order
    std::vector<ResidualRecord> residuals;
    std::vector<std::size_t> unusedObservations;
    std::vector<AdjustedParameter> parameters;
    Matrix cofactor;
    Matrix covariance;
    AdjustmentStatistics statistics;
};

// Errors (both adjustments)
//   InvalidArgument           bad options
//   AdjustmentFailure         no usable observations, no free coordinates, fewer
//                             equations than unknowns, datum defect / rank
//                             deficiency (the message names the unresolved
//                             unknowns), coincident stations, divergence
[[nodiscard]] katana::core::Result<HorizontalAdjustmentResult>
adjustHorizontalNetwork(const SurveyNetwork& network, const AdjustmentOptions& options = {});

[[nodiscard]] katana::core::Result<LevelAdjustmentResult>
adjustLevelNetwork(const SurveyNetwork& network, const AdjustmentOptions& options = {});

// Writes adjusted positions / elevations back into the network's points.
katana::core::Status applyAdjustment(SurveyNetwork& network,
                                     const HorizontalAdjustmentResult& result);
katana::core::Status applyAdjustment(SurveyNetwork& network, const LevelAdjustmentResult& result);

// A-priori precision used to turn a Traverse into weighted observations.
struct TraverseStochasticModel {
    double distanceSigmaConstant = 0.0; // metres
    double distanceSigmaPpm = 0.0;      // parts per million of the distance
    double angleSigma = 0.0;            // radians, per measured angle
    // Radians. Standard deviation of the known reference azimuths. For Open and
    // Link traverses it may be 0 (errorless reference); a ClosedLoop needs a
    // positive value because its first-leg azimuth enters as an observation (use
    // a small value such as 0.01" to hold it practically fixed).
    double azimuthSigma = 0.0;
};

// Network equivalent of a traverse: stations at their compass-rule coordinates
// (the approximate values), start (and end) held fixed, one distance per leg
// (sigma^2 = constant^2 + (ppm * 1e-6 * d)^2), one angle per interior station.
// An angle turned from a reference azimuth becomes an azimuth observation of the
// adjoining leg with sigma^2 = angleSigma^2 + azimuthSigma^2.
[[nodiscard]] katana::core::Result<SurveyNetwork>
buildTraverseNetwork(const Traverse& traverse, const TraverseStochasticModel& model);

// buildTraverseNetwork + adjustHorizontalNetwork. An open traverse is uniquely
// determined (degrees of freedom 0): it reproduces computeTraverse and reports
// no variance factor.
[[nodiscard]] katana::core::Result<HorizontalAdjustmentResult>
adjustTraverse(const Traverse& traverse, const TraverseStochasticModel& model,
               const AdjustmentOptions& options = {});

} // namespace katana::survey
