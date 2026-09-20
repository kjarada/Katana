#include "katana/survey/network_adjustment.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <string>
#include <string_view>
#include <utility>
#include <variant>

#include "ellipse_core.hpp"
#include "katana/survey/statistics.hpp"
#include "katana/survey/traverse.hpp"
#include "lsq_core.hpp"

namespace katana::survey {

using katana::core::Error;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::math::normalizeAngle;
using katana::math::normalizeAngleSigned;
namespace tol = katana::math::tolerance;

namespace {

constexpr Eigen::Index kNotAParameter = -1; // fixed component, or point outside the adjustment

// Unit conversion of the distance-proportional part of an EDM standard
// deviation (parts per million), not a tolerance.
constexpr double kPartsPerMillion = 1e-6;

struct Term {
    Eigen::Index parameter = kNotAParameter;
    double coefficient = 0.0;
};

// One linearised observation equation: sum(coefficient * correction) = misclosure + v.
struct Equation {
    ResidualSource source = ResidualSource::Observed;
    std::size_t index = 0;
    ResidualComponent component = ResidualComponent::Value;
    double sigma = 1.0;
    double misclosure = 0.0; // observed - computed from the current coordinates
    // An angle is the difference of two azimuths, each with four partials (two
    // per end); the occupied station therefore appears twice per component.
    std::array<Term, 8> terms{};
    std::size_t termCount = 0;

