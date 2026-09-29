// The adjustments of reduceAndAdjust: a traverse found in the setups, or a
// least-squares network over every observation. Both reuse the existing
// Eigen-based code (traverse.hpp, network_adjustment.hpp); what is here is
// the translation from reduced pointings into their inputs and from their
// results into the report.
//
// Structure is used to keep the least squares small: a SIDE SHOT - a point
// observed from one setup only, occupied by none, held by nothing, and no
// setup's backsight - has no redundancy and cannot move any other point, so
// it is left out of the adjustment and radiated afterwards from the adjusted
// stations. A detail survey of 20 setups and 20 000 shots is then a network
// of 20-odd points, which the dense QR solves in milliseconds; putting the
// side shots in would give the same coordinates at a cost cubic in their
// number.

#include <algorithm>
#include <cmath>
#include <deque>
#include <functional>
#include <string>
#include <unordered_set>
#include <utility>
#include <variant>

#include "katana/math/numerics.hpp"
#include "katana/survey/error_propagation.hpp"
#include "katana/survey/network.hpp"
#include "katana/survey/network_adjustment.hpp"
#include "katana/survey/traverse.hpp"
#include "reduction_engine.hpp"
#include "reduction_formulas.hpp"

namespace katana::survey::detail {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::math::normalizeAngle;
using katana::math::normalizeAngleSigned;

namespace {

// Dense least squares costs n u^2 per iteration, and the existing solver
// (adjustHorizontalNetwork) forms the full cofactor matrix and the thin Q on
// every one. Measured in a Release build (ReductionPerformance, a chain of
// setups with 17 side shots each): 400 unknowns 0.44 s, 1 000 unknowns
// 6.4 s, 2 000 unknowns 63 s, all of it in the solve. Beyond this many the
// reduction refuses rather than appear to hang. Side shots do not count (see
// above).
constexpr std::size_t kMaxNetworkUnknowns = 3000;

bool isKnown(const Position& position)
{
    return position.origin == PositionOrigin::Control ||
           position.origin == PositionOrigin::Entered || position.origin == PositionOrigin::Gnss;
}

double azimuthBetween(const Position& from, const Position& to)
{
    return normalizeAngle(std::atan2(to.easting - from.easting, to.northing - from.northing));
}

// The mean grid distance of a setup's pointings to `target`.
std::optional<double> meanDistance(const Engine& engine, std::size_t setupIndex,
                                   std::string_view target)
{
    double sum = 0.0;
    std::size_t count = 0;
    for (const std::size_t p : engine.setups[setupIndex].pointings) {
        const ReducedPointing& pointing = engine.pointings[p];
        if (!pointing.rejected && pointing.target == target && pointing.gridDistance) {
            sum += *pointing.gridDistance;
            ++count;
        }
    }
    if (count == 0) {
        return std::nullopt;
    }
    return sum / static_cast<double>(count);
}

// Which of a pointing's reduced values a network observation was made from:
// a rejection marks those raw rows and no others (a rejected distance does
// not make the direction of the same pointing any less used).
enum class PointingPart { Direction, Distance, Height };

std::vector<std::size_t> partRows(const ReducedPointing& pointing, PointingPart part)
{
    std::vector<std::size_t> rows;
    if (part == PointingPart::Direction) {
        rows.assign(pointing.directionRows.begin(),
                    pointing.directionRows.begin() +
                        static_cast<std::ptrdiff_t>(pointing.directionRowCount));
    } else if (part == PointingPart::Distance) {
        rows.assign(pointing.distanceRows.begin(),
                    pointing.distanceRows.begin() +
                        static_cast<std::ptrdiff_t>(pointing.distanceRowCount));
    } else if (pointing.heightRow) {
        rows.push_back(*pointing.heightRow);
    }
    return rows;
}

void markRows(Engine& engine, const std::vector<std::size_t>& rows, bool rejected,
              const std::string& reason)
{
    for (const std::size_t index : rows) {
        ReportObservation& row = engine.report.observations[index];
        row.rejected = rejected;
        row.rejectionReason = rejected ? reason : std::string{};
    }
}

// Every point positioned by the first pass that is not held and not a
// station of `stations`: what must follow the stations when they move.
std::unordered_set<std::string_view>
movablePoints(const Engine& engine, const std::unordered_set<std::string_view>& stations)
{
    std::unordered_set<std::string_view> movable;
    for (const auto& [id, position] : engine.positions) {
        if (position.origin == PositionOrigin::Computed && stations.count(id) == 0) {
            movable.insert(id);
        }
    }
    return movable;
}

void reradiate(Engine& engine, const std::unordered_set<std::string_view>& targets)
{
    for (std::size_t s = 0; s < engine.setups.size(); ++s) {
        if (engine.find(engine.raw.stations[s].setup.pointId) != nullptr) {
            orientAndRadiate(engine, s, &targets);
        }
    }
    radiateGnssVectors(engine, &targets);
}

// ---- Traverse ------------------------------------------------------------------------

struct Chain {
    std::vector<std::size_t> setups;
    std::string_view end; // empty: open
    std::optional<TraverseClosingAngle> closing;
};

std::optional<Chain> findTraverse(Engine& engine)
{
    const auto& stations = engine.raw.stations;
    const auto knownPoint = [&engine](std::string_view id) {
        const Position* position = engine.find(id);
        return position != nullptr && isKnown(*position);
    };
    const auto observes = [&engine](std::size_t setup, std::string_view target) {
        return meanDirection(engine, setup, target).has_value() &&
               meanDistance(engine, setup, target).has_value();
    };

    for (std::size_t start = 0; start < stations.size(); ++start) {
        const SurveyStation& first = stations[start];
        if (!knownPoint(first.setup.pointId) || first.backsightPointId.empty() ||
            !knownPoint(first.backsightPointId) ||
            !meanDirection(engine, start, first.backsightPointId)) {
            continue;
        }
        Chain chain;
        chain.setups.push_back(start);
        std::vector<bool> used(stations.size(), false);
        used[start] = true;
        while (true) {
            const std::size_t current = chain.setups.back();
            const SurveyStation& here = stations[current];
            std::optional<std::size_t> next;
            for (std::size_t j = 0; j < stations.size() && !next; ++j) {
                const SurveyStation& candidate = stations[j];
                if (used[j] || candidate.backsightPointId != here.setup.pointId ||
                    candidate.setup.pointId == here.backsightPointId ||
                    knownPoint(candidate.setup.pointId) ||
                    !observes(current, candidate.setup.pointId) ||
                    !meanDirection(engine, j, here.setup.pointId)) {
                    continue;
                }
                next = j;
            }
            if (!next) {
                break;
            }
            used[*next] = true;
            chain.setups.push_back(*next);
        }

        // The end: a known point the last setup observed forward.
        const std::size_t last = chain.setups.back();
        const SurveyStation& lastStation = stations[last];
        for (const std::size_t p : engine.setups[last].pointings) {
            const ReducedPointing& pointing = engine.pointings[p];
            if (pointing.rejected || pointing.target == lastStation.backsightPointId ||
                !knownPoint(pointing.target) || !observes(last, pointing.target)) {
                continue;
            }
            chain.end = pointing.target;
            break;
        }
        if (chain.end.empty() && chain.setups.size() < 2) {
            continue; // one setup and no known end: not a traverse
        }
        if (!chain.end.empty()) {
            // Closing angle: at the end, from the last station to a known mark.
            const Position& endPosition = *engine.find(chain.end);
            if (chain.end == first.setup.pointId) {
                const auto toLast = meanDirection(engine, start, lastStation.setup.pointId);
                const auto toBack = meanDirection(engine, start, first.backsightPointId);
                if (toLast && toBack) {
                    chain.closing = TraverseClosingAngle{
                        normalizeAngle(*toBack - *toLast),
                        azimuthBetween(endPosition, *engine.find(first.backsightPointId))};
                }
            } else {
                for (std::size_t k = 0; k < stations.size() && !chain.closing; ++k) {
                    if (used[k] || stations[k].setup.pointId != chain.end) {
                        continue;
                    }
                    const auto toLast = meanDirection(engine, k, lastStation.setup.pointId);
                    if (!toLast) {
                        continue;
                    }
                    for (const std::size_t p : engine.setups[k].pointings) {
                        const ReducedPointing& pointing = engine.pointings[p];
                        if (pointing.rejected || !pointing.direction ||
                            pointing.target == lastStation.setup.pointId ||
                            !knownPoint(pointing.target)) {
                            continue;
                        }
                        const auto toMark = meanDirection(engine, k, pointing.target);
                        chain.closing = TraverseClosingAngle{
                            normalizeAngle(*toMark - *toLast),
                            azimuthBetween(endPosition, *engine.find(pointing.target))};
                        break;
                    }
                }
            }
        }
        return chain;
    }
    return std::nullopt;
}

} // namespace

Status adjustAsTraverse(Engine& engine)
{
    const ReductionSettings& settings = engine.settings;
    const auto& stations = engine.raw.stations;
    const std::optional<Chain> found = findTraverse(engine);
    if (!found) {
        engine.warn("Traverse adjustment chosen, but no traverse was found: it needs a first setup "
                    "on a known point observing a known backsight, and each next setup "
                    "backsighting the one before. The coordinates are by radiation.");
        return {};
    }
    const Chain& chain = *found;

    Traverse traverse;
    traverse.name = "traverse from " + stations[chain.setups.front()].setup.pointId;
    const Position& startPosition = *engine.find(stations[chain.setups.front()].setup.pointId);
    const Position& backPosition = *engine.find(stations[chain.setups.front()].backsightPointId);
    traverse.start = Coordinate2{startPosition.northing, startPosition.easting};
    traverse.startAzimuth = azimuthBetween(startPosition, backPosition);

    // Legs: the occupied stations in order, each with the angle back -> forward
    // and the distance forward, meaned with the distance measured back from
    // the next setup where there is one.
    std::vector<std::string_view> forwards;
    for (std::size_t i = 0; i < chain.setups.size(); ++i) {
        const std::size_t s = chain.setups[i];
        const SurveyStation& station = stations[s];
        std::string_view forward;
        if (i + 1 < chain.setups.size()) {
            forward = stations[chain.setups[i + 1]].setup.pointId;
        } else if (!chain.end.empty()) {
            forward = chain.end;
        } else {
            break; // an open traverse ends at the last occupied station
        }
        const auto back = meanDirection(engine, s, station.backsightPointId);
        const auto ahead = meanDirection(engine, s, forward);
        auto distance = meanDistance(engine, s, forward);
        if (!back || !ahead || !distance) {
            return makeError(ErrorCode::AdjustmentFailure,
                             "The traverse leg " + station.setup.pointId + " to " +
                                 std::string(forward) +
                                 " lacks a direction or a distance, so the traverse cannot be "
                                 "computed.");
        }
        if (i + 1 < chain.setups.size()) {
            if (const auto returned =
                    meanDistance(engine, chain.setups[i + 1], station.setup.pointId)) {
                *distance = 0.5 * (*distance + *returned);
            }
        }
        traverse.setups.push_back(
            TraverseSetup{station.setup.pointId, normalizeAngle(*ahead - *back), *distance});
        forwards.push_back(forward);
    }
    if (chain.end.empty()) {
        traverse.kind = TraverseKind::Open;
        traverse.endStationId = stations[chain.setups.back()].setup.pointId;
        engine.warn("The traverse from " + stations[chain.setups.front()].setup.pointId +
                    " does not close on a known point: it has no misclosure and nothing to "
                    "adjust.");
    } else {
        traverse.kind = TraverseKind::Link;
        traverse.endStationId = chain.end;
        const Position& endPosition = *engine.find(chain.end);
        traverse.end = Coordinate2{endPosition.northing, endPosition.easting};
        traverse.closingAngle = chain.closing;
    }

    ComputationMethod method = ComputationMethod::TraverseBowditch;
    AdjustmentReport adjustment;
    MisclosureReport misclosure;
    misclosure.name = traverse.name;
    std::vector<std::pair<std::string, Coordinate2>> adjusted;
    std::vector<std::pair<std::string, AdjustedStation>> withPrecision;

    const Result<TraverseResult> computed = computeTraverse(
        traverse, TraverseOptions{true, settings.traverseRule == TraverseRule::Transit
                                            ? TraverseAdjustment::Transit
                                            : TraverseAdjustment::Compass});
    if (!computed) {
        return computed.error();
    }
    misclosure.angular = computed->angularMisclosure;
    misclosure.length = computed->totalLength;
    if (computed->linearMisclosure) {
        misclosure.northing = computed->linearMisclosure->latitude;
        misclosure.easting = computed->linearMisclosure->departure;
        misclosure.linear = computed->linearMisclosure->length;
        misclosure.precisionRatio = computed->linearMisclosure->precisionDenominator;
    }
    if (computed->angularMisclosure) {
        // Three standard deviations of the sum of the angles: each angle is
        // the difference of two face-meaned directions.
        const double angleSigma = settings.apriori.direction;
        misclosure.angularAllowable =
            3.0 * angleSigma * std::sqrt(static_cast<double>(computed->angleCount));
        misclosure.withinTolerance =
            std::abs(*computed->angularMisclosure) <= *misclosure.angularAllowable;
    }
    // Height misclosure: the trigonometric heights carried along the legs
    // against the known height of the end.
    {
        const std::string_view endId = chain.end;
        const Position* endPosition = endId.empty() ? nullptr : engine.find(endId);
        if (endPosition != nullptr && endPosition->height && startPosition.height) {
            double height = *startPosition.height;
            bool complete = true;
            for (std::size_t i = 0; i < traverse.setups.size() && complete; ++i) {
                const std::size_t s = chain.setups[i];
                double sum = 0.0;
                std::size_t count = 0;
                for (const std::size_t p : engine.setups[s].pointings) {
                    const ReducedPointing& pointing = engine.pointings[p];
                    if (!pointing.rejected && pointing.target == forwards[i] &&
                        pointing.heightDifference) {
                        sum += *pointing.heightDifference;
                        ++count;
                    }
                }
                complete = count > 0;
                height += count > 0 ? sum / static_cast<double>(count) : 0.0;
            }
            if (complete) {
                misclosure.height = height - *endPosition->height;
            }
        }
    }

    if (settings.traverseRule == TraverseRule::LeastSquares) {
        method = ComputationMethod::TraverseLeastSquares;
        TraverseStochasticModel model;
        model.distanceSigmaConstant =
            std::hypot(settings.apriori.distanceConstant, settings.apriori.instrumentCentring,
                       settings.apriori.targetCentring);
        model.distanceSigmaPpm = settings.apriori.distancePpm;
        model.angleSigma = settings.apriori.direction; // two face-meaned directions
        AdjustmentOptions options;
        options.maxIterations = settings.maxIterations;
        options.significanceLevel = 1.0 - settings.confidenceLevel;
        const Result<HorizontalAdjustmentResult> result = adjustTraverse(traverse, model, options);
        if (!result) {
            return result.error();
        }
        adjustment.method = "traverse, least squares";
        adjustment.unknowns = result->statistics.unknownCount;
        adjustment.observations = result->statistics.observationCount;
        adjustment.redundancy = result->statistics.degreesOfFreedom;
        adjustment.varianceFactor = result->statistics.varianceFactor;
        adjustment.iterations = result->iterations;
        if (result->statistics.globalTest) {
            const GlobalTest& test = *result->statistics.globalTest;
            adjustment.globalTest = ReportGlobalTest{test.statistic, test.lowerCritical,
                                                     test.upperCritical, test.significanceLevel,
                                                     test.passed};
        }
        const double scale = ellipseConfidenceScale(settings.confidenceLevel).valueOr(0.0);
        for (const AdjustedStation& station : result->stations) {
            adjusted.push_back({station.pointId, station.position});
            withPrecision.push_back({station.pointId, station});
            if (station.sigmaNorthing > 0.0 || station.sigmaEasting > 0.0) {
                adjustment.ellipses.push_back(ReportEllipse{station.pointId, station.ellipse, scale});
            }
        }
    } else {
        method = settings.traverseRule == TraverseRule::Transit ? ComputationMethod::TraverseTransit
                                                                : ComputationMethod::TraverseBowditch;
        adjustment.method = settings.traverseRule == TraverseRule::Transit
                                ? "traverse, Transit rule"
                                : "traverse, Bowditch (compass) rule";
        const std::size_t legs = traverse.setups.size();
        adjustment.unknowns = 2 * (legs - (traverse.kind == TraverseKind::Link ? 1 : 0));
        adjustment.observations = 2 * legs + (traverse.closingAngle ? 1 : 0);
        adjustment.redundancy = traverse.kind == TraverseKind::Link
                                    ? (traverse.closingAngle ? 3U : 2U)
                                    : 0U;
        for (const TraverseStationResult& station : computed->stations) {
            adjusted.push_back({station.id, station.adjusted});
        }
    }
    engine.report.misclosures.push_back(std::move(misclosure));

    // Move the traverse stations, then everything radiated from them.
    std::unordered_set<std::string_view> stationIds;
    for (const auto& [id, position] : adjusted) {
        Position* existing = engine.find(id);
        if (existing == nullptr || isKnown(*existing)) {
            continue; // held ends
        }
        existing->northing = position.northing;
        existing->easting = position.easting;
        existing->method = method;
        for (const auto& [precisionId, station] : withPrecision) {
            if (precisionId == id) {
                existing->sigmaNorthing = station.sigmaNorthing;
                existing->sigmaEasting = station.sigmaEasting;
            }
        }
    }
    for (const std::size_t s : chain.setups) {
        stationIds.insert(stations[s].setup.pointId);
    }
    reradiate(engine, movablePoints(engine, stationIds));
    engine.report.adjustments.push_back(std::move(adjustment));
    return {};
}

// ---- Network ---------------------------------------------------------------------------

namespace {

// One observation of the network, with where it came from.
struct NetworkObservation {
    Observation observation;
    std::string label;
    SourceRecord source;
    const ReducedPointing* pointing = nullptr;
    PointingPart part = PointingPart::Direction;
    // For an observation that is not part of a pointing (a GNSS vector): the
    // report row a rejection marks.
    std::optional<std::size_t> row{};
};

struct OutlierVerdict {
    std::optional<double> statistic;
    bool flagged = false;
};

// w or tau for each residual, and whether it exceeds the critical value.
template <typename AdjustmentResult>
std::vector<OutlierVerdict> testResiduals(const ReductionSettings& settings,
                                          const AdjustmentResult& result, std::string& critical)
{
    std::vector<OutlierVerdict> verdicts(result.residuals.size());
    if (settings.outlierTest == OutlierTest::None) {
        return verdicts;
    }
    const std::size_t redundancy = result.statistics.degreesOfFreedom;
    std::optional<double> limit;
    double scale = 1.0;
    if (settings.outlierTest == OutlierTest::Baarda) {
        limit = baardaCritical(settings.outlierSignificance);
        critical = "Baarda w, critical " + formatNumber(*limit, 2);
    } else if (result.statistics.varianceFactor && *result.statistics.varianceFactor > 0.0) {
        limit = tauCritical(settings.outlierSignificance, redundancy);
        scale = 1.0 / std::sqrt(*result.statistics.varianceFactor);
        if (limit) {
            critical = "tau, critical " + formatNumber(*limit, 2);
        }
    }
    for (std::size_t i = 0; i < result.residuals.size(); ++i) {
        const ResidualRecord& residual = result.residuals[i];
        if (!residual.standardizedResidual) {
            continue;
        }
        verdicts[i].statistic = *residual.standardizedResidual * scale;
        verdicts[i].flagged = limit && std::abs(*verdicts[i].statistic) > *limit;
    }
    return verdicts;
}

std::string residualLabel(const std::vector<NetworkObservation>& observations,
                          const std::vector<std::size_t>& included, const ResidualRecord& residual,
                          const SurveyNetwork& network, SourceRecord& source)
{
    if (residual.source == ResidualSource::Control) {
        const char* component = residual.component == ResidualComponent::Northing ? "northing"
                                : residual.component == ResidualComponent::Easting ? "easting"
                                                                                   : "height";
        return std::string("control ") + component + " " +
               network.controlPoints()[residual.index].pointId;
    }
    const NetworkObservation& observation = observations[included[residual.index]];
    source = observation.source;
    if (residual.component == ResidualComponent::Northing) {
        return observation.label + " (north)";
    }
    if (residual.component == ResidualComponent::Easting) {
        return observation.label + " (east)";
    }
    return observation.label;
}

struct NetworkInputs {
    std::vector<SurveyPoint> points;
    std::vector<ControlPoint> control;
    std::vector<NetworkObservation> observations;
};

Result<SurveyNetwork> buildNetwork(const NetworkInputs& inputs, const std::vector<bool>& excluded,
                                   std::vector<std::size_t>& included)
{
    SurveyNetwork network;
    for (const SurveyPoint& point : inputs.points) {
        if (Status status = network.addPoint(point); !status) {
            return status.error();
        }
    }
    for (const ControlPoint& control : inputs.control) {
        if (Status status = network.addControlPoint(control); !status) {
            return status.error();
        }
    }
    included.clear();
    for (std::size_t i = 0; i < inputs.observations.size(); ++i) {
        if (excluded[i]) {
            continue;
        }
        const auto added = network.addObservation(inputs.observations[i].observation);
        if (!added) {
            return added.error();
        }
        included.push_back(i);
    }
    return network;
}

// Runs one adjustment with the outlier test and, when the settings ask, the
// rejection loop; fills `report`. `adjust` is adjustHorizontalNetwork or
// adjustLevelNetwork.
template <typename AdjustmentResult>
Result<AdjustmentResult>
adjustWithOutliers(Engine& engine, const NetworkInputs& inputs, const std::string& method,
                   const std::function<Result<AdjustmentResult>(const SurveyNetwork&,
                                                                const AdjustmentOptions&)>& adjust,
                   AdjustmentReport& report)
{
    const ReductionSettings& settings = engine.settings;
    AdjustmentOptions options;
    options.maxIterations = settings.maxIterations;
    options.significanceLevel = 1.0 - settings.confidenceLevel;
    std::vector<bool> excluded(inputs.observations.size(), false);
    std::vector<ReportResidual> rejectedResiduals;
    std::vector<std::size_t> included;
    // The last rejection, so that one which leaves the network unsolvable (the
    // only distance to a point) can be put back.
    std::optional<std::size_t> lastRejected;
    std::vector<std::size_t> lastRows;
    bool stopRejecting = false;
    report.method = method;

    while (true) {
        Result<SurveyNetwork> network = buildNetwork(inputs, excluded, included);
        if (!network) {
            return network.error();
        }
        Result<AdjustmentResult> result = adjust(*network, options);
        if (!result && lastRejected) {
            engine.warn(report.rejectedOutliers.back() +
                        " was put back: without it the network cannot be adjusted (" +
                        result.error().message + ").");
            excluded[*lastRejected] = false;
            markRows(engine, lastRows, false, {});
            report.rejectedOutliers.pop_back();
            rejectedResiduals.pop_back();
            lastRejected.reset();
            stopRejecting = true;
            continue;
        }
        if (!result) {
            return result.error();
        }
        std::string critical;
        const std::vector<OutlierVerdict> verdicts = testResiduals(settings, *result, critical);

        std::optional<std::size_t> worst;
        for (std::size_t i = 0; i < verdicts.size(); ++i) {
            if (!verdicts[i].flagged || result->residuals[i].source != ResidualSource::Observed) {
                continue;
            }
            if (!worst || std::abs(*verdicts[i].statistic) > std::abs(*verdicts[*worst].statistic)) {
                worst = i;
            }
        }
        if (settings.autoRejectOutliers && !stopRejecting && worst &&
            result->statistics.degreesOfFreedom > 1) {
            const ResidualRecord& residual = result->residuals[*worst];
            const std::size_t observationIndex = included[residual.index];
            SourceRecord source;
            ReportResidual rejected;
            rejected.observation =
                residualLabel(inputs.observations, included, residual, *network, source);
            rejected.source = source;
            rejected.angular = std::holds_alternative<HorizontalAngleObservation>(
                                   inputs.observations[observationIndex].observation) ||
                               std::holds_alternative<AzimuthObservation>(
                                   inputs.observations[observationIndex].observation);
            rejected.residual = residual.residual;
            rejected.sigma = residual.sigma;
            rejected.redundancyNumber = residual.redundancyNumber;
            rejected.standardised = verdicts[*worst].statistic;
            rejected.flagged = true;
            rejected.rejected = true;
            const std::string reason = "rejected by the " + critical + ", value " +
                                       formatNumber(*verdicts[*worst].statistic, 2);
            report.rejectedOutliers.push_back(rejected.observation);
            lastRows.clear();
            if (const ReducedPointing* pointing = inputs.observations[observationIndex].pointing) {
                lastRows = partRows(*pointing, inputs.observations[observationIndex].part);
            } else if (const auto rowIndex = inputs.observations[observationIndex].row) {
                lastRows = {*rowIndex};
            }
            markRows(engine, lastRows, true, reason);
            lastRejected = observationIndex;
            engine.warn(rejected.observation + " " + reason + "; the adjustment was run again "
                                                              "without it.",
                        source);
            rejectedResiduals.push_back(std::move(rejected));
            excluded[observationIndex] = true;
            continue;
        }

        // The final run.
        const auto& statistics = result->statistics;
        report.unknowns = statistics.unknownCount;
        report.observations = statistics.observationCount;
        report.redundancy = statistics.degreesOfFreedom;
        report.varianceFactor = statistics.varianceFactor;
        if (statistics.globalTest) {
            const GlobalTest& test = *statistics.globalTest;
            report.globalTest = ReportGlobalTest{test.statistic, test.lowerCritical,
                                                 test.upperCritical, test.significanceLevel,
                                                 test.passed};
        }
        report.residuals.clear();
        report.residuals.reserve(result->residuals.size() + rejectedResiduals.size());
        for (std::size_t i = 0; i < result->residuals.size(); ++i) {
            const ResidualRecord& residual = result->residuals[i];
            ReportResidual entry;
            entry.observation =
                residualLabel(inputs.observations, included, residual, *network, entry.source);
            if (residual.source == ResidualSource::Observed) {
                const Observation& observation =
                    inputs.observations[included[residual.index]].observation;
                entry.angular = std::holds_alternative<HorizontalAngleObservation>(observation) ||
                                std::holds_alternative<AzimuthObservation>(observation);
            }
            entry.residual = residual.residual;
            entry.sigma = residual.sigma;
            entry.redundancyNumber = residual.redundancyNumber;
            entry.standardised = verdicts[i].statistic;
            entry.flagged = verdicts[i].flagged;
            if (entry.flagged) {
                report.flaggedOutliers.push_back(entry.observation);
                engine.warn(entry.observation + " is flagged by the " + critical + " (value " +
                                formatNumber(*verdicts[i].statistic, 2) +
                                "); it was kept - turn on automatic rejection or remove it to "
                                "see the network without it.",
                            entry.source);
            }
            report.residuals.push_back(std::move(entry));
        }
        for (ReportResidual& rejected : rejectedResiduals) {
            report.residuals.push_back(std::move(rejected));
        }
        return result;
    }
}

} // namespace

Status adjustAsNetwork(Engine& engine)
{
    const ReductionSettings& settings = engine.settings;
    const auto& stations = engine.raw.stations;
    const bool horizontal = settings.networkDimension != NetworkDimension::Levels;
    const bool levels = settings.networkDimension != NetworkDimension::Horizontal;
    const ObservationPrecision& apriori = settings.apriori;
    const double centring = std::hypot(apriori.instrumentCentring, apriori.targetCentring);

    // ---- which points are in the network ----
    std::unordered_map<std::string_view, std::size_t> observedFrom; // first setup
    std::unordered_set<std::string_view> multiple;
    std::unordered_set<std::string_view> inNetwork;
    for (std::size_t s = 0; s < stations.size(); ++s) {
        inNetwork.insert(stations[s].setup.pointId);
        if (!stations[s].backsightPointId.empty()) {
            inNetwork.insert(stations[s].backsightPointId);
        }
        for (const std::size_t p : engine.setups[s].pointings) {
            const ReducedPointing& pointing = engine.pointings[p];
            if (pointing.rejected) {
                continue;
            }
            const auto [it, inserted] = observedFrom.try_emplace(pointing.target, s);
            if (!inserted && it->second != s) {
                multiple.insert(pointing.target);
            }
        }
    }
    for (const auto& [id, position] : engine.positions) {
        if (isKnown(position)) {
            inNetwork.insert(id);
        }
    }
    for (const std::string_view id : multiple) {
        inNetwork.insert(id);
    }
    for (const RecordedAngle& angle : engine.angles) {
        inNetwork.insert(angle.at);
        inNetwork.insert(angle.to);
        if (!angle.from.empty()) {
            inNetwork.insert(angle.from);
        }
    }
    // Ids of points no position names yet (a levelled point with no
    // horizontal position): owned here, since the network holds views.
    std::deque<std::string> ownedIds;
    const auto viewOf = [&](const std::string& id) -> std::string_view {
        if (const auto it = engine.positions.find(id); it != engine.positions.end()) {
            return it->first;
        }
        if (const auto it = engine.filePoints.find(id); it != engine.filePoints.end()) {
            return it->first;
        }
        ownedIds.push_back(id);
        return ownedIds.back();
    };
    for (const Observation& observation : engine.raw.observations) {
        // GNSS vectors take part through engine.vectors: a rover reached by
        // one vector only is a side shot like a point shot from one setup.
        if (std::holds_alternative<GnssBaselineObservation>(observation) ||
            std::holds_alternative<GnssGeocentricBaselineObservation>(observation)) {
            continue;
        }
        for (const std::string& id : referencedPoints(observation)) {
            inNetwork.insert(viewOf(id));
        }
    }
    for (std::size_t v = 0; v < engine.vectors.size(); ++v) {
        const GridVector& vector = engine.vectors[v];
        if (engine.report.observations[vector.row].rejected) {
            continue;
        }
        inNetwork.insert(vector.from);
        // A vector is a setup of its own for the side-shot rule.
        const auto [it, inserted] = observedFrom.try_emplace(vector.to, stations.size() + v);
        if (!inserted && it->second != stations.size() + v) {
            inNetwork.insert(vector.to);
        }
    }
    std::unordered_set<std::string_view> sideShots;
    for (const auto& [id, setup] : observedFrom) {
        (void)setup;
        if (inNetwork.count(id) == 0) {
            sideShots.insert(id);
        }
    }

    // ---- points and control ----
    NetworkInputs horizontalInputs;
    NetworkInputs levelInputs;
    std::unordered_set<std::string_view> present;
    for (const std::string_view id : engine.positionOrder) {
        if (inNetwork.count(id) == 0) {
            continue;
        }
        const Position& position = engine.positions.at(id);
        SurveyPoint point;
        point.id = id;
        point.northing = position.northing;
        point.easting = position.easting;
        point.elevation = position.height;
        horizontalInputs.points.push_back(point);
        levelInputs.points.push_back(point);
        present.insert(id);
    }
    // The level network needs no horizontal position: a point the horizontal
    // network cannot take still takes part in the heights.
    std::vector<std::string_view> unplaced;
    for (const std::string_view id : inNetwork) {
        if (present.count(id) == 0) {
            unplaced.push_back(id);
        }
    }
    std::sort(unplaced.begin(), unplaced.end());
    std::unordered_set<std::string_view> levelPresent = present;
    for (const std::string_view id : unplaced) {
        SurveyPoint point;
        point.id = id;
        if (const auto it = engine.filePoints.find(id); it != engine.filePoints.end()) {
            point.northing = it->second->northing;
            point.easting = it->second->easting;
            point.elevation = it->second->elevation;
        }
        levelInputs.points.push_back(point);
        levelPresent.insert(id);
        if (horizontal && observedFrom.count(id) != 0) {
            engine.warn("Point " + std::string(id) +
                        " has no approximate position (none of its setups could be oriented), "
                        "so it was left out of the horizontal network.");
        }
    }
    if (horizontalInputs.points.size() * 2 > kMaxNetworkUnknowns) {
        return makeError(ErrorCode::AdjustmentFailure,
                         "The network has " + std::to_string(horizontalInputs.points.size()) +
                             " points besides its side shots; this build adjusts at most " +
                             std::to_string(kMaxNetworkUnknowns / 2) +
                             " at once. Adjust the control network on its own, or use radiation "
                             "or a traverse.");
    }
    for (const ControlSelection& selection : settings.control) {
        ControlPoint control = selection.point;
        if (present.count(selection.point.pointId) != 0 &&
            (control.northing.constraint != ControlConstraint::Free ||
             control.easting.constraint != ControlConstraint::Free)) {
            horizontalInputs.control.push_back(control);
        }
        if (levelPresent.count(selection.point.pointId) != 0 &&
            control.elevation.constraint != ControlConstraint::Free) {
            levelInputs.control.push_back(control);
        }
    }

    // ---- observations ----
    const auto label = [&stations](std::size_t s, const char* what, std::string_view from,
                                   std::string_view to) {
        std::string text = std::string(what) + " at " + stations[s].setup.id + ": ";
        if (!from.empty()) {
            text += std::string(from) + " -> ";
        }
        text += to;
        return text;
    };
    for (std::size_t s = 0; s < stations.size(); ++s) {
        const SetupState& state = engine.setups[s];
        const SurveyStation& station = stations[s];
        const std::string& at = station.setup.pointId;
        const bool horizontalHere = horizontal && state.positioned && present.count(at) != 0;

        // The reference direction: the backsight, else the first network target.
        std::string_view reference;
        if (!station.backsightPointId.empty() && present.count(station.backsightPointId) &&
            meanDirection(engine, s, station.backsightPointId)) {
            reference = station.backsightPointId;
        } else {
            for (const std::size_t p : state.pointings) {
                const ReducedPointing& pointing = engine.pointings[p];
                if (!pointing.rejected && pointing.direction && present.count(pointing.target)) {
                    reference = pointing.target;
                    break;
                }
            }
        }
        std::optional<double> referenceDirection;
        double referenceSigma = 0.0;
        if (!reference.empty()) {
            referenceDirection = meanDirection(engine, s, reference);
            std::size_t count = 0;
            double sigma = 0.0;
            for (const std::size_t p : state.pointings) {
                const ReducedPointing& pointing = engine.pointings[p];
                if (!pointing.rejected && pointing.target == reference && pointing.direction) {
                    sigma = pointing.sigmaDirection;
                    ++count;
                }
            }
            const double distance = meanDistance(engine, s, reference).value_or(0.0);
            referenceSigma = sigma / std::sqrt(static_cast<double>(std::max<std::size_t>(count, 1)));
            if (distance > 0.0) {
                referenceSigma = std::hypot(referenceSigma, centring / distance);
            }
            if (state.orientationAssumed && state.orientation && horizontalHere) {
                // The circle was set, or the file states an azimuth, on a
                // backsight with no position: that orientation is the only
                // one this setup has.
                horizontalInputs.observations.push_back(NetworkObservation{
                    AzimuthObservation{at, std::string(reference),
                                       normalizeAngle(*referenceDirection + *state.orientation),
                                       referenceSigma, station.source},
                    label(s,
                          state.orientationStated ? "azimuth (as the file states)"
                                                  : "azimuth (circle as set)",
                          {}, reference),
                    station.source, nullptr});
            }
        }

        for (const std::size_t p : state.pointings) {
            const ReducedPointing& pointing = engine.pointings[p];
            if (pointing.rejected) {
                continue;
            }
            const bool horizontalTarget = horizontalHere && present.count(pointing.target) != 0;
            const SourceRecord source = pointing.source ? *pointing.source : station.source;
            if (horizontalTarget && pointing.direction && referenceDirection &&
                pointing.target != reference) {
                const double distance = pointing.gridDistance.value_or(0.0);
                double sigma = std::hypot(pointing.sigmaDirection, referenceSigma);
                if (distance > 0.0) {
                    sigma = std::hypot(sigma, centring / distance);
                }
                horizontalInputs.observations.push_back(NetworkObservation{
                    HorizontalAngleObservation{at, std::string(reference),
                                               std::string(pointing.target),
                                               normalizeAngle(*pointing.direction -
                                                              *referenceDirection),
                                               sigma, source},
                    label(s, "angle", reference, pointing.target), source, &pointing,
                    PointingPart::Direction});
            }
            if (horizontalTarget && pointing.gridDistance) {
                DistanceObservation distance;
                distance.from = at;
                distance.to = pointing.target;
                distance.distance = *pointing.gridDistance;
                distance.sigma = std::hypot(pointing.sigmaDistance, centring);
                distance.kind = DistanceKind::Horizontal;
                distance.source = source;
                horizontalInputs.observations.push_back(NetworkObservation{
                    distance, label(s, "distance", {}, pointing.target), source, &pointing,
                    PointingPart::Distance});
            }
            if (levels && pointing.heightDifference && pointing.slope && pointing.zenith &&
                levelPresent.count(at) != 0 && levelPresent.count(pointing.target) != 0) {
                const double s2 = std::cos(*pointing.zenith) * pointing.sigmaDistance;
                const double z2 = *pointing.slope * std::sin(*pointing.zenith) * pointing.sigmaZenith;
                const double sigma = std::sqrt(s2 * s2 + z2 * z2 +
                                               2.0 * apriori.heightMeasurement *
                                                   apriori.heightMeasurement);
                levelInputs.observations.push_back(NetworkObservation{
                    LevelDifferenceObservation{at, std::string(pointing.target),
                                               *pointing.heightDifference, sigma,
                                               pointing.gridDistance.value_or(0.0), source},
                    label(s, "height difference", {}, pointing.target), source, &pointing,
                    PointingPart::Height});
            }
        }
    }
    for (const RecordedAngle& angle : engine.angles) {
        if (!horizontal || present.count(angle.at) == 0 || present.count(angle.to) == 0 ||
            (!angle.from.empty() && present.count(angle.from) == 0)) {
            continue;
        }
        const SourceRecord& source = engine.report.observations[angle.row].source;
        if (angle.from.empty()) {
            horizontalInputs.observations.push_back(NetworkObservation{
                AzimuthObservation{std::string(angle.at), std::string(angle.to), angle.value,
                                   angle.sigma, source},
                "azimuth " + std::string(angle.at) + " -> " + std::string(angle.to), source,
                nullptr});
        } else {
            horizontalInputs.observations.push_back(NetworkObservation{
                HorizontalAngleObservation{std::string(angle.at), std::string(angle.from),
                                           std::string(angle.to), angle.value, angle.sigma,
                                           source},
                "angle at " + std::string(angle.at) + ": " + std::string(angle.from) + " -> " +
                    std::string(angle.to),
                source, nullptr});
        }
    }
    for (std::size_t i = 0; i < engine.raw.observations.size(); ++i) {
        const Observation& observation = engine.raw.observations[i];
        const auto points = referencedPoints(observation);
        const bool isLevel = std::holds_alternative<LevelDifferenceObservation>(observation);
        const auto& among = isLevel ? levelPresent : present;
        const bool allPresent = std::all_of(points.begin(), points.end(), [&](const std::string& id) {
            return among.count(id) != 0;
        });
        if (!allPresent) {
            continue;
        }
        const SourceRecord& source = observationSource(observation);
        if (const auto* level = std::get_if<LevelDifferenceObservation>(&observation); level && levels) {
            levelInputs.observations.push_back(NetworkObservation{
                *level, "levelled height difference " + level->from + " -> " + level->to, source,
                nullptr});
        } else if (horizontal && (std::holds_alternative<GnssPositionObservation>(observation) ||
                                  (std::holds_alternative<DistanceObservation>(observation) &&
                                   std::get<DistanceObservation>(observation).kind ==
                                       DistanceKind::Horizontal))) {
            horizontalInputs.observations.push_back(NetworkObservation{
                observation, observationKindName(observation) + " " + points.front() +
                                 (points.size() > 1 ? " -> " + points.back() : std::string{}),
                source, nullptr});
        } else if (const auto* global = std::get_if<GnssGlobalPositionObservation>(&observation);
                   global && horizontal) {
            // Converted in the seeding pass; enters as a grid position, each
            // occupation with its own value and precision. A point that is
            // control or keyed in keeps what it went in with, as in radiation.
            const Position* position = engine.find(global->point);
            const std::optional<GlobalGridPosition>& converted = engine.globalPositions[i];
            if (converted && position != nullptr && position->origin == PositionOrigin::Gnss) {
                NetworkObservation entry{
                    GnssPositionObservation{global->point, converted->northing, converted->easting,
                                            converted->height.value_or(0.0),
                                            converted->sigmaHorizontal, converted->sigmaHorizontal,
                                            converted->sigmaVertical, source},
                    "GNSS position " + global->point +
                        (source.recordNumber != 0
                             ? " (record " + std::to_string(source.recordNumber) + ")"
                             : std::string{}),
                    source, nullptr};
                entry.row = converted->row;
                horizontalInputs.observations.push_back(std::move(entry));
            }
        }
    }

    for (const GridVector& vector : engine.vectors) {
        if (engine.report.observations[vector.row].rejected) {
            continue;
        }
        const SourceRecord source = vector.source ? *vector.source : SourceRecord{};
        const std::string vectorLabel =
            "GNSS vector " + std::string(vector.from) + " -> " + std::string(vector.to);
        if (horizontal && present.count(vector.from) != 0 && present.count(vector.to) != 0) {
            NetworkObservation entry{
                GnssBaselineObservation{std::string(vector.from), std::string(vector.to),
                                        vector.deltaNorthing, vector.deltaEasting,
                                        vector.deltaHeight.value_or(0.0), vector.sigmaNorthing,
                                        vector.sigmaEasting, vector.sigmaHeight, source},
                vectorLabel, source, nullptr};
            entry.row = vector.row;
            horizontalInputs.observations.push_back(std::move(entry));
        }
        if (levels && vector.deltaHeight && levelPresent.count(vector.from) != 0 &&
            levelPresent.count(vector.to) != 0) {
            NetworkObservation entry{
                LevelDifferenceObservation{std::string(vector.from), std::string(vector.to),
                                           *vector.deltaHeight, vector.sigmaHeight,
                                           std::hypot(vector.deltaNorthing, vector.deltaEasting),
                                           source},
                vectorLabel + " (height)", source, nullptr};
            entry.row = vector.heightRow;
            levelInputs.observations.push_back(std::move(entry));
        }
    }

    // ---- adjust ----
    const double scale = ellipseConfidenceScale(settings.confidenceLevel).valueOr(0.0);
    std::unordered_set<std::string_view> adjustedIds;
    bool runHorizontal = horizontal;
    bool runLevels = levels;
    if (horizontal && levels) {
        // Both asked for: a half with nothing to adjust is skipped, and said so.
        if (horizontalInputs.observations.empty() && !levelInputs.observations.empty()) {
            runHorizontal = false;
            engine.warn("No horizontal observations to adjust; only the heights were adjusted.");
        } else if (levelInputs.observations.empty() && !horizontalInputs.observations.empty()) {
            runLevels = false;
            engine.warn("No height differences to adjust; only the horizontal network was "
                        "adjusted.");
        }
    }
    if (runHorizontal) {
        AdjustmentReport report;
        const auto result = adjustWithOutliers<HorizontalAdjustmentResult>(
            engine, horizontalInputs, "network least squares (horizontal)",
            [](const SurveyNetwork& network, const AdjustmentOptions& options) {
                return adjustHorizontalNetwork(network, options);
            },
            report);
        if (!result) {
            return result.error();
        }
        report.iterations = result->iterations;
        for (const AdjustedStation& station : result->stations) {
            Position* position = engine.find(station.pointId);
            if (position == nullptr) {
                continue;
            }
            position->northing = station.position.northing;
            position->easting = station.position.easting;
            position->sigmaNorthing = station.sigmaNorthing;
            position->sigmaEasting = station.sigmaEasting;
            if (!position->heldHorizontal || position->origin != PositionOrigin::Control) {
                position->method = ComputationMethod::NetworkLeastSquares;
            }
            if (station.sigmaNorthing > 0.0 || station.sigmaEasting > 0.0) {
                report.ellipses.push_back(ReportEllipse{station.pointId, station.ellipse, scale});
            }
            adjustedIds.insert(engine.positions.find(station.pointId)->first);
        }
        engine.report.adjustments.push_back(std::move(report));
    }
    if (runLevels) {
        AdjustmentReport report;
        const auto result = adjustWithOutliers<LevelAdjustmentResult>(
            engine, levelInputs, "network least squares (levels)",
            [](const SurveyNetwork& network, const AdjustmentOptions& options) {
                return adjustLevelNetwork(network, options);
            },
            report);
        if (!result) {
            return result.error();
        }
        report.iterations = 1;
        for (const AdjustedElevation& elevation : result->elevations) {
            Position* position = engine.find(elevation.pointId);
            if (position == nullptr) {
                const auto file = engine.filePoints.find(elevation.pointId);
                if (file == engine.filePoints.end()) {
                    engine.warn("Point " + elevation.pointId + " was levelled to " +
                                formatNumber(elevation.elevation, 4) +
                                " m but has no horizontal position, so it cannot be drawn.");
                    continue;
                }
                // A levelled height on the file's horizontal position.
                Position levelled;
                levelled.northing = file->second->northing;
                levelled.easting = file->second->easting;
                levelled.origin = PositionOrigin::Computed;
                levelled.method = ComputationMethod::NetworkLeastSquares;
                engine.place(file->first, levelled);
                position = engine.find(file->first);
            }
            position->height = elevation.elevation;
            position->sigmaHeight = elevation.sigma;
            if (position->origin == PositionOrigin::Computed) {
                position->method = ComputationMethod::NetworkLeastSquares;
            }
            adjustedIds.insert(engine.positions.find(elevation.pointId)->first);
        }
        engine.report.adjustments.push_back(std::move(report));
    }

    // ---- side shots from the adjusted stations ----
    if (!sideShots.empty()) {
        reradiate(engine, sideShots);
    }
    return {};
}

} // namespace katana::survey::detail
