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

// Dense least squares costs n u^2: beyond this many unknowns the existing
// solver takes minutes and gigabytes, so the reduction refuses rather than
// appear to hang. Side shots do not count (see above).
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

void rejectPointingRows(Engine& engine, const ReducedPointing& pointing, const std::string& reason)
{
    for (std::size_t i = 0; i < pointing.rawRowCount; ++i) {
        ReportObservation& row = engine.report.observations[pointing.rawRows[i]];
        row.rejected = true;
        row.rejectionReason = reason;
    }
    if (pointing.heightRow) {
        ReportObservation& row = engine.report.observations[*pointing.heightRow];
        row.rejected = true;
        row.rejectionReason = reason;
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
    report.method = method;

    while (true) {
        Result<SurveyNetwork> network = buildNetwork(inputs, excluded, included);
        if (!network) {
            return network.error();
        }
        Result<AdjustmentResult> result = adjust(*network, options);
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
        if (settings.autoRejectOutliers && worst && result->statistics.degreesOfFreedom > 1) {
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
            if (const ReducedPointing* pointing = inputs.observations[observationIndex].pointing) {
                rejectPointingRows(engine, *pointing, reason);
            }
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
    for (const Observation& observation : engine.raw.observations) {
        for (const std::string& id : referencedPoints(observation)) {
            inNetwork.insert(engine.positions.count(id) ? engine.positions.find(id)->first
                                                         : std::string_view{});
        }
    }
    inNetwork.erase(std::string_view{});
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
    for (const std::string_view id : inNetwork) {
        if (present.count(id) == 0 && observedFrom.count(id) != 0) {
            engine.warn("Point " + std::string(id) +
                        " has no approximate position (none of its setups could be oriented), "
                        "so it was left out of the network.");
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
        if (present.count(selection.point.pointId) == 0) {
            continue;
        }
        ControlPoint control = selection.point;
        if (control.northing.constraint != ControlConstraint::Free ||
            control.easting.constraint != ControlConstraint::Free) {
            horizontalInputs.control.push_back(control);
        }
        if (control.elevation.constraint != ControlConstraint::Free) {
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
        if (!state.positioned) {
            continue;
        }
        const SurveyStation& station = stations[s];
        const std::string& at = station.setup.pointId;

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
            if (state.orientationAssumed && state.orientation && horizontal) {
                // The circle was set on a backsight with no position: the set
                // orientation is the only one this setup has.
                horizontalInputs.observations.push_back(NetworkObservation{
                    AzimuthObservation{at, std::string(reference),
                                       normalizeAngle(*referenceDirection + *state.orientation),
                                       referenceSigma, station.source},
                    label(s, "azimuth (circle as set)", {}, reference), station.source, nullptr});
            }
        }

        for (const std::size_t p : state.pointings) {
            const ReducedPointing& pointing = engine.pointings[p];
            if (pointing.rejected || present.count(pointing.target) == 0) {
                continue;
            }
            const SourceRecord source = pointing.source ? *pointing.source : station.source;
            if (horizontal && pointing.direction && referenceDirection &&
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
                    label(s, "angle", reference, pointing.target), source, &pointing});
            }
            if (horizontal && pointing.gridDistance) {
                DistanceObservation distance;
                distance.from = at;
                distance.to = pointing.target;
                distance.distance = *pointing.gridDistance;
                distance.sigma = std::hypot(pointing.sigmaDistance, centring);
                distance.kind = DistanceKind::Horizontal;
                distance.source = source;
                horizontalInputs.observations.push_back(NetworkObservation{
                    distance, label(s, "distance", {}, pointing.target), source, &pointing});
            }
            if (levels && pointing.heightDifference && pointing.slope && pointing.zenith) {
                const double s2 = std::cos(*pointing.zenith) * pointing.sigmaDistance;
                const double z2 = *pointing.slope * std::sin(*pointing.zenith) * pointing.sigmaZenith;
                const double sigma = std::sqrt(s2 * s2 + z2 * z2 +
                                               2.0 * apriori.heightMeasurement *
                                                   apriori.heightMeasurement);
                levelInputs.observations.push_back(NetworkObservation{
                    LevelDifferenceObservation{at, std::string(pointing.target),
                                               *pointing.heightDifference, sigma,
                                               pointing.gridDistance.value_or(0.0), source},
                    label(s, "height difference", {}, pointing.target), source, &pointing});
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
    for (const Observation& observation : engine.raw.observations) {
        const auto points = referencedPoints(observation);
        const bool allPresent = std::all_of(points.begin(), points.end(), [&](const std::string& id) {
            return present.count(id) != 0;
        });
        if (!allPresent) {
            continue;
        }
        const SourceRecord& source = observationSource(observation);
        if (const auto* level = std::get_if<LevelDifferenceObservation>(&observation); level && levels) {
            levelInputs.observations.push_back(NetworkObservation{
                *level, "levelled height difference " + level->from + " -> " + level->to, source,
                nullptr});
        } else if (horizontal && (std::holds_alternative<GnssBaselineObservation>(observation) ||
                                  std::holds_alternative<GnssPositionObservation>(observation) ||
                                  (std::holds_alternative<DistanceObservation>(observation) &&
                                   std::get<DistanceObservation>(observation).kind ==
                                       DistanceKind::Horizontal))) {
            horizontalInputs.observations.push_back(NetworkObservation{
                observation, observationKindName(observation) + " " + points.front() +
                                 (points.size() > 1 ? " -> " + points.back() : std::string{}),
                source, nullptr});
        } else if (const auto* global = std::get_if<GnssGlobalPositionObservation>(&observation);
                   global && horizontal) {
            // Converted in the seeding pass; enters as a grid position.
            const Position* position = engine.find(global->point);
            if (position != nullptr && position->origin == PositionOrigin::Gnss) {
                const double sigma = position->sigmaNorthing.value_or(apriori.gnssHorizontal);
                horizontalInputs.observations.push_back(NetworkObservation{
                    GnssPositionObservation{global->point, position->northing, position->easting,
                                            position->height.value_or(0.0), sigma, sigma,
                                            position->sigmaHeight.value_or(apriori.gnssVertical),
                                            source},
                    "GNSS position " + global->point, source, nullptr});
            }
        }
    }

    // ---- adjust ----
    const double scale = ellipseConfidenceScale(settings.confidenceLevel).valueOr(0.0);
    std::unordered_set<std::string_view> adjustedIds;
    if (horizontal) {
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
    if (levels) {
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
                continue;
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