    // Coefficients of fixed components multiply a zero correction and are dropped.
    void add(Eigen::Index parameter, double coefficient)
    {
        if (parameter != kNotAParameter) {
            terms[termCount++] = Term{parameter, coefficient};
        }
    }
};

Status checkOptions(const AdjustmentOptions& options)
{
    if (options.maxIterations == 0) {
        return makeError(ErrorCode::InvalidArgument, "maxIterations must be at least 1");
    }
    if (!std::isfinite(options.convergenceThreshold) || !(options.convergenceThreshold > 0.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         "convergenceThreshold must be positive and finite");
    }
    if (!(options.significanceLevel > 0.0 && options.significanceLevel < 1.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         "significanceLevel must lie strictly between 0 and 1");
    }
    return {};
}

std::string parameterName(const AdjustedParameter& parameter)
{
    const char* component = parameter.component == CoordinateComponent::Northing  ? "N"
                            : parameter.component == CoordinateComponent::Easting ? "E"
                                                                                  : "H";
    return std::string(component) + "(" + parameter.pointId + ")";
}

Error rankError(const detail::RankDeficiency& deficiency,
                const std::vector<AdjustedParameter>& parameters, const std::string& hint)
{
    std::string names;
    for (const std::size_t index : deficiency.unresolved) {
        names += names.empty() ? "" : ", ";
        names += parameterName(parameters[index]);
    }
    std::string message = "the network is rank deficient (rank " +
                          std::to_string(deficiency.rank) + " of " +
                          std::to_string(parameters.size()) +
                          " unknowns); coordinates that cannot be resolved: " + names;
    if (!hint.empty()) {
        message += ". " + hint;
    }
    return makeError(ErrorCode::AdjustmentFailure, std::move(message));
}

// Whitens (divides each row by its sigma) and solves.
std::variant<detail::WhitenedSolution, detail::RankDeficiency>
solveEquations(const std::vector<Equation>& equations, std::size_t unknownCount)
{
    const auto rows = static_cast<Eigen::Index>(equations.size());
    Eigen::MatrixXd design = Eigen::MatrixXd::Zero(rows, static_cast<Eigen::Index>(unknownCount));
    Eigen::VectorXd misclosures(rows);
    for (Eigen::Index r = 0; r < rows; ++r) {
        const Equation& equation = equations[static_cast<std::size_t>(r)];
        for (std::size_t t = 0; t < equation.termCount; ++t) {
            design(r, equation.terms[t].parameter) += equation.terms[t].coefficient / equation.sigma;
        }
        misclosures[r] = equation.misclosure / equation.sigma;
    }
    return detail::solveWhitened(design, misclosures);
}

Result<AdjustmentStatistics> makeStatistics(const std::vector<Equation>& equations,
                                            std::size_t unknownCount,
                                            const AdjustmentOptions& options)
{
    AdjustmentStatistics statistics;
    statistics.observationCount = equations.size();
    statistics.unknownCount = unknownCount;
    statistics.degreesOfFreedom = equations.size() - unknownCount;
    for (const Equation& equation : equations) {
        const double whitened = equation.misclosure / equation.sigma;
        statistics.weightedSquaredResiduals += whitened * whitened;
    }
    if (statistics.degreesOfFreedom == 0) {
        return statistics; // variance factor and global test are undefined
    }
    statistics.varianceFactor = statistics.weightedSquaredResiduals /
                                static_cast<double>(statistics.degreesOfFreedom);
    if (options.covarianceScaling == CovarianceScaling::APosteriori) {
        statistics.covarianceScale = *statistics.varianceFactor;
    }

    GlobalTest test;
    test.statistic = statistics.weightedSquaredResiduals;
    test.significanceLevel = options.significanceLevel;
    const auto lower =
        chiSquareQuantile(0.5 * options.significanceLevel, statistics.degreesOfFreedom);
    const auto upper =
        chiSquareQuantile(1.0 - 0.5 * options.significanceLevel, statistics.degreesOfFreedom);
    if (!lower) {
        return lower.error();
    }
    if (!upper) {
        return upper.error();
    }
    test.lowerCritical = *lower;
    test.upperCritical = *upper;
    test.passed = test.statistic >= test.lowerCritical && test.statistic <= test.upperCritical;
    statistics.globalTest = test;
    return statistics;
}

// Residuals of the final linearisation: v = computed - observed = -misclosure.
std::vector<ResidualRecord> makeResiduals(const std::vector<Equation>& equations,
                                          const Eigen::VectorXd& leverage)
{
    std::vector<ResidualRecord> records;
    records.reserve(equations.size());
    for (std::size_t r = 0; r < equations.size(); ++r) {
        const Equation& equation = equations[r];
        ResidualRecord record;
        record.source = equation.source;
        record.index = equation.index;
        record.component = equation.component;
        record.residual = -equation.misclosure;
        record.sigma = equation.sigma;
        record.redundancyNumber = 1.0 - leverage[static_cast<Eigen::Index>(r)];
        if (record.redundancyNumber > tol::kRelative) {
            record.standardizedResidual =
                record.residual / (equation.sigma * std::sqrt(record.redundancyNumber));
        }
        records.push_back(record);
    }
    return records;
}

void logOutcome(const AdjustmentOptions& options, std::string_view what,
                const AdjustmentStatistics& statistics, std::size_t iterations)
{
    if (options.logger == nullptr) {
        return;
    }
    options.logger->info(
        "survey", what,
        {{"observations", std::to_string(statistics.observationCount)},
         {"unknowns", std::to_string(statistics.unknownCount)},
         {"degrees_of_freedom", std::to_string(statistics.degreesOfFreedom)},
         {"iterations", std::to_string(iterations)},
         {"variance_factor",
          statistics.varianceFactor ? std::to_string(*statistics.varianceFactor) : "undefined"}});
}

void logFailure(const AdjustmentOptions& options, std::string_view what, const Error& error)
{
    if (options.logger != nullptr) {
        options.logger->error("survey", what, {{"error", error.describe()}});
    }
}

// ---- horizontal network ------------------------------------------------------

bool isHorizontal(const Observation& observation)
{
    if (const auto* distance = std::get_if<DistanceObservation>(&observation)) {
        return distance->kind == DistanceKind::Horizontal;
    }
    return std::holds_alternative<HorizontalAngleObservation>(observation) ||
           std::holds_alternative<AzimuthObservation>(observation) ||
           std::holds_alternative<GnssBaselineObservation>(observation) ||
           std::holds_alternative<GnssPositionObservation>(observation);
}

struct HorizontalModel {
    const SurveyNetwork* network = nullptr;
    std::vector<std::size_t> usedObservations;
    std::vector<std::size_t> unusedObservations;
    std::vector<bool> involved; // per network point
    std::vector<Eigen::Index> northingParameter;
    std::vector<Eigen::Index> eastingParameter;
    std::vector<AdjustedParameter> parameters;

