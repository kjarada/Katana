// The adjustments of reduceAndAdjust: a traverse found in the setups, or a
// least-squares network over every observation - and, before either, the
// resection of a setup nothing else positions, which is a small network of
// its own. All reuse the existing Eigen-based code (traverse.hpp,
// network_adjustment.hpp); what is here is the translation from reduced
// pointings into their inputs and from their results into the report.
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
#include <array>
#include <cmath>
#include <complex>
#include <deque>
#include <functional>
#include <limits>
#include <string>
#include <string_view>
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
// station of `stations`: what must follow the stations when they move. Not a
// resected station: its resection placed it, from points it holds, and
// radiating it again would put it where one other setup's pointing to it -
// a check - says.
std::unordered_set<std::string_view>
movablePoints(const Engine& engine, const std::unordered_set<std::string_view>& stations)
{
    std::unordered_set<std::string_view> movable;
    for (const auto& [id, position] : engine.positions) {
        if (position.origin == PositionOrigin::Computed && stations.count(id) == 0 &&
            position.method != ComputationMethod::Resection) {
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

// ---- The one weighting of a reduced pointing ---------------------------------------------
//
// The network and the resection weight a pointing from the same a-priori
// precision (ReductionSettings::apriori, already meaned over the faces in
// phase A), with the centring and the instrument and target heights, which
// depend on the line and the setup and so are added here. They differ only
// in which of those errors belong to one pointing:
//
//   Occupied  the network positions the MARK a setup stands on: the
//             instrument's centring over it and its measured height are
//             errors of each pointing, as the target's are.
//   Resected  a resection positions the INSTRUMENT, from its pointings: its
//             centring and height are the same for every pointing, so they
//             move no pointing against another and are no part of any
//             pointing's weight - treating them as independent errors of
//             each would weight a short sight as if the instrument moved
//             between pointings. They are the mark's error against the
//             instrument, added to the station's precision afterwards where
//             there is a mark (resectSetup).
enum class StationModel { Occupied, Resected };

double centringOf(const ObservationPrecision& apriori, StationModel model)
{
    return model == StationModel::Resected
               ? apriori.targetCentring
               : std::hypot(apriori.instrumentCentring, apriori.targetCentring);
}

// An angular value of standard deviation `sigma` across a line of `distance`
// (0 when not known, and then no centring is added): a centring error e moves
// a direction by e / distance.
double withCentring(const ObservationPrecision& apriori, double sigma, double distance,
                    StationModel model = StationModel::Occupied)
{
    return distance > 0.0 ? std::hypot(sigma, centringOf(apriori, model) / distance) : sigma;
}

// A reduced horizontal distance: the EDM's precision and the centring, which
// can move it by its whole amount.
double distanceSigmaOf(const ObservationPrecision& apriori, const ReducedPointing& pointing,
                       StationModel model = StationModel::Occupied)
{
    return std::hypot(pointing.sigmaDistance, centringOf(apriori, model));
}

// A trigonometric height difference, dh = S cos z + HI - HT (curvature and
// refraction are exact enough not to count): sigma^2 = (cos z sigma_S)^2 +
// (S sin z sigma_z)^2 + the heights, each measured to sigma_h - the
// instrument's and the target's for an occupied mark, the target's alone for
// a resected instrument (see StationModel). Needs the pointing's slope
// distance and zenith.
double heightDifferenceSigmaOf(const ObservationPrecision& apriori,
                               const ReducedPointing& pointing,
                               StationModel model = StationModel::Occupied)
{
    const double s2 = std::cos(*pointing.zenith) * pointing.sigmaDistance;
    const double z2 = *pointing.slope * std::sin(*pointing.zenith) * pointing.sigmaZenith;
    const double heights = model == StationModel::Resected ? 1.0 : 2.0;
    return std::sqrt(s2 * s2 + z2 * z2 +
                     heights * apriori.heightMeasurement * apriori.heightMeasurement);
}

// Whether a network observation's residual is an angle (seconds in the
// report) rather than a length.
bool isAngular(const Observation& observation)
{
    return std::holds_alternative<HorizontalAngleObservation>(observation) ||
           std::holds_alternative<HorizontalDirectionObservation>(observation) ||
           std::holds_alternative<AzimuthObservation>(observation);
}

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
// adjustLevelNetwork; `subject` is what the warnings call the adjustment
// ("the network", "the resection").
template <typename AdjustmentResult>
Result<AdjustmentResult>
adjustWithOutliers(Engine& engine, const NetworkInputs& inputs, const std::string& method,
                   const std::function<Result<AdjustmentResult>(const SurveyNetwork&,
                                                                const AdjustmentOptions&)>& adjust,
                   AdjustmentReport& report, std::string_view subject = "the network")
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
            engine.warn(report.rejectedOutliers.back() + " was put back: without it " +
                        std::string(subject) + " cannot be adjusted (" + result.error().message +
                        ").");
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
            rejected.angular = isAngular(inputs.observations[observationIndex].observation);
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
                entry.angular = isAngular(inputs.observations[included[residual.index]].observation);
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
                                "see " + std::string(subject) + " without it.",
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

        // The reference direction: the backsight, else the first network
        // target. What an adjustment before this one rejected - a resection's
        // outlier test - the network takes no more than it did (the report
        // says rejected), and never as the reference, whose error would be in
        // every angle measured from it.
        std::string_view reference;
        if (!station.backsightPointId.empty() && present.count(station.backsightPointId) &&
            meanDirection(engine, s, station.backsightPointId)) {
            reference = station.backsightPointId;
        } else {
            for (const std::size_t p : state.pointings) {
                const ReducedPointing& pointing = engine.pointings[p];
                if (!pointing.rejected && pointing.direction && present.count(pointing.target) &&
                    !directionRejected(engine, pointing)) {
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
                if (!pointing.rejected && pointing.target == reference && pointing.direction &&
                    !directionRejected(engine, pointing)) {
                    sigma = pointing.sigmaDirection;
                    ++count;
                }
            }
            referenceSigma = withCentring(
                apriori, sigma / std::sqrt(static_cast<double>(std::max<std::size_t>(count, 1))),
                meanDistance(engine, s, reference).value_or(0.0));
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
                pointing.target != reference && !directionRejected(engine, pointing)) {
                const double sigma =
                    withCentring(apriori, std::hypot(pointing.sigmaDirection, referenceSigma),
                                 pointing.gridDistance.value_or(0.0));
                horizontalInputs.observations.push_back(NetworkObservation{
                    HorizontalAngleObservation{at, std::string(reference),
                                               std::string(pointing.target),
                                               normalizeAngle(*pointing.direction -
                                                              *referenceDirection),
                                               sigma, source},
                    label(s, "angle", reference, pointing.target), source, &pointing,
                    PointingPart::Direction});
            }
            if (horizontalTarget && pointing.gridDistance && !distanceRejected(engine, pointing)) {
                DistanceObservation distance;
                distance.from = at;
                distance.to = pointing.target;
                distance.distance = *pointing.gridDistance;
                distance.sigma = distanceSigmaOf(apriori, pointing);
                distance.kind = DistanceKind::Horizontal;
                distance.source = source;
                horizontalInputs.observations.push_back(NetworkObservation{
                    distance, label(s, "distance", {}, pointing.target), source, &pointing,
                    PointingPart::Distance});
            }
            if (levels && pointing.heightDifference && pointing.slope && pointing.zenith &&
                levelPresent.count(at) != 0 && levelPresent.count(pointing.target) != 0 &&
                !heightRejected(engine, pointing)) {
                levelInputs.observations.push_back(NetworkObservation{
                    LevelDifferenceObservation{at, std::string(pointing.target),
                                               *pointing.heightDifference,
                                               heightDifferenceSigmaOf(apriori, pointing),
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

// ---- Resection -------------------------------------------------------------------------
//
// A free station: a setup on a point nothing else positions - or a setup that
// names no backsight on a point only another setup's radiation positions -
// computed from its reduced pointings to points already placed, which are held
// as they are. When it is tried, and why then, is placeSetups' (reduction.cpp);
// what it needs, how it is solved and what refuses it, here.
//
// Needed: two placed points each observed with a direction and a horizontal
// distance, or three observed with a direction - the fewest that fix a
// position and an orientation (two directions and two distances are four
// observations of the three unknowns, north, east and the orientation; three
// directions are three). Points closer together than their centring lets the
// pointings tell apart count once. Where the file marks the observations its
// field software resected from (kResectionEndMetadata), only those: the
// pointings after them are checks of the station, as they were in the field.
//
// Solved: by the network adjustment itself (adjustHorizontalNetwork), the
// targets held, each pointing's face-meaned direction an observation of one
// set whose orientation is the third unknown, and each horizontal distance,
// with phase B's factors taken at the approximate station; weighted by the
// reduction's one policy for an instrument that is itself the unknown
// (StationModel::Resected); its last run through adjustWithOutliers, so the
// settings' outlier test flags a residual here as in a network. Then the level
// adjustment of its trigonometric height differences to the targets with
// heights.
//
// Where it starts, and whether the station has one answer. Gauss-Newton finds
// the solution nearest its start, and a resection can have two: two marks with
// distances and a third mark by direction alone give one distance and one
// angle, two circles that meet twice. Started from one construction, it can
// place such a station tens of metres from where the readings were made, with
// a precision that says millimetres, and nothing flagged - both solutions fit.
// So it starts from every point the observations give: where
// two of the station's loci meet - the circle of a distance about its target,
// the arc from which two targets are seen under the angle between their
// readings (the line through them where that angle is 0 or 180 degrees) - and
// the rigid fit of the station's own frame onto the targets with distances.
// Each start is judged by the weighted squares of the residuals it leaves, the
// orientation there the weighted mean of azimuth less reading (which minimises
// them over the orientation). The least squares runs from the best, then from
// each other start beyond the linear reach (below) of the solutions found
// whose squares are within the bound below of the least found; a run that
// fails moves on to the next start. The bound is chi-square with two degrees
// of freedom at the settings' confidence level - the square of the scale the
// station's ellipse is drawn at, 5.99 at 95 % - which is where the likelihood
// puts the edge of the station's confidence region: the positions whose
// weighted squares exceed the least by less than that are the ones its
// observations do not exclude. The ellipse stands in for that region about the
// solution. Where a second solution lies within the bound but outside the best
// one's ellipse, the region has a second part the ellipse does not show: the
// observations do not tell the two apart, and the resection is refused. And a
// start from the best point, not from the first three marks, keeps three marks
// on the danger circle, read first, from refusing a station a fourth one fixes.
//
// Refused, too, where the solution's own geometry does not fix it. Exactly
// degenerate geometry of directions alone - the station on one line with every
// three targets, or on the circle through them - the closed form finds.
// Readings carry their errors, though, and then it seldom is exact: a station
// on the danger circle read to an arc second lands anywhere on the circle. So
// the solution is also judged by its own a-priori precision - the cofactor at
// the settings' weights, whatever the residuals. The least squares takes each
// observation as linear about the solution; over an offset a from it, a
// distance of length D changes by up to a^2 / 2D more than that, and a
// direction by up to a^2 / 2D^2 radians (the second-order terms of hypot and
// atan2). Where the station's ellipse at the settings' confidence level
// reaches past the offset at which that exceeds an observation's own standard
// deviation - sqrt(2 D sigma) for a distance, D sqrt(2 sigma) for a direction -
// the model the least squares solved does not hold over the region the station
// may be in: its observations do not fix it, and it is refused. That criterion
// is the least squares' own and needs no tolerance from outside. A geometry it
// passes that still magnifies the observations' errors more than
// kWeakResectionDilution times is placed, and flagged weak.
//
// A geometric refusal names the shape of a geometry that fixes nothing which
// the station's comes nearest - two marks at one place, the station on one
// line with its marks, or on the circle through three of them - with its
// numbers at the solution: how far apart, within what angle of one line, how
// far from that circle. It says what the geometry is; whether that fixes the
// station is the precision's to say, so no tolerance decides what is near.
//
// Rejected: a resection formula (Tienstra's, Collins') for the answer. It
// takes three directions exactly, so it neither uses a fourth nor a distance,
// nor weights, nor says how well the station is fixed; the least squares does
// all of that and is the adjustment every other coordinate here comes from.
// Rejected too: a tolerance on each degenerate shape - how near the danger
// circle, how narrow the angles, how close two marks - which would need a
// number from outside for each, where the precision judges them all alike; and
// a relative one-position rule (marks closer than their pointings resolve at
// the range count once), which would need a number of standard deviations,
// and would still not see two solutions of marks that are well apart.

namespace {

// A point in the plane: the real part the easting, the imaginary the northing.
using Plane = std::complex<double>;

Plane planeOf(const Position& position)
{
    return {position.easting, position.northing};
}

// "5.0 mm" below a metre, "12.345 m" from there.
std::string lengthWords(double metres)
{
    return metres < 1.0 ? formatMillimetres(metres) : formatNumber(metres, 3) + " m";
}

// "2.1\"" below a degree, "12.3 deg" from there.
std::string angleWords(double radians)
{
    return radians < katana::math::kDegToRad
               ? formatSeconds(radians)
               : formatNumber(radians * katana::math::kRadToDeg, 1) + " deg";
}

// A setup's pointings to one placed point, meaned.
struct ResectionTarget {
    std::string_view id;
    const Position* position = nullptr;
    std::optional<double> direction{}; // mean circle reading
    std::optional<double> distance{};  // mean horizontal distance, on the ground (phase A)
};

// The placed points a setup observes in its resection block, in the order it
// first observes each, and the pointings to them it can use.
struct ResectionData {
    std::vector<ResectionTarget> targets;
    std::vector<std::size_t> pointings;
    // Pointings after the block to one of the targets: checks of the station.
    std::size_t checks = 0;
    // The first target whose pointing or position holds a value that is not finite.
    std::string notFinite;
};

ResectionData resectionData(const Engine& engine, std::size_t setupIndex)
{
    const std::string_view at = engine.raw.stations[setupIndex].setup.pointId;
    const auto finite = [](const std::optional<double>& value) {
        return !value || std::isfinite(*value);
    };
    ResectionData data;
    std::unordered_map<std::string_view, std::size_t> slot;
    // Per target, the readings meaned as meanDirection means them (wrapped
    // about the first) and the distances; over the block's pointings only.
    std::vector<double> firstReadings;
    std::vector<double> readingOffsets;
    std::vector<std::size_t> readingCounts;
    std::vector<double> distanceSums;
    std::vector<std::size_t> distanceCounts;
    std::vector<std::size_t> later;
    for (const std::size_t p : engine.setups[setupIndex].pointings) {
        const ReducedPointing& pointing = engine.pointings[p];
        if (pointing.rejected || pointing.target == at ||
            (!pointing.direction && !pointing.horizontal)) {
            continue;
        }
        const auto found = engine.positions.find(pointing.target);
        if (found == engine.positions.end()) {
            continue;
        }
        if (!inResectionBlock(engine, setupIndex, pointing)) {
            later.push_back(p);
            continue;
        }
        const Position& position = found->second;
        if (data.notFinite.empty() &&
            (!finite(pointing.direction) || !finite(pointing.horizontal) ||
             !finite(pointing.heightDifference) || !std::isfinite(position.northing) ||
             !std::isfinite(position.easting) || !finite(position.height))) {
            data.notFinite = pointing.target;
        }
        const auto [it, inserted] = slot.try_emplace(pointing.target, data.targets.size());
        if (inserted) {
            data.targets.push_back(ResectionTarget{pointing.target, &position, {}, {}});
            firstReadings.push_back(0.0);
            readingOffsets.push_back(0.0);
            readingCounts.push_back(0);
            distanceSums.push_back(0.0);
            distanceCounts.push_back(0);
        }
        const std::size_t k = it->second;
        if (pointing.direction) {
            if (readingCounts[k] == 0) {
                firstReadings[k] = *pointing.direction;
            }
            readingOffsets[k] += normalizeAngleSigned(*pointing.direction - firstReadings[k]);
            ++readingCounts[k];
        }
        if (pointing.horizontal) {
            distanceSums[k] += *pointing.horizontal;
            ++distanceCounts[k];
        }
        data.pointings.push_back(p);
    }
    for (std::size_t k = 0; k < data.targets.size(); ++k) {
        if (readingCounts[k] > 0) {
            data.targets[k].direction = normalizeAngle(
                firstReadings[k] + readingOffsets[k] / static_cast<double>(readingCounts[k]));
        }
        if (distanceCounts[k] > 0) {
            data.targets[k].distance = distanceSums[k] / static_cast<double>(distanceCounts[k]);
        }
    }
    for (const std::size_t p : later) {
        data.checks += slot.count(engine.pointings[p].target);
    }
    return data;
}

// Two marks closer than this are one position to a resection: the standard
// deviation of the difference of two targets, each centred to the settings'
// target centring, which no pointing to them can resolve - and never less than
// the project's coordinate tolerance.
double onePositionOf(const ObservationPrecision& apriori)
{
    return std::max(katana::math::tolerance::kCoordinate,
                    std::sqrt(2.0) * apriori.targetCentring);
}

// The targets at distinct positions: a point within `onePosition` of an
// earlier one adds nothing to the geometry, so it is not counted again.
struct DistinctTargets {
    std::vector<const ResectionTarget*> withBoth;      // a direction and a distance
    std::vector<const ResectionTarget*> withDirection; // a direction, with or without
    std::string coincident; // the first two that share a position, "A and B"
};

DistinctTargets distinctTargets(const std::vector<ResectionTarget>& targets, double onePosition)
{
    DistinctTargets distinct;
    struct Place {
        const ResectionTarget* first = nullptr;
        bool both = false;
        bool direction = false;
    };
    std::vector<Place> places;
    for (const ResectionTarget& target : targets) {
        std::size_t place = places.size();
        for (std::size_t k = 0; k < places.size(); ++k) {
            if (std::abs(planeOf(*places[k].first->position) - planeOf(*target.position)) <=
                onePosition) {
                place = k;
                break;
            }
        }
        if (place == places.size()) {
            places.push_back(Place{&target, false, false});
        } else if (distinct.coincident.empty()) {
            distinct.coincident = std::string(places[place].first->id) + " and " +
                                  std::string(target.id);
        }
        if (target.direction && target.distance && !places[place].both) {
            places[place].both = true;
            distinct.withBoth.push_back(&target);
        }
        if (target.direction && !places[place].direction) {
            places[place].direction = true;
            distinct.withDirection.push_back(&target);
        }
    }
    return distinct;
}

std::string namesOf(const std::vector<const ResectionTarget*>& targets)
{
    std::string names;
    for (std::size_t i = 0; i < targets.size(); ++i) {
        names += i == 0 ? "" : (i + 1 == targets.size() ? " and " : ", ");
        names += targets[i]->id;
    }
    return names;
}

// Too few placed points, as a clause.
std::string tooFew(const DistinctTargets& distinct)
{
    const std::size_t directions = distinct.withDirection.size();
    std::string clause = "it observes ";
    if (directions == 0) {
        clause += "no placed point with a direction";
    } else {
        clause += std::to_string(directions) + " placed point" + (directions == 1 ? "" : "s") +
                  " with a direction (" + namesOf(distinct.withDirection) + "), " +
                  std::to_string(distinct.withBoth.size()) + " of them with a distance as well";
    }
    clause += ", where a resection needs two placed points observed with a direction and a "
              "distance, or three observed with a direction";
    if (!distinct.coincident.empty()) {
        clause += " (" + distinct.coincident + " stand on one position, so count as one)";
    }
    return clause;
}

// The station from targets observed with a direction and a distance: its own
// frame - the circle's zero as north, itself at the origin, each target at its
// reading and distance - turned and moved onto the targets by the rigid fit of
// least squares (a Helmert fit without scale). With local l and world w
// reduced to their centroids (primed), the turn that fits them is
// arg(sum conj(w') l'), a turn of the frame by e^(-i turn), and the station is
// the world centroid less the turned local one. Absent where the frame puts
// the targets at one place.
std::optional<Plane> fitStation(const std::vector<const ResectionTarget*>& targets)
{
    std::vector<Plane> local;
    std::vector<Plane> world;
    Plane localMean{};
    Plane worldMean{};
    for (const ResectionTarget* target : targets) {
        // Reading d from north, clockwise: (D sin d, D cos d) = D e^(i(pi/2 - d)).
        local.push_back(std::polar(*target->distance, katana::math::kHalfPi - *target->direction));
        world.push_back(planeOf(*target->position));
        localMean += local.back();
        worldMean += world.back();
    }
    const double count = static_cast<double>(targets.size());
    localMean /= count;
    worldMean /= count;
    Plane fit{};
    double size = 0.0;
    for (std::size_t i = 0; i < targets.size(); ++i) {
        const Plane l = local[i] - localMean;
        const Plane w = world[i] - worldMean;
        fit += std::conj(w) * l;
        size += std::norm(l) + std::norm(w);
    }
    if (std::abs(fit) <= katana::math::tolerance::kRelative * size) {
        return std::nullopt;
    }
    return worldMean - localMean * std::polar(1.0, -std::arg(fit));
}

// The centre of the circle on which a point sees `from` and `to` under the
// clockwise angle `angle` from the one to the other (the inscribed angle
// theorem): on the chord's perpendicular bisector, (chord / 2) cot(angle) from
// its middle.
Plane inscribedCentre(Plane from, Plane to, double angle)
{
    return 0.5 * (from + to) -
           Plane(0.0, 1.0) * (0.5 * (to - from)) * (std::cos(angle) / std::sin(angle));
}

struct ThreeDirections {
    std::optional<Plane> station{};
    std::string why{};
};

// The station from three targets by direction alone. It is on each circle
// through two of them that sees them under the angle between their readings;
// two such circles share a target and meet again at the station, which is that
// target mirrored in the line through their centres. Refused where two of the
// three angles are exactly 0 or 180 degrees (the station on one line with the
// targets) and where the two circles are exactly one (the station on the
// circle through all three: the danger circle, where any point of it sees them
// alike). Used only to find directions alone exactly that degenerate: the
// least squares' starts come from the loci (resectSetup), and near those
// shapes its precision judges the solution.
ThreeDirections threeDirections(const ResectionTarget& a, const ResectionTarget& b,
                                const ResectionTarget& c)
{
    namespace tol = katana::math::tolerance;
    const std::array<const ResectionTarget*, 3> targets{&a, &b, &c};
    struct Chord {
        std::size_t from = 0;
        std::size_t to = 0;
        double angle = 0.0;
    };
    std::array<Chord, 3> chords{{{0, 1, *b.direction - *a.direction},
                                 {1, 2, *c.direction - *b.direction},
                                 {2, 0, *a.direction - *c.direction}}};
    std::stable_sort(chords.begin(), chords.end(), [](const Chord& x, const Chord& y) {
        return std::abs(std::sin(x.angle)) > std::abs(std::sin(y.angle));
    });
    const std::string names =
        std::string(a.id) + ", " + std::string(b.id) + " and " + std::string(c.id);
    if (std::abs(std::sin(chords[1].angle)) <= tol::kAngular) {
        return {std::nullopt,
                "it stands on one line with " + names + ", where directions cannot fix it"};
    }
    const Chord& first = chords[0];
    const Chord& second = chords[1];
    const std::size_t shared =
        first.from == second.from || first.from == second.to ? first.from : first.to;
    const Plane centre1 = inscribedCentre(planeOf(*targets[first.from]->position),
                                          planeOf(*targets[first.to]->position), first.angle);
    const Plane centre2 = inscribedCentre(planeOf(*targets[second.from]->position),
                                          planeOf(*targets[second.to]->position), second.angle);
    const Plane axis = centre2 - centre1;
    const Plane toShared = planeOf(*targets[shared]->position) - centre1;
    if (std::abs(axis) <= tol::kRelative * std::abs(toShared)) {
        return {std::nullopt, "it stands on the circle through " + names +
                                  " (the danger circle), where directions to them cannot fix it"};
    }
    return {centre1 + axis / std::conj(axis) * std::conj(toShared), {}};
}

// The circle through three points, absent where they are on one line.
std::optional<std::pair<Plane, double>> circleThrough(Plane a, Plane b, Plane c)
{
    // About a, so that grid-sized coordinates keep their digits.
    const Plane u = b - a;
    const Plane v = c - a;
    const double twice = 2.0 * (u.real() * v.imag() - u.imag() * v.real());
    if (!(std::abs(twice) > katana::math::tolerance::kRelative * std::max(std::norm(u), std::norm(v)))) {
        return std::nullopt;
    }
    const Plane centre{(v.imag() * std::norm(u) - u.imag() * std::norm(v)) / twice,
                       (u.real() * std::norm(v) - v.real() * std::norm(u)) / twice};
    return std::pair{a + centre, std::abs(centre)};
}

// Where the station is by one or two of its observations: a circle - of a
// distance about its target, or of the arc from which two targets are seen
// under the angle between their readings (inscribedCentre) - or, where that
// angle is 0 or 180 degrees, the line through the two targets.
struct Locus {
    Plane point{}; // a circle's centre, or a point of the line
    double radius = 0.0;
    Plane along{}; // the line's unit direction; zero for a circle
};

// The cross product of two vectors of the plane (its one component): zero for
// parallel ones.
double cross(Plane a, Plane b)
{
    return a.real() * b.imag() - a.imag() * b.real();
}

// Where two loci meet, added to `points`: none, one or two points. Two that
// miss each other - read with errors, where the true ones touch or nearly do -
// give one point instead, on the line of their centres (the foot of the
// radical line for two circles, of the centre for a circle and a line), which
// for two that nearly touch is where they would: a start near the station,
// which the least squares then solves.
void meet(const Locus& a, const Locus& b, std::vector<Plane>& points)
{
    const bool lineA = a.along != Plane{};
    const bool lineB = b.along != Plane{};
    if (lineA && lineB) {
        const double turn = cross(a.along, b.along);
        if (turn != 0.0) {
            points.push_back(a.point + a.along * (cross(b.point - a.point, b.along) / turn));
        }
        return;
    }
    if (lineA || lineB) {
        const Locus& line = lineA ? a : b;
        const Locus& circle = lineA ? b : a;
        const Plane foot =
            line.point + line.along * (std::conj(line.along) * (circle.point - line.point)).real();
        const double offset = std::abs(circle.point - foot);
        if (offset <= circle.radius) {
            const Plane half =
                line.along * std::sqrt(circle.radius * circle.radius - offset * offset);
            points.push_back(foot + half);
            points.push_back(foot - half);
        } else {
            points.push_back(foot);
        }
        return;
    }
    const Plane between = b.point - a.point;
    const double apart = std::abs(between);
    if (apart == 0.0) {
        return; // one centre: the circles meet everywhere or nowhere
    }
    const Plane unit = between / apart;
    const double along = (a.radius * a.radius - b.radius * b.radius + apart * apart) / (2.0 * apart);
    const double across = a.radius * a.radius - along * along;
    const Plane foot = a.point + unit * along;
    if (across >= 0.0) {
        const Plane half = Plane(0.0, 1.0) * unit * std::sqrt(across);
        points.push_back(foot + half);
        points.push_back(foot - half);
    } else {
        points.push_back(foot);
    }
}

// One horizontal observation of the resection as a start is judged by it, in
// the order its least squares takes them (resectSetup), so that one mask of
// the observations its outlier test rejected serves both.
struct Reading {
    Plane target{};
    bool direction = false;
    double value = 0.0;            // the circle reading, or the grid distance
    double sigma = 0.0;            // a distance's; a direction's before its target's centring
    std::optional<double> sight{}; // a direction's measured sight, over which that centring acts
};

// The weighted squares of the residuals a station at `station` leaves, at the
// orientation there that makes them least - the weighted mean of azimuth less
// reading - with the weights its least squares gives them. The readings
// `excluded` marks take no part.
double squaresAt(Plane station, const std::vector<Reading>& readings,
                 const std::vector<bool>& excluded, const ObservationPrecision& apriori)
{
    std::vector<double> sigmas(readings.size(), 0.0);
    std::optional<double> first;
    double offsets = 0.0;
    double weights = 0.0;
    for (std::size_t i = 0; i < readings.size(); ++i) {
        const Reading& reading = readings[i];
        if (excluded[i] || !reading.direction) {
            continue;
        }
        const Plane line = reading.target - station;
        sigmas[i] = withCentring(apriori, reading.sigma, reading.sight.value_or(std::abs(line)),
                                 StationModel::Resected);
        const double value = normalizeAngle(std::atan2(line.real(), line.imag()) - reading.value);
        if (!first) {
            first = value;
        }
        const double weight = 1.0 / (sigmas[i] * sigmas[i]);
        offsets += weight * normalizeAngleSigned(value - *first);
        weights += weight;
    }
    const double orientation = first ? *first + offsets / weights : 0.0;
    double squares = 0.0;
    for (std::size_t i = 0; i < readings.size(); ++i) {
        const Reading& reading = readings[i];
        if (excluded[i]) {
            continue;
        }
        const Plane line = reading.target - station;
        const double standardised =
            reading.direction
                ? normalizeAngleSigned(std::atan2(line.real(), line.imag()) - reading.value -
                                       orientation) /
                      sigmas[i]
                : (std::abs(line) - reading.value) / reading.sigma;
        squares += standardised * standardised;
    }
    return squares;
}

// The squared Mahalanobis distance of `offset` from a station whose
// coordinates have the cofactor `q`: above chi-square(2) at a confidence
// level, the offset lies outside the station's ellipse at that level.
double mahalanobis2(Plane offset, const Covariance2& q)
{
    const double determinant = q.northing * q.easting - q.northingEasting * q.northingEasting;
    if (!(determinant > 0.0)) {
        return std::numeric_limits<double>::infinity();
    }
    const double north = offset.imag();
    const double east = offset.real();
    return (q.easting * north * north - 2.0 * q.northingEasting * north * east +
            q.northing * east * east) /
           determinant;
}

// A shape of the geometries that fix no station, and how near one comes to it.
struct Shape {
    double measure = std::numeric_limits<double>::infinity();
    std::string clause;
};

// Of the shapes that fix no station, the one a station at `station` comes
// nearest, as a clause with its numbers: two targets at one place (their
// separation over their distance from it), the station on one line with three
// or more targets (the largest sine of an angle between two sight lines, which
// one line through the station makes zero), or on the circle through three of
// them (its distance from that circle over the radius). Each measure is zero
// for its shape exactly; the least is named. What it names is a fact of the
// geometry; the refusal it goes with says why that does not fix the station.
Shape nearestShape(const std::vector<const ResectionTarget*>& targets, Plane station)
{
    Shape nearest;
    const auto consider = [&nearest](double measure, std::string clause) {
        if (std::isfinite(measure) && measure < nearest.measure) {
            nearest = Shape{measure, std::move(clause)};
        }
    };
    const std::size_t count = targets.size();
    for (std::size_t i = 0; i < count; ++i) {
        for (std::size_t j = i + 1; j < count; ++j) {
            const Plane a = planeOf(*targets[i]->position);
            const Plane b = planeOf(*targets[j]->position);
            const double apart = std::abs(a - b);
            // How far they are: as measured, else from the station.
            const double reach = std::max(targets[i]->distance.value_or(std::abs(a - station)),
                                          targets[j]->distance.value_or(std::abs(b - station)));
            if (reach > 0.0) {
                consider(apart / reach, std::string(targets[i]->id) + " and " +
                                            std::string(targets[j]->id) + " are " +
                                            lengthWords(apart) + " apart, " + lengthWords(reach) +
                                            " from it");
            }
        }
    }
    if (count < 3) {
        return nearest;
    }
    double widest = 0.0;
    for (std::size_t i = 0; i < count; ++i) {
        for (std::size_t j = i + 1; j < count; ++j) {
            widest = std::max(widest,
                              std::abs(std::sin(*targets[j]->direction - *targets[i]->direction)));
        }
    }
    consider(widest, "its sight lines to " + namesOf(targets) + " are within " +
                         angleWords(std::asin(std::min(widest, 1.0))) + " of one line");
    for (std::size_t i = 0; i < count; ++i) {
        for (std::size_t j = i + 1; j < count; ++j) {
            for (std::size_t k = j + 1; k < count; ++k) {
                const auto circle = circleThrough(planeOf(*targets[i]->position),
                                                  planeOf(*targets[j]->position),
                                                  planeOf(*targets[k]->position));
                if (!circle) {
                    continue;
                }
                const double offset = std::abs(std::abs(station - circle->first) - circle->second);
                consider(offset / circle->second,
                         "it stands " + lengthWords(offset) + " from the circle through " +
                             namesOf({targets[i], targets[j], targets[k]}) +
                             " (the danger circle, radius " + lengthWords(circle->second) + ")");
            }
        }
    }
    return nearest;
}

// One observation of the resection, as its strength needs it.
struct Sight {
    Plane target;
    double sigma = 0.0; // radians for a direction, metres for a distance
    bool direction = false;
};

// How well the least squares fixed the station, from the geometry and the
// observations' a-priori precision alone (see the section's head).
struct StationStrength {
    Covariance2 cofactor{};    // of the station's northing and easting, at the a-priori weights
    ErrorEllipse apriori{};    // its standard ellipse
    double confidence = 0.0;   // its semi-major axis at the settings' confidence level
    double linearWithin = 0.0; // the offset within which the linear model holds
    double worstLine = 0.0;    // the least precise single observation's line of position

    [[nodiscard]] bool fixes() const
    {
        return std::isfinite(confidence) && confidence <= linearWithin;
    }
    [[nodiscard]] double dilution() const { return apriori.semiMajor / worstLine; }
};

std::optional<StationStrength> stationStrength(const HorizontalAdjustmentResult& result,
                                               std::string_view at, Plane station,
                                               const std::vector<Sight>& sights,
                                               double confidenceScale)
{
    std::optional<std::size_t> north;
    std::optional<std::size_t> east;
    for (std::size_t k = 0; k < result.parameters.size(); ++k) {
        const AdjustedParameter& parameter = result.parameters[k];
        if (parameter.pointId != at) {
            continue;
        }
        if (parameter.component == CoordinateComponent::Northing) {
            north = k;
        } else if (parameter.component == CoordinateComponent::Easting) {
            east = k;
        }
    }
    if (!north || !east || sights.empty()) {
        return std::nullopt;
    }
    const Covariance2 cofactor{result.cofactor(*north, *north), result.cofactor(*east, *east),
                               result.cofactor(*north, *east)};
    const Result<ErrorEllipse> ellipse = errorEllipse(cofactor);
    if (!ellipse) {
        return std::nullopt;
    }
    StationStrength strength;
    strength.cofactor = cofactor;
    strength.apriori = *ellipse;
    strength.confidence = ellipse->semiMajor * confidenceScale;
    strength.linearWithin = std::numeric_limits<double>::infinity();
    for (const Sight& sight : sights) {
        const double length = std::abs(sight.target - station);
        if (sight.direction) {
            strength.linearWithin =
                std::min(strength.linearWithin, length * std::sqrt(2.0 * sight.sigma));
            strength.worstLine = std::max(strength.worstLine, length * sight.sigma);
        } else {
            strength.linearWithin =
                std::min(strength.linearWithin, std::sqrt(2.0 * length * sight.sigma));
            strength.worstLine = std::max(strength.worstLine, sight.sigma);
        }
    }
    return strength;
}

// A solution of the resection's least squares: where it put the station, the
// weighted squares of its residuals, and how well it fixes the station.
struct Solution {
    Plane position{};
    double squares = 0.0;
    StationStrength strength{};
};

// The one of `found` with the least squares.
std::size_t leastOf(const std::vector<Solution>& found)
{
    return static_cast<std::size_t>(
        std::min_element(found.begin(), found.end(),
                         [](const Solution& x, const Solution& y) { return x.squares < y.squares; }) -
        found.begin());
}

// Another solution of `found` its observations do not tell from `best`: its
// squares within `bound` of best's and it outside best's ellipse at the
// confidence level whose chi-square(2) is `bound`. The nearest in squares, or
// none.
const Solution* rivalOf(const std::vector<Solution>& found, const Solution& best, double bound)
{
    const Solution* rival = nullptr;
    for (const Solution& other : found) {
        if (&other == &best || other.squares > best.squares + bound ||
            mahalanobis2(other.position - best.position, best.strength.cofactor) <= bound) {
            continue;
        }
        if (rival == nullptr || other.squares < rival->squares) {
            rival = &other;
        }
    }
    return rival;
}

// "12 deg": the azimuth of an ellipse's semi-major axis, in whole degrees.
std::string axisWords(const ErrorEllipse& ellipse)
{
    return formatNumber(ellipse.orientation * 180.0 / katana::math::kPi, 0) + " deg";
}

// "N 4935.721 E 5076.604": a position, to the millimetre.
std::string positionWords(Plane position)
{
    return "N " + formatNumber(position.imag(), 3) + " E " + formatNumber(position.real(), 3);
}

// The precision a least squares gives, as a factor on its a-priori cofactor:
// the variance factor where its global test finds the residuals larger than
// the a-priori weights allow (above the test's upper bound), else 1. On the
// few degrees of freedom of a resection the variance factor is itself
// uncertain - a chi-square on r has a relative standard deviation of
// sqrt(2 / r), 141 % on one, 82 % on three - so one the test accepts says no
// more than the weights did, and scaling by it would shrink or swell the
// precision by chance (to nanometres where exact readings fit exactly); one
// above the bound says the weights were too optimistic.
double precisionScaleOf(const AdjustmentStatistics& statistics)
{
    return statistics.globalTest && statistics.varianceFactor &&
                   statistics.globalTest->statistic > statistics.globalTest->upperCritical
               ? *statistics.varianceFactor
               : 1.0;
}

} // namespace

bool inResectionBlock(const Engine& engine, std::size_t setupIndex,
                      const ReducedPointing& pointing)
{
    const std::optional<std::size_t>& end = engine.setups[setupIndex].resectionBlockEnd;
    return !end || pointing.source == nullptr || pointing.source->recordNumber == 0 ||
           pointing.source->recordNumber < *end;
}

std::string resectionShortfall(const Engine& engine, std::size_t setupIndex)
{
    const ResectionData data = resectionData(engine, setupIndex);
    if (data.targets.empty()) {
        return {};
    }
    const DistinctTargets distinct =
        distinctTargets(data.targets, onePositionOf(engine.settings.apriori));
    if (distinct.withBoth.size() >= 2 || distinct.withDirection.size() >= 3) {
        return {}; // enough: a refusal would have said why not
    }
    return tooFew(distinct);
}

bool resectSetup(Engine& engine, std::size_t setupIndex, std::string& why)
{
    const ReductionSettings& settings = engine.settings;
    const ObservationPrecision& apriori = settings.apriori;
    const SurveyStation& station = engine.raw.stations[setupIndex];
    SetupState& state = engine.setups[setupIndex];
    const std::string& at = station.setup.pointId;
    // Where another setup's radiation put the station, when placeSetups
    // resects a setup that names no backsight on it: replaced, and kept as a
    // check of the resection.
    const Position* before = engine.find(at);
    const std::optional<Position> radiated =
        before != nullptr && before->method == ComputationMethod::Radiation
            ? std::optional<Position>(*before)
            : std::nullopt;

    const ResectionData data = resectionData(engine, setupIndex);
    if (!data.notFinite.empty()) {
        why = "what it measured to " + data.notFinite +
              ", or where that point is, is not a finite number";
        return false;
    }
    const double onePosition = onePositionOf(apriori);
    const DistinctTargets distinct = distinctTargets(data.targets, onePosition);
    if (distinct.withBoth.size() < 2 && distinct.withDirection.size() < 3) {
        why = tooFew(distinct);
        return false;
    }

    // Directions alone, every three of them exactly on one line with the
    // station or on the circle through it: nothing fixes it, whatever the
    // start, and the closed form says which.
    const bool anyDistance =
        std::any_of(data.targets.begin(), data.targets.end(),
                    [](const ResectionTarget& target) { return target.distance.has_value(); });
    if (!anyDistance) {
        const std::vector<const ResectionTarget*>& t = distinct.withDirection;
        std::string first;
        bool fixes = false;
        for (std::size_t i = 0; i < t.size() && !fixes; ++i) {
            for (std::size_t j = i + 1; j < t.size() && !fixes; ++j) {
                for (std::size_t k = j + 1; k < t.size() && !fixes; ++k) {
                    const ThreeDirections three = threeDirections(*t[i], *t[j], *t[k]);
                    fixes = three.station.has_value();
                    if (!fixes && first.empty()) {
                        first = three.why;
                    }
                }
            }
        }
        if (!fixes) {
            why = first;
            return false;
        }
    }

    // ---- its observations ----
    // The distances' factors are taken at a station not yet known: the rigid
    // fit where there is one, else amid the targets, and the height the
    // targets with heights give it. Only the grid scale and the geoid depend
    // on where, a few parts per million over the few hundred metres a start
    // can be off; the least squares takes them again at each start it runs
    // from.
    const std::optional<Plane> fit =
        distinct.withBoth.size() >= 2 ? fitStation(distinct.withBoth) : std::nullopt;
    Plane reference{};
    if (fit && std::isfinite(fit->real()) && std::isfinite(fit->imag())) {
        reference = *fit;
    } else {
        for (const ResectionTarget* target : distinct.withDirection) {
            reference += planeOf(*target->position);
        }
        reference /= static_cast<double>(distinct.withDirection.size());
    }
    Position approximate;
    approximate.northing = reference.imag();
    approximate.easting = reference.real();
    {
        double sum = 0.0;
        std::size_t count = 0;
        for (const std::size_t p : data.pointings) {
            const ReducedPointing& pointing = engine.pointings[p];
            const Position& target = engine.positions.at(pointing.target);
            if (pointing.heightDifference && target.height) {
                sum += *target.height - *pointing.heightDifference;
                ++count;
            }
        }
        if (count > 0) {
            approximate.height = sum / static_cast<double>(count);
        }
    }
    std::vector<Reading> readings;
    for (const std::size_t p : data.pointings) {
        const ReducedPointing& pointing = engine.pointings[p];
        const Plane target = planeOf(engine.positions.at(pointing.target));
        const std::optional<double> gridDistance =
            gridDistanceFrom(engine, setupIndex, pointing, approximate);
        if (gridDistance && !(std::isfinite(*gridDistance) && *gridDistance > 0.0)) {
            why = "its distance to " + std::string(pointing.target) + " is not a positive number";
            return false;
        }
        if (pointing.direction) {
            readings.push_back(
                Reading{target, true, *pointing.direction, pointing.sigmaDirection, gridDistance});
        }
        if (gridDistance) {
            readings.push_back(Reading{target, false, *gridDistance,
                                       distanceSigmaOf(apriori, pointing, StationModel::Resected),
                                       std::nullopt});
        }
    }

    // The network of its least squares, started at `start`: the station free,
    // its targets held, a direction and a distance per pointing as `readings`
    // has them, with the distances' factors at the start.
    const auto label = [&station](const char* what, std::string_view to) {
        return std::string(what) + " at " + station.setup.id + ": " + std::string(to);
    };
    struct Inputs {
        NetworkInputs network;
        std::vector<Sight> sights;                              // one per observation
        std::vector<std::pair<std::size_t, double>> directions; // pointing, its sigma
    };
    const auto inputsAt = [&](Plane start) {
        Position here = approximate;
        here.northing = start.imag();
        here.easting = start.real();
        Inputs inputs;
        {
            SurveyPoint free;
            free.id = at;
            free.northing = here.northing;
            free.easting = here.easting;
            inputs.network.points.push_back(std::move(free));
        }
        for (const ResectionTarget& target : data.targets) {
            SurveyPoint held;
            held.id = target.id;
            held.northing = target.position->northing;
            held.easting = target.position->easting;
            inputs.network.points.push_back(std::move(held));
            inputs.network.control.push_back(ControlPoint::fixedHorizontal(std::string(target.id)));
        }
        for (const std::size_t p : data.pointings) {
            const ReducedPointing& pointing = engine.pointings[p];
            const Position& target = engine.positions.at(pointing.target);
            const SourceRecord source = pointing.source ? *pointing.source : station.source;
            const std::optional<double> gridDistance =
                gridDistanceFrom(engine, setupIndex, pointing, here);
            const Pointing group{pointing.leftIndex, Face::Unknown};
            if (pointing.direction) {
                HorizontalDirectionObservation direction;
                direction.at = at;
                direction.to = pointing.target;
                direction.direction = *pointing.direction;
                // Across the measured line where there is one, else the line
                // the start gives: centring is millimetres, the start closer.
                direction.sigma =
                    withCentring(apriori, pointing.sigmaDirection,
                                 gridDistance.value_or(std::abs(planeOf(target) - start)),
                                 StationModel::Resected);
                direction.source = source;
                direction.pointing = group;
                inputs.directions.emplace_back(p, direction.sigma);
                inputs.sights.push_back(Sight{planeOf(target), direction.sigma, true});
                inputs.network.observations.push_back(NetworkObservation{
                    direction, label("direction", pointing.target), source, &pointing,
                    PointingPart::Direction});
            }
            if (gridDistance) {
                DistanceObservation distance;
                distance.from = at;
                distance.to = pointing.target;
                distance.distance = *gridDistance;
                distance.sigma = distanceSigmaOf(apriori, pointing, StationModel::Resected);
                distance.kind = DistanceKind::Horizontal;
                distance.source = source;
                distance.pointing = group;
                inputs.sights.push_back(Sight{planeOf(target), distance.sigma, false});
                inputs.network.observations.push_back(NetworkObservation{
                    distance, label("distance", pointing.target), source, &pointing,
                    PointingPart::Distance});
            }
        }
        return inputs;
    };

    // ---- where its least squares starts ----
    // Every point where two of its loci meet (the section's head), and the
    // rigid fit.
    std::vector<Locus> loci;
    {
        // A circle per distinct place with a distance, its radius the mean.
        std::vector<Plane> centres;
        for (const ResectionTarget& target : data.targets) {
            const Plane centre = planeOf(*target.position);
            double sum = 0.0;
            std::size_t count = 0;
            for (const Reading& reading : readings) {
                if (!reading.direction && reading.target == centre) {
                    sum += reading.value;
                    ++count;
                }
            }
            const bool repeated = std::any_of(centres.begin(), centres.end(), [&](Plane other) {
                return std::abs(other - centre) <= onePosition;
            });
            if (count > 0 && !repeated) {
                centres.push_back(centre);
                loci.push_back(Locus{centre, sum / static_cast<double>(count), {}});
            }
        }
        // An arc per pair of targets read with a direction: each with the
        // next round the circle and with the one it makes the widest angle
        // with - every pair of three targets, and of more a number of arcs
        // that grows with the targets, not with their square, while every
        // target is on two of them.
        const std::vector<const ResectionTarget*>& seen = distinct.withDirection;
        const std::size_t count = seen.size();
        std::vector<std::size_t> byReading(count);
        for (std::size_t k = 0; k < count; ++k) {
            byReading[k] = k;
        }
        std::stable_sort(byReading.begin(), byReading.end(), [&seen](std::size_t x, std::size_t y) {
            return *seen[x]->direction < *seen[y]->direction;
        });
        std::vector<std::pair<std::size_t, std::size_t>> pairs;
        const auto pair = [&pairs](std::size_t i, std::size_t j) {
            const std::pair<std::size_t, std::size_t> both{std::min(i, j), std::max(i, j)};
            if (i != j && std::find(pairs.begin(), pairs.end(), both) == pairs.end()) {
                pairs.push_back(both);
            }
        };
        for (std::size_t k = 0; k < count; ++k) {
            pair(byReading[k], byReading[(k + 1) % count]);
        }
        for (std::size_t i = 0; i < count; ++i) {
            std::size_t widest = i;
            double sine = -1.0;
            for (std::size_t j = 0; j < count; ++j) {
                const double s = std::abs(std::sin(*seen[j]->direction - *seen[i]->direction));
                if (j != i && s > sine) {
                    sine = s;
                    widest = j;
                }
            }
            pair(i, widest);
        }
        for (const auto& [i, j] : pairs) {
            const Plane from = planeOf(*seen[i]->position);
            const Plane to = planeOf(*seen[j]->position);
            const double angle = *seen[j]->direction - *seen[i]->direction;
            if (std::abs(std::sin(angle)) <= katana::math::tolerance::kAngular) {
                loci.push_back(Locus{from, 0.0, (to - from) / std::abs(to - from)});
            } else {
                const Plane centre = inscribedCentre(from, to, angle);
                loci.push_back(Locus{centre, std::abs(from - centre), {}});
            }
        }
    }
    std::vector<Plane> starts;
    if (fit) {
        starts.push_back(*fit);
    }
    for (std::size_t a = 0; a < loci.size(); ++a) {
        for (std::size_t b = a + 1; b < loci.size(); ++b) {
            meet(loci[a], loci[b], starts);
        }
    }
    // Not a start: a point that is not finite, or one on a target, where
    // every arc through that target meets another.
    std::erase_if(starts, [&](Plane start) {
        return !std::isfinite(start.real()) || !std::isfinite(start.imag()) ||
               std::any_of(data.targets.begin(), data.targets.end(),
                           [&](const ResectionTarget& target) {
                               return std::abs(planeOf(*target.position) - start) <= onePosition;
                           });
    });

    // ---- its solutions ----
    const double confidenceScale = ellipseConfidenceScale(settings.confidenceLevel)
                                       .valueOr(std::numeric_limits<double>::infinity());
    // chi-square(2) at the confidence level: the edge of the station's
    // confidence region, in weighted squares above the least.
    const double bound = confidenceScale * confidenceScale;
    const std::string confidenceWords = formatNumber(settings.confidenceLevel * 100.0, 0) + " %";
    AdjustmentOptions options;
    options.maxIterations = settings.maxIterations;
    options.significanceLevel = 1.0 - settings.confidenceLevel;
    // Why the first least squares that failed did, and where it started.
    std::string failure;
    Plane failedAt{};
    const auto fail = [&failure, &failedAt](Plane start, const std::string& message) {
        if (failure.empty()) {
            failure = message;
            failedAt = start;
        }
    };
    const auto solveFrom = [&](Plane start,
                               const std::vector<bool>& excluded) -> std::optional<Solution> {
        const Inputs inputs = inputsAt(start);
        std::vector<std::size_t> included;
        const Result<SurveyNetwork> network = buildNetwork(inputs.network, excluded, included);
        if (!network) {
            fail(start, network.error().message);
            return std::nullopt;
        }
        const Result<HorizontalAdjustmentResult> result = adjustHorizontalNetwork(*network, options);
        if (!result) {
            fail(start, result.error().message);
            return std::nullopt;
        }
        std::optional<Plane> position;
        for (const AdjustedStation& adjusted : result->stations) {
            if (adjusted.pointId == at) {
                position = Plane{adjusted.position.easting, adjusted.position.northing};
            }
        }
        std::vector<Sight> kept;
        for (std::size_t i = 0; i < inputs.sights.size(); ++i) {
            if (!excluded[i]) {
                kept.push_back(inputs.sights[i]);
            }
        }
        const std::optional<StationStrength> strength =
            position ? stationStrength(*result, at, *position, kept, confidenceScale)
                     : std::nullopt;
        if (!strength) {
            fail(start, "it gives the station no precision");
            return std::nullopt;
        }
        return Solution{*position, result->statistics.weightedSquaredResiduals, *strength};
    };
    // The solutions from the starts, beside `found`, over the observations
    // `excluded` leaves (see the section's head).
    const auto search = [&](const std::vector<bool>& excluded, std::vector<Solution> found) {
        struct Start {
            Plane point{};
            double squares = 0.0;
        };
        std::vector<Start> order;
        for (const Plane start : starts) {
            const double squares = squaresAt(start, readings, excluded, apriori);
            if (std::isfinite(squares)) {
                order.push_back(Start{start, squares});
            }
        }
        std::stable_sort(order.begin(), order.end(),
                         [](const Start& x, const Start& y) { return x.squares < y.squares; });
        for (const Start& start : order) {
            double least = std::numeric_limits<double>::infinity();
            bool reached = false;
            for (const Solution& solution : found) {
                least = std::min(least, solution.squares);
                reached = reached || std::abs(start.point - solution.position) <=
                                         solution.strength.linearWithin;
            }
            if (start.squares > least + bound) {
                break; // it, and every start after it, fits worse than the region allows
            }
            if (reached) {
                continue; // within a solution's linear reach: that solution
            }
            std::optional<Solution> solution = solveFrom(start.point, excluded);
            if (!solution) {
                continue;
            }
            const bool known = std::any_of(found.begin(), found.end(), [&](const Solution& other) {
                return mahalanobis2(solution->position - other.position, other.strength.cofactor) <=
                       bound;
            });
            if (!known) {
                found.push_back(std::move(*solution));
            }
        }
        return found;
    };
    const auto shapeAt = [&distinct](Plane position) {
        const std::string clause = nearestShape(distinct.withDirection, position).clause;
        return clause.empty() ? clause : clause + ", and ";
    };
    const auto twoPositions = [&](const Solution& one, const Solution& other) {
        return shapeAt(one.position) + "its observations fit two positions " +
               lengthWords(std::abs(other.position - one.position)) + " apart (" +
               positionWords(one.position) + " and " + positionWords(other.position) +
               ": weighted squares of residuals " + formatNumber(one.squares, 3) + " and " +
               formatNumber(other.squares, 3) + ", where the station's " + confidenceWords +
               " confidence region takes in all within " + formatNumber(bound, 2) +
               " of the least), so they do not fix it";
    };

    const std::vector<Solution> solutions =
        search(std::vector<bool>(readings.size(), false), {});
    if (solutions.empty()) {
        // A rank deficiency (network_adjustment.cpp's rankError) is the
        // geometry's: its observations do not determine the station where the
        // best start put it, and the shape says how. A failure to converge is
        // not, and names none.
        const bool geometric = failure.starts_with("the network is rank deficient");
        why = (geometric ? shapeAt(failedAt) : std::string{}) + "its least squares cannot fix it (" +
              (failure.empty() ? std::string("no start was found") : failure) + ")";
        return false;
    }
    const Solution& best = solutions[leastOf(solutions)];
    if (const Solution* rival = rivalOf(solutions, best, bound); rival != nullptr) {
        why = twoPositions(best, *rival);
        return false;
    }

    // ---- the least squares it is reported by, from the best solution ----
    const Inputs horizontal = inputsAt(best.position);

    // What its least squares leaves behind - warnings, observations marked
    // rejected - so that a refusal takes it back: a setup that was not
    // resected has no resection residuals.
    const std::size_t warningsBefore = engine.report.warnings.size();
    struct RowState {
        std::size_t row = 0;
        bool rejected = false;
        std::string reason;
    };
    std::vector<RowState> rowsBefore;
    for (const std::size_t p : data.pointings) {
        for (const PointingPart part :
             {PointingPart::Direction, PointingPart::Distance, PointingPart::Height}) {
            for (const std::size_t row : partRows(engine.pointings[p], part)) {
                const ReportObservation& observation = engine.report.observations[row];
                rowsBefore.push_back(
                    RowState{row, observation.rejected, observation.rejectionReason});
            }
        }
    }
    const auto takeBack = [&engine, &rowsBefore, warningsBefore]() {
        engine.report.warnings.erase(
            engine.report.warnings.begin() + static_cast<std::ptrdiff_t>(warningsBefore),
            engine.report.warnings.end());
        for (const RowState& previous : rowsBefore) {
            ReportObservation& observation = engine.report.observations[previous.row];
            observation.rejected = previous.rejected;
            observation.rejectionReason = previous.reason;
        }
    };

    const std::string name = "resection at " + station.setup.id;
    AdjustmentReport horizontalReport;
    const Result<HorizontalAdjustmentResult> solved = adjustWithOutliers<HorizontalAdjustmentResult>(
        engine, horizontal.network, name + " (horizontal)",
        [](const SurveyNetwork& network, const AdjustmentOptions& adjustment) {
            return adjustHorizontalNetwork(network, adjustment);
        },
        horizontalReport, "the resection");
    if (!solved) {
        takeBack();
        why = "its least squares cannot fix it (" + solved.error().message + ")";
        return false;
    }
    const AdjustedStation* fixed = nullptr;
    for (const AdjustedStation& adjusted : solved->stations) {
        if (adjusted.pointId == at) {
            fixed = &adjusted;
        }
    }
    const AdjustedOrientation* orientation = nullptr;
    for (const AdjustedOrientation& adjusted : solved->orientations) {
        if (adjusted.pointId == at) {
            orientation = &adjusted;
        }
    }
    if (fixed == nullptr || orientation == nullptr) {
        takeBack();
        why = "no direction is left to orient it";
        return false;
    }

    // ---- does it fix the station? ----
    const Plane solution{fixed->position.easting, fixed->position.northing};
    std::vector<bool> excluded(horizontal.network.observations.size(), false);
    std::vector<Sight> kept;
    for (std::size_t i = 0; i < horizontal.network.observations.size(); ++i) {
        const NetworkObservation& observation = horizontal.network.observations[i];
        for (const std::size_t row : partRows(*observation.pointing, observation.part)) {
            excluded[i] = excluded[i] || engine.report.observations[row].rejected;
        }
        if (!excluded[i]) {
            kept.push_back(horizontal.sights[i]);
        }
    }
    const std::optional<StationStrength> strength =
        stationStrength(*solved, at, solution, kept, confidenceScale);
    if (strength && std::find(excluded.begin(), excluded.end(), true) != excluded.end()) {
        // What the outlier test rejected changes what the rest fix: searched
        // again without it, the solution reached the first found.
        const std::vector<Solution> again = search(
            excluded, {Solution{solution, solved->statistics.weightedSquaredResiduals, *strength}});
        const Solution& least = again[leastOf(again)];
        const Solution* rival = &least != &again.front() ? &least : rivalOf(again, least, bound);
        if (rival != nullptr) {
            takeBack();
            why = twoPositions(again.front(), *rival);
            return false;
        }
    }
    if (!strength || !strength->fixes()) {
        takeBack();
        why = strength ? shapeAt(solution) + "at the precision of its observations it is uncertain by " +
                             lengthWords(strength->confidence) + " (" + confidenceWords +
                             ") along azimuth " + axisWords(strength->apriori) + ", past the " +
                             lengthWords(strength->linearWithin) +
                             " within which its least squares' linear model holds"
                       : "its least squares gives it no precision";
        return false;
    }

    // ---- its height ----
    std::optional<double> elevation;
    std::optional<double> sigmaElevation;
    std::optional<AdjustmentReport> heightReport;
    NetworkInputs levels;
    {
        SurveyPoint free;
        free.id = at;
        free.northing = fixed->position.northing;
        free.easting = fixed->position.easting;
        free.elevation = approximate.height;
        levels.points.push_back(std::move(free));
    }
    std::unordered_set<std::string_view> heldInHeight;
    for (const std::size_t p : data.pointings) {
        const ReducedPointing& pointing = engine.pointings[p];
        const Position& target = engine.positions.at(pointing.target);
        if (!pointing.heightDifference || !pointing.slope || !pointing.zenith || !target.height) {
            continue;
        }
        if (heldInHeight.insert(pointing.target).second) {
            SurveyPoint held;
            held.id = pointing.target;
            held.northing = target.northing;
            held.easting = target.easting;
            held.elevation = target.height;
            levels.points.push_back(std::move(held));
            levels.control.push_back(ControlPoint::fixedVertical(std::string(pointing.target)));
        }
        const SourceRecord source = pointing.source ? *pointing.source : station.source;
        levels.observations.push_back(NetworkObservation{
            LevelDifferenceObservation{at, std::string(pointing.target), *pointing.heightDifference,
                                       heightDifferenceSigmaOf(apriori, pointing,
                                                               StationModel::Resected),
                                       pointing.horizontal.value_or(0.0), source},
            label("height difference", pointing.target), source, &pointing, PointingPart::Height});
    }
    if (levels.observations.empty()) {
        engine.warnSetup(station, "resected with no height: no point it was resected from has a "
                                  "height and was observed with both a zenith angle and a "
                                  "distance, which a trigonometric height needs, so nothing "
                                  "radiated from it has one.");
    } else {
        AdjustmentReport report;
        const Result<LevelAdjustmentResult> levelled = adjustWithOutliers<LevelAdjustmentResult>(
            engine, levels, name + " (heights)",
            [](const SurveyNetwork& network, const AdjustmentOptions& adjustment) {
                return adjustLevelNetwork(network, adjustment);
            },
            report, "the resection");
        if (levelled) {
            const double scale = precisionScaleOf(levelled->statistics);
            for (std::size_t k = 0; k < levelled->parameters.size(); ++k) {
                if (levelled->parameters[k].pointId == at) {
                    sigmaElevation = std::sqrt(levelled->cofactor(k, k) * scale);
                }
            }
            for (const AdjustedElevation& adjusted : levelled->elevations) {
                if (adjusted.pointId == at) {
                    elevation = adjusted.elevation;
                }
            }
            report.iterations = 1;
            heightReport = std::move(report);
        } else {
            engine.warn("Setup " + station.setup.id + ": the heights of its resection could not "
                        "be adjusted (" + levelled.error().message +
                        "), so its station has no height.",
                        station.source);
        }
    }
    if (horizontalReport.redundancy == 0) {
        engine.warnSetup(station, "its resection has no redundancy - as many observations as "
                                  "unknowns - so nothing checks the position it gives.");
    }

    // ---- the station ----
    // Its precision is the least squares' a-priori one, scaled only where the
    // global test finds its residuals too large (precisionScaleOf). The least
    // squares placed the instrument. Where it stands over a mark - the setup
    // records an instrument height - the mark is off the instrument by the
    // centring and below it by the measured height, errors common to every
    // pointing and so in no pointing's weight (StationModel): they are the
    // station's, added here. An instrument height of zero is a free station
    // with no mark under it, the instrument itself the point.
    const double planScale = precisionScaleOf(solved->statistics);
    Covariance2 covariance{strength->cofactor.northing * planScale,
                           strength->cofactor.easting * planScale,
                           strength->cofactor.northingEasting * planScale};
    double sigmaOrientation = 0.0;
    for (std::size_t k = 0; k < solved->parameters.size(); ++k) {
        if (solved->parameters[k].pointId == at &&
            solved->parameters[k].component == CoordinateComponent::Orientation) {
            sigmaOrientation = std::sqrt(solved->cofactor(k, k) * planScale);
        }
    }
    const bool overAMark = station.setup.instrumentHeight != 0.0;
    if (overAMark) {
        const double centring = apriori.instrumentCentring * apriori.instrumentCentring;
        covariance.northing += centring;
        covariance.easting += centring;
        if (sigmaElevation) {
            sigmaElevation = std::hypot(*sigmaElevation, apriori.heightMeasurement);
        }
    }
    Position position;
    position.northing = fixed->position.northing;
    position.easting = fixed->position.easting;
    position.height = elevation;
    position.origin = PositionOrigin::Computed;
    position.method = ComputationMethod::Resection;
    position.sigmaNorthing = std::sqrt(covariance.northing);
    position.sigmaEasting = std::sqrt(covariance.easting);
    position.sigmaHeight = sigmaElevation;
    engine.place(at, position);

    state.resected = true;
    state.resectionDirections.clear();
    for (const auto& [p, sigma] : horizontal.directions) {
        // One the outlier test rejected is not in the solution, so not in its
        // orientation either.
        if (!directionRejected(engine, engine.pointings[p])) {
            state.resectionDirections.emplace_back(p, sigma);
        }
    }
    state.resectionPointings.clear();
    state.resectionPointings.insert(data.pointings.begin(), data.pointings.end());
    state.resectionTargets.clear();
    for (const ResectionTarget& target : data.targets) {
        state.resectionTargets.insert(target.id);
    }

    ResectionReport report;
    report.stationId = station.setup.id;
    report.pointId = at;
    for (const ResectionTarget& target : data.targets) {
        report.targets.emplace_back(target.id);
    }
    report.northing = position.northing;
    report.easting = position.easting;
    report.elevation = elevation;
    report.sigmaNorthing = *position.sigmaNorthing;
    report.sigmaEasting = *position.sigmaEasting;
    report.sigmaElevation = sigmaElevation;
    report.orientation = normalizeAngleSigned(orientation->orientation);
    report.sigmaOrientation = sigmaOrientation;
    report.aprioriEllipse = ReportEllipse{at, strength->apriori, confidenceScale};
    report.dilution = strength->dilution();
    report.weakGeometry = report.dilution > kWeakResectionDilution;
    report.checks = data.checks;
    horizontalReport.iterations = solved->iterations;
    horizontalReport.ellipses.push_back(
        ReportEllipse{at, errorEllipse(covariance).valueOr(fixed->ellipse), confidenceScale});
    report.horizontal = std::move(horizontalReport);
    report.height = std::move(heightReport);
    report.source = station.source;
    const std::string resectedFrom = namesOf(distinct.withDirection);

    // The position another setup radiated the station to, which the
    // resection replaced: a check of the one against the other, and a warning.
    if (radiated) {
        const std::size_t by = radiated->fromSetup;
        const std::string radiator = by < engine.raw.stations.size()
                                         ? engine.raw.stations[by].setup.id
                                         : std::string("another setup");
        MisclosureReport check;
        check.name = at + " by resection at setup " + station.setup.id +
                     ", against its radiation from setup " + radiator;
        check.northing = position.northing - radiated->northing;
        check.easting = position.easting - radiated->easting;
        check.linear = std::hypot(*check.northing, *check.easting);
        if (position.height && radiated->height) {
            check.height = *position.height - *radiated->height;
        }
        report.radiatedFrom = radiator;
        report.radiatedNorthingDifference = check.northing;
        report.radiatedEastingDifference = check.easting;
        engine.warn("Setup " + station.setup.id + " stands on " + at + ", which setup " + radiator +
                        " radiated: its own resection from " + resectedFrom + " places it " +
                        lengthWords(*check.linear) +
                        " from there, and the radiation is a check of it (the misclosure \"" +
                        check.name + "\").",
                    station.source);
        engine.report.misclosures.push_back(std::move(check));
    }

    // The file's own coordinates for the station, which the resection
    // replaced (placeSetups says why it comes first): not dropped unseen, but
    // a check of the one against the other, and a warning.
    if (const auto file = engine.filePoints.find(at); file != engine.filePoints.end()) {
        MisclosureReport check;
        check.name = at + " by resection at setup " + station.setup.id +
                     ", against the file's own coordinates";
        check.northing = position.northing - file->second->northing;
        check.easting = position.easting - file->second->easting;
        check.linear = std::hypot(*check.northing, *check.easting);
        if (position.height && file->second->elevation) {
            check.height = *position.height - *file->second->elevation;
        }
        report.fileNorthingDifference = check.northing;
        report.fileEastingDifference = check.easting;
        engine.warn("Setup " + station.setup.id + " stands on " + at + ": its resection from " +
                        resectedFrom + " places it " + lengthWords(*check.linear) +
                        " from the file's own coordinates for it, which were not used (the "
                        "misclosure \"" + check.name + "\").",
                    station.source);
        engine.report.misclosures.push_back(std::move(check));
    }

    // Ground distances fitted to grid coordinates: where the settings reduce
    // no distance to the grid and the drawing's projection says the grid is
    // not the ground there, by more than the longest distance's own standard
    // deviation, the resection's distance residuals and its station carry the
    // difference, and nothing else would say why.
    if (!settings.useCombinedFactor && settings.gridScale == GridScale::None &&
        engine.context.gridScaleFactor) {
        double longest = 0.0;
        double longestSigma = 0.0;
        for (const Sight& sight : kept) {
            const double length = std::abs(sight.target - solution);
            if (!sight.direction && length > longest) {
                longest = length;
                longestSigma = sight.sigma;
            }
        }
        const std::optional<double> scale = engine.context.gridScaleFactor(
            position.northing, position.easting, position.height.value_or(0.0));
        if (longest > 0.0 && scale && std::isfinite(*scale) &&
            std::abs(*scale - 1.0) * longest > longestSigma) {
            report.unappliedScaleFactor = *scale;
            engine.warnSetup(station,
                             "its resection fitted distances measured on the ground to its "
                             "targets' grid coordinates: the drawing's projection gives a point "
                             "scale factor at the station that these settings do not apply (no "
                             "grid scale), and it changes the longest distance by more than that "
                             "distance's standard deviation, so the resection's residuals and "
                             "its station carry the difference. Set the grid scale to the "
                             "projection's to reduce the distances to the grid.");
        }
    }

    if (report.weakGeometry) {
        engine.warn("Setup " + station.setup.id + ": its resection from " + resectedFrom +
                        " has a weak geometry - at the precision of its observations its station "
                        "is uncertain by " + lengthWords(strength->confidence) + " (" +
                        confidenceWords + ") along azimuth " + axisWords(strength->apriori) +
                        ", " + formatNumber(report.dilution, 1) +
                        " times the standard deviation of its least precise observation - and "
                        "everything radiated from it carries that.",
                    station.source);
    }
    engine.report.resections.push_back(std::move(report));
    return true;
}

} // namespace katana::survey::detail