    [[nodiscard]] std::size_t pointOf(const std::string& id) const
    {
        return *network->pointIndex(id); // the network guarantees referential integrity
    }
};

HorizontalModel buildHorizontalModel(const SurveyNetwork& network)
{
    HorizontalModel model;
    model.network = &network;
    const std::size_t pointCount = network.points().size();
    model.involved.assign(pointCount, false);
    model.northingParameter.assign(pointCount, kNotAParameter);
    model.eastingParameter.assign(pointCount, kNotAParameter);

    const auto& observations = network.observations();
    for (std::size_t i = 0; i < observations.size(); ++i) {
        if (!isHorizontal(observations[i])) {
            model.unusedObservations.push_back(i);
            continue;
        }
        model.usedObservations.push_back(i);
        for (const std::string& id : referencedPoints(observations[i])) {
            model.involved[model.pointOf(id)] = true;
        }
    }

    // Unknowns in network point order, northing before easting: the layout of
    // the cofactor matrix is deterministic and documented.
    for (std::size_t p = 0; p < pointCount; ++p) {
        if (!model.involved[p]) {
            continue;
        }
        const std::string& id = network.points()[p].id;
        const auto control = network.controlFor(id);
        if (!control || control->northing.constraint != ControlConstraint::Fixed) {
            model.northingParameter[p] = static_cast<Eigen::Index>(model.parameters.size());
            model.parameters.push_back({id, CoordinateComponent::Northing});
        }
        if (!control || control->easting.constraint != ControlConstraint::Fixed) {
            model.eastingParameter[p] = static_cast<Eigen::Index>(model.parameters.size());
            model.parameters.push_back({id, CoordinateComponent::Easting});
        }
    }
    return model;
}

struct Line {
    double deltaNorthing = 0.0;
    double deltaEasting = 0.0;
    double distance = 0.0;
};

Result<Line> lineBetween(const HorizontalModel& model, const std::vector<Coordinate2>& coordinates,
                         std::size_t from, std::size_t to)
{
    Line line;
    line.deltaNorthing = coordinates[to].northing - coordinates[from].northing;
    line.deltaEasting = coordinates[to].easting - coordinates[from].easting;
    line.distance = std::hypot(line.deltaNorthing, line.deltaEasting);
    if (!std::isfinite(line.distance)) {
        return makeError(ErrorCode::AdjustmentFailure,
                         "the adjustment diverged: coordinates are no longer finite");
    }
    if (line.distance <= tol::kCoordinate) {
        const auto& points = model.network->points();
        return makeError(ErrorCode::AdjustmentFailure,
                         "two observed stations coincide; direction and distance partials are "
                         "undefined",
                         "stations " + points[from].id + " and " + points[to].id);
    }
    return line;
}

// Adds the azimuth partials of the line from -> to, scaled by `sign`.
void addAzimuthTerms(Equation& equation, const HorizontalModel& model, const Line& line,
                     std::size_t from, std::size_t to, double sign)
{
    const double squared = line.distance * line.distance;
    const double dN = sign * -line.deltaEasting / squared; // d azimuth / d N_to
    const double dE = sign * line.deltaNorthing / squared; // d azimuth / d E_to
    equation.add(model.northingParameter[to], dN);
    equation.add(model.eastingParameter[to], dE);
    equation.add(model.northingParameter[from], -dN);
    equation.add(model.eastingParameter[from], -dE);
}

Result<std::vector<Equation>> linearizeHorizontal(const HorizontalModel& model,
                                                  const std::vector<Coordinate2>& coordinates)
{
    const SurveyNetwork& network = *model.network;
    std::vector<Equation> equations;

    for (const std::size_t index : model.usedObservations) {
        const Observation& observation = network.observations()[index];
        Equation base;
        base.source = ResidualSource::Observed;
        base.index = index;

        if (const auto* distance = std::get_if<DistanceObservation>(&observation)) {
            const std::size_t from = model.pointOf(distance->from);
            const std::size_t to = model.pointOf(distance->to);
            const auto line = lineBetween(model, coordinates, from, to);
            if (!line) {
                return line.error();
            }
            Equation equation = base;
            equation.sigma = distance->sigma;
            equation.misclosure = distance->distance - line->distance;
            const double dN = line->deltaNorthing / line->distance;
            const double dE = line->deltaEasting / line->distance;
            equation.add(model.northingParameter[to], dN);
            equation.add(model.eastingParameter[to], dE);
            equation.add(model.northingParameter[from], -dN);
            equation.add(model.eastingParameter[from], -dE);
            equations.push_back(equation);
        } else if (const auto* azimuth = std::get_if<AzimuthObservation>(&observation)) {
            const std::size_t from = model.pointOf(azimuth->from);
            const std::size_t to = model.pointOf(azimuth->to);
            const auto line = lineBetween(model, coordinates, from, to);
            if (!line) {
                return line.error();
            }
            Equation equation = base;
            equation.sigma = azimuth->sigma;
            // Wrapped: an azimuth of 359.999° against a computed 0.001° is a
            // misclosure of -0.002°, not of 359.998°.
            equation.misclosure = normalizeAngleSigned(
                azimuth->azimuth - std::atan2(line->deltaEasting, line->deltaNorthing));
            addAzimuthTerms(equation, model, *line, from, to, 1.0);
            equations.push_back(equation);
        } else if (const auto* angle = std::get_if<HorizontalAngleObservation>(&observation)) {
            const std::size_t at = model.pointOf(angle->at);
            const std::size_t back = model.pointOf(angle->from);
            const std::size_t fore = model.pointOf(angle->to);
            const auto backLine = lineBetween(model, coordinates, at, back);
            if (!backLine) {
                return backLine.error();
            }
            const auto foreLine = lineBetween(model, coordinates, at, fore);
            if (!foreLine) {
                return foreLine.error();
            }
            Equation equation = base;
            equation.sigma = angle->sigma;
            const double computed = std::atan2(foreLine->deltaEasting, foreLine->deltaNorthing) -
                                    std::atan2(backLine->deltaEasting, backLine->deltaNorthing);
            equation.misclosure = normalizeAngleSigned(angle->angle - computed);
            // angle = azimuth(at->fore) - azimuth(at->back). The occupied station
            // receives one term per line and component; solveEquations sums them.
            addAzimuthTerms(equation, model, *foreLine, at, fore, 1.0);
            addAzimuthTerms(equation, model, *backLine, at, back, -1.0);
            equations.push_back(equation);
        } else if (const auto* baseline = std::get_if<GnssBaselineObservation>(&observation)) {
            const std::size_t from = model.pointOf(baseline->from);
            const std::size_t to = model.pointOf(baseline->to);
            Equation north = base;
            north.component = ResidualComponent::Northing;
            north.sigma = baseline->sigmaNorthing;
            north.misclosure = baseline->deltaNorthing -
                               (coordinates[to].northing - coordinates[from].northing);
            north.add(model.northingParameter[to], 1.0);
            north.add(model.northingParameter[from], -1.0);
            equations.push_back(north);

            Equation east = base;
            east.component = ResidualComponent::Easting;
            east.sigma = baseline->sigmaEasting;
            east.misclosure =
                baseline->deltaEasting - (coordinates[to].easting - coordinates[from].easting);
            east.add(model.eastingParameter[to], 1.0);
            east.add(model.eastingParameter[from], -1.0);
            equations.push_back(east);
        } else if (const auto* position = std::get_if<GnssPositionObservation>(&observation)) {
            const std::size_t point = model.pointOf(position->point);
            Equation north = base;
            north.component = ResidualComponent::Northing;
            north.sigma = position->sigmaNorthing;
            north.misclosure = position->northing - coordinates[point].northing;
            north.add(model.northingParameter[point], 1.0);
            equations.push_back(north);

            Equation east = base;
            east.component = ResidualComponent::Easting;
            east.sigma = position->sigmaEasting;
            east.misclosure = position->easting - coordinates[point].easting;
            east.add(model.eastingParameter[point], 1.0);
            equations.push_back(east);
        }
    }

    // Weighted control: the published coordinate is an observation of the unknown.
    const auto& control = network.controlPoints();
    for (std::size_t c = 0; c < control.size(); ++c) {
        const std::size_t point = model.pointOf(control[c].pointId);
        if (!model.involved[point]) {
            continue;
        }
        const SurveyPoint& published = network.points()[point];
        if (control[c].northing.constraint == ControlConstraint::Weighted) {
            Equation equation;
            equation.source = ResidualSource::Control;
            equation.index = c;
            equation.component = ResidualComponent::Northing;
            equation.sigma = control[c].northing.sigma;
            equation.misclosure = published.northing - coordinates[point].northing;
            equation.add(model.northingParameter[point], 1.0);
            equations.push_back(equation);
        }
        if (control[c].easting.constraint == ControlConstraint::Weighted) {
            Equation equation;
            equation.source = ResidualSource::Control;
            equation.index = c;
            equation.component = ResidualComponent::Easting;
            equation.sigma = control[c].easting.sigma;
            equation.misclosure = published.easting - coordinates[point].easting;
            equation.add(model.eastingParameter[point], 1.0);
            equations.push_back(equation);
        }
    }
    return equations;
}

// Plain-language reason for the most common datum defects. A hint only: the
// authoritative decision is the numerical rank of the design matrix.
std::string horizontalDatumHint(const HorizontalModel& model)
{
    const SurveyNetwork& network = *model.network;
    std::size_t constrainedPoints = 0;
    for (const ControlPoint& control : network.controlPoints()) {
        const bool horizontal = control.northing.constraint != ControlConstraint::Free ||
                                control.easting.constraint != ControlConstraint::Free;
        if (horizontal && model.involved[model.pointOf(control.pointId)]) {
            ++constrainedPoints;
        }
    }
    bool hasOrientation = false;
    for (const std::size_t index : model.usedObservations) {
        const Observation& observation = network.observations()[index];
        if (std::holds_alternative<GnssPositionObservation>(observation)) {
            ++constrainedPoints;
        }
        if (std::holds_alternative<AzimuthObservation>(observation) ||
            std::holds_alternative<GnssBaselineObservation>(observation)) {
            hasOrientation = true;
        }
    }
    if (constrainedPoints == 0) {
        return "No control: the position of the network is undefined; fix or weight at least "
               "one point";
    }
    if (constrainedPoints == 1 && !hasOrientation) {
        return "The orientation of the network is undefined; add an azimuth or a second control "
               "point";
    }
    return "Check for stations that are tied in by too few observations";
}

Result<HorizontalAdjustmentResult> adjustHorizontal(const SurveyNetwork& network,
                                                    const AdjustmentOptions& options)
{
    if (Status status = checkOptions(options); !status) {
        return status.error();
    }
    const HorizontalModel model = buildHorizontalModel(network);
    if (model.usedObservations.empty()) {
        return makeError(ErrorCode::AdjustmentFailure,
                         "the network has no horizontal observations to adjust");
    }
    const std::size_t unknownCount = model.parameters.size();
    if (unknownCount == 0) {
        return makeError(ErrorCode::AdjustmentFailure,
                         "the network has no free coordinates: every observed point is fixed");
    }

    std::vector<Coordinate2> coordinates;
    coordinates.reserve(network.points().size());
    for (const SurveyPoint& point : network.points()) {
        coordinates.push_back(point.position());
    }

    HorizontalAdjustmentResult result;
    bool converged = false;
    while (true) {
        auto equations = linearizeHorizontal(model, coordinates);
        if (!equations) {
            return equations.error();
        }
        if (equations->size() < unknownCount) {
            return makeError(ErrorCode::AdjustmentFailure,
                             "fewer observation equations than unknown coordinates",
                             "equations " + std::to_string(equations->size()) + ", unknowns " +
                                 std::to_string(unknownCount));
        }
        const auto solved = solveEquations(*equations, unknownCount);
        if (const auto* deficiency = std::get_if<detail::RankDeficiency>(&solved)) {
            return rankError(*deficiency, model.parameters, horizontalDatumHint(model));
        }
        const auto& core = std::get<detail::WhitenedSolution>(solved);

        if (converged) {
            // Linearised at the final coordinates: their residuals, cofactors and
            // redundancy numbers are reported; the (sub-threshold) correction of
            // this pass is not applied, so coordinates and residuals agree exactly.
            auto statistics = makeStatistics(*equations, unknownCount, options);
            if (!statistics) {
                return statistics.error();
            }
            result.statistics = *statistics;
            result.residuals = makeResiduals(*equations, core.leverage);
            result.cofactor = detail::toMatrix(core.cofactor);
            result.covariance = detail::toMatrix(core.cofactor * statistics->covarianceScale);
            break;
        }

        double largest = 0.0;
        for (std::size_t p = 0; p < coordinates.size(); ++p) {
            if (model.northingParameter[p] != kNotAParameter) {
                const double correction = core.parameters[model.northingParameter[p]];
                coordinates[p].northing += correction;
                largest = std::max(largest, std::abs(correction));
            }
            if (model.eastingParameter[p] != kNotAParameter) {
                const double correction = core.parameters[model.eastingParameter[p]];
                coordinates[p].easting += correction;
                largest = std::max(largest, std::abs(correction));
            }
        }
        ++result.iterations;
        result.lastCorrection = largest;
        if (!std::isfinite(largest)) {
            return makeError(ErrorCode::AdjustmentFailure,
                             "the adjustment diverged: a coordinate correction is not finite",
                             "iteration " + std::to_string(result.iterations));
        }
        if (largest < options.convergenceThreshold) {
            converged = true;
        } else if (result.iterations >= options.maxIterations) {
            return makeError(ErrorCode::AdjustmentFailure,
                             "the adjustment did not converge; check the approximate coordinates "
                             "and look for gross errors",
                             "iterations " + std::to_string(result.iterations) +
                                 ", last correction " + std::to_string(largest) + " m");
        }
    }

    result.parameters = model.parameters;
    result.unusedObservations = model.unusedObservations;
    for (std::size_t p = 0; p < coordinates.size(); ++p) {
        if (!model.involved[p]) {
            continue;
        }
        AdjustedStation station;
        station.pointId = network.points()[p].id;
        station.position = coordinates[p];
        const Eigen::Index n = model.northingParameter[p];
        const Eigen::Index e = model.eastingParameter[p];
        const auto at = [&](Eigen::Index r, Eigen::Index c) {
            return result.covariance(static_cast<std::size_t>(r), static_cast<std::size_t>(c));
        };
        if (n != kNotAParameter) {
            station.covariance.northing = at(n, n);
        }
        if (e != kNotAParameter) {
            station.covariance.easting = at(e, e);
        }
        if (n != kNotAParameter && e != kNotAParameter) {
            station.covariance.northingEasting = at(n, e);
        }
        station.sigmaNorthing = std::sqrt(station.covariance.northing);
        station.sigmaEasting = std::sqrt(station.covariance.easting);
        station.ellipse = detail::ellipseOf(station.covariance);
        result.stations.push_back(std::move(station));
    }
    return result;
}

// ---- level network -----------------------------------------------------------

struct LevelModel {
    const SurveyNetwork* network = nullptr;
    std::vector<std::size_t> usedObservations;
    std::vector<std::size_t> unusedObservations;
    std::vector<bool> involved;
    std::vector<Eigen::Index> parameter; // per network point
    std::vector<AdjustedParameter> parameters;

    [[nodiscard]] std::size_t pointOf(const std::string& id) const
    {
        return *network->pointIndex(id);
    }
};

LevelModel buildLevelModel(const SurveyNetwork& network)
{
    LevelModel model;
    model.network = &network;
    const std::size_t pointCount = network.points().size();
    model.involved.assign(pointCount, false);
    model.parameter.assign(pointCount, kNotAParameter);

    const auto& observations = network.observations();
    for (std::size_t i = 0; i < observations.size(); ++i) {
        const auto* level = std::get_if<LevelDifferenceObservation>(&observations[i]);
        if (level == nullptr) {
            model.unusedObservations.push_back(i);
            continue;
        }
        model.usedObservations.push_back(i);
        model.involved[model.pointOf(level->from)] = true;
        model.involved[model.pointOf(level->to)] = true;
    }
    for (std::size_t p = 0; p < pointCount; ++p) {
        if (!model.involved[p]) {
            continue;
        }
        const std::string& id = network.points()[p].id;
        const auto control = network.controlFor(id);
        if (!control || control->elevation.constraint != ControlConstraint::Fixed) {
            model.parameter[p] = static_cast<Eigen::Index>(model.parameters.size());
            model.parameters.push_back({id, CoordinateComponent::Elevation});
        }
    }
    return model;
}

std::vector<Equation> levelEquations(const LevelModel& model, const std::vector<double>& elevations)
{
    const SurveyNetwork& network = *model.network;
    std::vector<Equation> equations;
    for (const std::size_t index : model.usedObservations) {
        const auto& level = std::get<LevelDifferenceObservation>(network.observations()[index]);
        const std::size_t from = model.pointOf(level.from);
        const std::size_t to = model.pointOf(level.to);
        Equation equation;
        equation.source = ResidualSource::Observed;
        equation.index = index;
        equation.sigma = level.sigma;
        equation.misclosure = level.heightDifference - (elevations[to] - elevations[from]);
        equation.add(model.parameter[to], 1.0);
        equation.add(model.parameter[from], -1.0);
        equations.push_back(equation);
    }
    const auto& control = network.controlPoints();
    for (std::size_t c = 0; c < control.size(); ++c) {
        const std::size_t point = model.pointOf(control[c].pointId);
        if (!model.involved[point] ||
            control[c].elevation.constraint != ControlConstraint::Weighted) {
            continue;
        }
        Equation equation;
        equation.source = ResidualSource::Control;
        equation.index = c;
        equation.component = ResidualComponent::Elevation;
        equation.sigma = control[c].elevation.sigma;
        equation.misclosure = network.points()[point].elevation - elevations[point];
        equation.add(model.parameter[point], 1.0);
        equations.push_back(equation);
    }
    return equations;
}

Result<LevelAdjustmentResult> adjustLevel(const SurveyNetwork& network,
                                          const AdjustmentOptions& options)
{
    if (Status status = checkOptions(options); !status) {
        return status.error();
    }
    const LevelModel model = buildLevelModel(network);
    if (model.usedObservations.empty()) {
        return makeError(ErrorCode::AdjustmentFailure,
                         "the network has no level differences to adjust");
    }
    const std::size_t unknownCount = model.parameters.size();
    if (unknownCount == 0) {
        return makeError(ErrorCode::AdjustmentFailure,
                         "the network has no free elevations: every levelled point is fixed");
    }

    std::vector<double> elevations;
    elevations.reserve(network.points().size());
    for (const SurveyPoint& point : network.points()) {
        elevations.push_back(point.elevation);
    }

    // The model is linear, so one solve for corrections to the stored elevations
    // is exact whatever those elevations are. Solving for corrections rather than
    // for the elevations keeps the right-hand side small.
    std::vector<Equation> equations = levelEquations(model, elevations);
    if (equations.size() < unknownCount) {
        return makeError(ErrorCode::AdjustmentFailure,
                         "fewer level differences than unknown elevations",
                         "equations " + std::to_string(equations.size()) + ", unknowns " +
                             std::to_string(unknownCount));
    }
    const auto solved = solveEquations(equations, unknownCount);
    if (const auto* deficiency = std::get_if<detail::RankDeficiency>(&solved)) {
        return rankError(*deficiency, model.parameters,
                         "Every connected part of a level network needs a fixed or weighted "
                         "benchmark");
    }
    const auto& core = std::get<detail::WhitenedSolution>(solved);
    for (std::size_t p = 0; p < elevations.size(); ++p) {
        if (model.parameter[p] != kNotAParameter) {
            elevations[p] += core.parameters[model.parameter[p]];
        }
    }

    // Residuals from the adjusted elevations themselves, not from A x - l.
    equations = levelEquations(model, elevations);
    auto statistics = makeStatistics(equations, unknownCount, options);
    if (!statistics) {
        return statistics.error();
    }

    LevelAdjustmentResult result;
    result.statistics = *statistics;
    result.residuals = makeResiduals(equations, core.leverage);
    result.unusedObservations = model.unusedObservations;
    result.parameters = model.parameters;
    result.cofactor = detail::toMatrix(core.cofactor);
    result.covariance = detail::toMatrix(core.cofactor * statistics->covarianceScale);
    for (std::size_t p = 0; p < elevations.size(); ++p) {
        if (!model.involved[p]) {
            continue;
        }
        AdjustedElevation adjusted;
        adjusted.pointId = network.points()[p].id;
        adjusted.elevation = elevations[p];
        if (model.parameter[p] != kNotAParameter) {
            const auto k = static_cast<std::size_t>(model.parameter[p]);
            adjusted.sigma = std::sqrt(result.covariance(k, k));
        }
        result.elevations.push_back(std::move(adjusted));
    }
    return result;
}

Status checkStochasticModel(const Traverse& traverse, const TraverseStochasticModel& model)
{
    const auto nonNegative = [](double value) { return std::isfinite(value) && value >= 0.0; };
    if (!nonNegative(model.distanceSigmaConstant) || !nonNegative(model.distanceSigmaPpm) ||
        !(model.distanceSigmaConstant > 0.0 || model.distanceSigmaPpm > 0.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         "distance sigma needs a non-negative constant and ppm part, at least "
                         "one of them positive");
    }
    if (!std::isfinite(model.angleSigma) || !(model.angleSigma > 0.0)) {
        return makeError(ErrorCode::InvalidArgument, "angle sigma must be positive and finite");
    }
    if (!nonNegative(model.azimuthSigma)) {
        return makeError(ErrorCode::InvalidArgument,
                         "azimuth sigma must be finite and not negative");
    }
    if (traverse.kind == TraverseKind::ClosedLoop && !(model.azimuthSigma > 0.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         "a closed loop needs a positive azimuth sigma: its first-leg azimuth "
                         "enters the adjustment as an observation");
    }
    return {};
}

} // namespace

Result<HorizontalAdjustmentResult> adjustHorizontalNetwork(const SurveyNetwork& network,
                                                           const AdjustmentOptions& options)
{
    auto result = adjustHorizontal(network, options);
    if (result) {
        logOutcome(options, "horizontal network adjusted", result->statistics, result->iterations);
    } else {
        logFailure(options, "horizontal network adjustment failed", result.error());
    }
    return result;
}

Result<LevelAdjustmentResult> adjustLevelNetwork(const SurveyNetwork& network,
                                                 const AdjustmentOptions& options)
{
    auto result = adjustLevel(network, options);
    if (result) {
        logOutcome(options, "level network adjusted", result->statistics, 1);
    } else {
        logFailure(options, "level network adjustment failed", result.error());
    }
    return result;
}

Status applyAdjustment(SurveyNetwork& network, const HorizontalAdjustmentResult& result)
{
    // Validate everything first so that a failure leaves the network untouched.
    std::vector<SurveyPoint> updated;
    updated.reserve(result.stations.size());
    for (const AdjustedStation& station : result.stations) {
        auto point = network.point(station.pointId);
        if (!point) {
            return point.error();
        }
        point->northing = station.position.northing;
        point->easting = station.position.easting;
        updated.push_back(std::move(*point));
    }
    for (SurveyPoint& point : updated) {
        if (Status status = network.updatePoint(std::move(point)); !status) {
            return status;
        }
    }
    return {};
}

Status applyAdjustment(SurveyNetwork& network, const LevelAdjustmentResult& result)
{
    std::vector<SurveyPoint> updated;
    updated.reserve(result.elevations.size());
    for (const AdjustedElevation& adjusted : result.elevations) {
        auto point = network.point(adjusted.pointId);
        if (!point) {
            return point.error();
        }
        point->elevation = adjusted.elevation;
        updated.push_back(std::move(*point));
    }
    for (SurveyPoint& point : updated) {
        if (Status status = network.updatePoint(std::move(point)); !status) {
            return status;
        }
    }
    return {};
}

Result<SurveyNetwork> buildTraverseNetwork(const Traverse& traverse,
                                           const TraverseStochasticModel& model)
{
    if (Status status = checkStochasticModel(traverse, model); !status) {
        return status.error();
    }
    // Compass-rule coordinates are the approximate values: already within the
    // misclosure of the least-squares solution.
    const auto preliminary = computeTraverse(traverse, TraverseOptions{});
    if (!preliminary) {
        return preliminary.error();
    }
    const auto& setups = traverse.setups;
    const bool isLoop = traverse.kind == TraverseKind::ClosedLoop;
    const bool closesOnStart =
        !isLoop && traverse.endStationId == setups.front().stationId; // link back to its start

    SurveyNetwork network;
    for (std::size_t s = 0; s < preliminary->stations.size(); ++s) {
        const TraverseStationResult& station = preliminary->stations[s];
        if (closesOnStart && s + 1 == preliminary->stations.size()) {
            break; // the end station is the start point, already present
        }
        SurveyPoint point;
        point.id = station.id;
        point.northing = station.adjusted.northing;
        point.easting = station.adjusted.easting;
        if (Status status = network.addPoint(std::move(point)); !status) {
            return status.error();
        }
    }
    if (Status status =
            network.addControlPoint(ControlPoint::fixedHorizontal(setups.front().stationId));
        !status) {
        return status.error();
    }
    if (traverse.kind == TraverseKind::Link && !closesOnStart) {
        if (Status status =
                network.addControlPoint(ControlPoint::fixedHorizontal(traverse.endStationId));
            !status) {
            return status.error();
        }
    }

    const auto forwardStation = [&](std::size_t k) -> const std::string& {
        if (k + 1 < setups.size()) {
            return setups[k + 1].stationId;
        }
        return isLoop ? setups.front().stationId : traverse.endStationId;
    };
    const double referencedAngleSigma = std::hypot(model.angleSigma, model.azimuthSigma);

    for (std::size_t k = 0; k < setups.size(); ++k) {
        DistanceObservation distance;
        distance.from = setups[k].stationId;
        distance.to = forwardStation(k);
        distance.distance = setups[k].distance;
        distance.sigma = std::hypot(model.distanceSigmaConstant,
                                    model.distanceSigmaPpm * kPartsPerMillion * setups[k].distance);
        if (auto added = network.addObservation(distance); !added) {
            return added.error();
        }

        if (k == 0 && !isLoop) {
            // Angle turned from a reference direction of known azimuth: equivalent
            // to observing the azimuth of the first leg.
            AzimuthObservation azimuth;
            azimuth.from = setups[0].stationId;
            azimuth.to = forwardStation(0);
            azimuth.azimuth = normalizeAngle(traverse.startAzimuth + setups[0].angle);
            azimuth.sigma = referencedAngleSigma;
            if (auto added = network.addObservation(azimuth); !added) {
                return added.error();
            }
            continue;
        }
        if (k == 0) {
            AzimuthObservation azimuth;
            azimuth.from = setups[0].stationId;
            azimuth.to = forwardStation(0);
            azimuth.azimuth = normalizeAngle(traverse.startAzimuth);
            azimuth.sigma = model.azimuthSigma;
            if (auto added = network.addObservation(azimuth); !added) {
                return added.error();
            }
        }
        HorizontalAngleObservation angle;
        angle.at = setups[k].stationId;
        angle.from = k == 0 ? setups.back().stationId : setups[k - 1].stationId;
        angle.to = forwardStation(k);
        angle.angle = setups[k].angle;
        angle.sigma = model.angleSigma;
        if (auto added = network.addObservation(angle); !added) {
            return added.error();
        }
    }

    if (traverse.kind == TraverseKind::Link && traverse.closingAngle) {
        // closing angle = azimuth(end->reference) - azimuth(end->last station).
        AzimuthObservation azimuth;
        azimuth.from = traverse.endStationId;
        azimuth.to = setups.back().stationId;
        azimuth.azimuth = normalizeAngle(traverse.closingAngle->referenceAzimuth -
                                         traverse.closingAngle->angle);
        azimuth.sigma = referencedAngleSigma;
        if (auto added = network.addObservation(azimuth); !added) {
            return added.error();
        }
    }
    return network;
}

Result<HorizontalAdjustmentResult> adjustTraverse(const Traverse& traverse,
                                                  const TraverseStochasticModel& model,
                                                  const AdjustmentOptions& options)
{
    const auto network = buildTraverseNetwork(traverse, model);
    if (!network) {
        return network.error();
    }
    return adjustHorizontalNetwork(*network, options);
}

} // namespace katana::survey
