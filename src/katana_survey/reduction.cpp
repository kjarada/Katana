// reduceAndAdjust: raw total-station and GNSS observations to coordinates,
// with every step written into the report (reduction.hpp has the order).
//
// Phase A, per setup and independent of any coordinate: group the
// observations of each pointing, correct each distance (prism constant, then
// atmospheric ppm), pair face left with face right and mean them, reduce slope
// to horizontal with curvature and refraction, and form the height difference
// mark to mark.
//
// Phase B, setup by setup as their stations become known: orient the circle
// on the backsight, apply the factors that need a position (height reduction,
// grid scale at the line's mid-point, or the combined factor), and radiate the
// targets. A target already positioned becomes a check instead.
//
// Then the adjustment the settings ask for (reduction_adjust.cpp), and the
// outputs: the computed points, the reduced project and the report.

#include "katana/survey/reduction.hpp"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <functional>
#include <queue>
#include <string>
#include <type_traits>
#include <unordered_set>
#include <utility>
#include <variant>

#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"
#include "reduction_engine.hpp"
#include "reduction_formulas.hpp"

namespace katana::survey {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::math::kHalfPi;
using katana::math::kPi;
using katana::math::kTwoPi;
using katana::math::normalizeAngle;
using katana::math::normalizeAngleSigned;

namespace detail {

std::string formatNumber(double value, int decimals)
{
    char buffer[64];
    std::snprintf(buffer, sizeof buffer, "%.*f", decimals, value);
    return buffer;
}

std::string formatSeconds(double radians, int decimals)
{
    return formatNumber(radians * 648000.0 / kPi, decimals) + "\"";
}

std::string formatMillimetres(double metres, int decimals)
{
    return formatNumber(metres * 1000.0, decimals) + " mm";
}

namespace {

constexpr std::size_t kNoRow = static_cast<std::size_t>(-1);

// ---- What the instrument did to its distances -----------------------------------------

// The atmospheric correction one setup's distances get: a factor, and why.
struct AtmosphereDecision {
    bool apply = false;
    double ppm = 0.0;       // net ppm applied (after taking out the instrument's)
    std::string note;       // short: it is copied onto every distance of the setup
};

AtmosphereDecision decideAtmosphere(Engine& engine, const SurveyStation& station)
{
    const ReductionSettings& settings = engine.settings;
    const InstrumentSettings& instrument = station.instrument;
    AtmosphereDecision decision;

    std::optional<double> computed;
    if (instrument.temperatureCelsius && instrument.pressureHectopascals) {
        double humidity = kAssumedHumidityPercent;
        if (instrument.relativeHumidityPercent) {
            humidity = *instrument.relativeHumidityPercent;
        } else {
            engine.warnSetup(station, "no humidity recorded; 60 % was assumed for the "
                                      "atmospheric correction (under 1 ppm below 25 C).");
        }
        computed = atmosphericPpm(*instrument.temperatureCelsius,
                                  *instrument.pressureHectopascals, humidity);
    }
    const auto ppmText = [](double ppm) {
        return (ppm >= 0.0 ? "+" : "") + formatNumber(ppm, 1) + " ppm";
    };

    switch (settings.atmospheric) {
    case AtmosphericCorrection::None:
        return decision;
    case AtmosphericCorrection::Auto:
        if (instrument.atmosphericPpmState == CorrectionState::Applied) {
            return decision; // the instrument did it; the setup record says so
        }
        if (instrument.atmosphericPpmState == CorrectionState::Unknown) {
            engine.warnSetup(station, "the file does not say whether the instrument applied an "
                            "atmospheric correction, so none was applied. Choose Recompute or "
                            "Fixed if the distances are raw.");
            return decision;
        }
        if (computed) {
            // The note names the value; the setup row names the weather.
            decision = {true, *computed, ppmText(*computed)};
            return decision;
        }
        if (instrument.atmosphericPpm) {
            decision = {true, *instrument.atmosphericPpm,
                        ppmText(*instrument.atmosphericPpm) + " as recorded"};
            return decision;
        }
        engine.warnSetup(station, "the instrument did not apply an atmospheric correction and the file "
                        "records no weather to compute one, so none was applied.");
        return decision;
    case AtmosphericCorrection::Recompute: {
        if (!computed) {
            engine.warnSetup(station, "no temperature and pressure recorded, so the atmospheric "
                            "correction cannot be recomputed; distances used as recorded.");
            return decision;
        }
        double net = *computed;
        std::string note = ppmText(*computed);
        if (instrument.atmosphericPpmState == CorrectionState::Applied) {
            if (!instrument.atmosphericPpm) {
                engine.warnSetup(station, "the instrument applied an atmospheric correction but the file "
                                "does not say how much, so it cannot be taken out; distances "
                                "used as recorded.");
                return decision;
            }
            // (1 + new) / (1 + old), as ppm.
            net = ((1.0 + *computed * 1e-6) / (1.0 + *instrument.atmosphericPpm * 1e-6) - 1.0) *
                  1e6;
            note = ppmText(*computed) + " for " + ppmText(*instrument.atmosphericPpm);
        } else if (instrument.atmosphericPpmState == CorrectionState::Unknown) {
            engine.warnSetup(station, "the file does not say whether the instrument applied an "
                            "atmospheric correction; the recomputed one was applied as if it had "
                            "not.");
        }
        return AtmosphereDecision{true, net, note};
    }
    case AtmosphericCorrection::Fixed:
        if (instrument.atmosphericPpmState == CorrectionState::Applied) {
            return decision;
        }
        if (instrument.atmosphericPpmState == CorrectionState::Unknown) {
            engine.warnSetup(station, "the file does not say whether the instrument applied an "
                            "atmospheric correction; the fixed value was applied as if it had "
                            "not.");
        }
        return AtmosphereDecision{true, settings.fixedPpm, ppmText(settings.fixedPpm) + " fixed"};
    }
    return decision;
}

bool isPrismTarget(const TargetInfo& target)
{
    std::string type;
    type.reserve(target.targetType.size());
    for (const char c : target.targetType) {
        type += static_cast<char>(c >= 'A' && c <= 'Z' ? c - 'A' + 'a' : c);
    }
    return type.find("reflectorless") == std::string::npos &&
           type.find("non-prism") == std::string::npos &&
           type.find("nonprism") == std::string::npos && type.find("tape") == std::string::npos;
}

// The prism constant correction of one distance, in metres, and its note.
// `warned` keeps the "not stated" warning to one per setup.
std::optional<AppliedCorrection> prismCorrection(Engine& engine, const SurveyStation& station,
                                                 const DistanceObservation& distance,
                                                 bool& warned)
{
    const ReductionSettings& settings = engine.settings;
    if (settings.prismConstantPolicy == PrismConstantPolicy::None ||
        !isPrismTarget(distance.target)) {
        return std::nullopt;
    }
    // The shot's own record wins over the setup's: the prism may have been
    // changed mid setup.
    const bool shotStates = distance.target.prismConstant.has_value() ||
                            distance.target.prismConstantState != CorrectionState::Unknown;
    const std::optional<double> constant =
        shotStates ? distance.target.prismConstant : station.instrument.prismConstant;
    const CorrectionState state = shotStates ? distance.target.prismConstantState
                                             : station.instrument.prismConstantState;
    const auto notStated = [&](const char* what) {
        if (!warned) {
            warned = true;
            engine.warnSetup(station, what);
        }
    };

    if (settings.prismConstantPolicy == PrismConstantPolicy::Auto) {
        if (state == CorrectionState::NotApplied) {
            if (!constant) {
                notStated("the file says the prism constant is not in the distances but not "
                          "what it is; distances used as recorded.");
                return std::nullopt;
            }
            return AppliedCorrection{CorrectionKind::PrismConstant, *constant, std::nullopt,
                                     "recorded"};
        }
        if (state == CorrectionState::Unknown) {
            notStated("the file does not say whether the prism constant is in the distances; "
                      "none was added.");
        }
        return std::nullopt;
    }
    // Override: take out what the instrument applied, add the chosen constant.
    double amount = settings.prismConstant;
    if (state == CorrectionState::Applied) {
        if (!constant) {
            notStated("the instrument applied a prism constant the file does not state, so it "
                      "cannot be replaced; distances used as recorded.");
            return std::nullopt;
        }
        amount -= *constant;
    } else if (state == CorrectionState::Unknown) {
        notStated("the file does not say whether the prism constant is in the distances; the "
                  "chosen constant was added as if it were not.");
    }
    return AppliedCorrection{CorrectionKind::PrismConstant, amount, std::nullopt,
                             state == CorrectionState::Applied ? "replaced" : "chosen"};
}

// ---- Phase A: one setup ----------------------------------------------------------------

// The observations of one pointing.
struct Shot {
    std::string_view target;
    Face face = Face::Unknown;
    std::size_t pointingIndex = 0;
    const HorizontalDirectionObservation* direction = nullptr;
    const ZenithAngleObservation* zenith = nullptr;
    const VerticalAngleObservation* vertical = nullptr;
    const DistanceObservation* distance = nullptr;
    // Report rows.
    std::size_t directionRow = kNoRow;
    std::size_t zenithRow = kNoRow;
    std::size_t distanceRow = kNoRow;
    // After prism and atmosphere.
    double correctedDistance = 0.0;
    // FL-equivalent zenith (vertical angle converted, face right folded).
    double zenithValue = 0.0;
};

ReportObservation& row(Engine& engine, std::size_t index)
{
    return engine.report.observations[index];
}

std::size_t addRow(Engine& engine, const SurveyStation& station, std::string_view kind,
                   std::string_view to, const Pointing& pointing, bool angular, double raw,
                   const SourceRecord& source)
{
    ReportObservation observation;
    observation.stationId = station.setup.id;
    observation.kind = kind;
    observation.from = station.setup.pointId;
    observation.to = to;
    observation.pointing = pointing;
    observation.angular = angular;
    observation.raw = raw;
    observation.reduced = raw;
    observation.source = source;
    // One allocation for the whole chain instead of one per doubling: a
    // distance collects up to six corrections, and at 100 000 rows the
    // reallocations were a measurable share of the reduction.
    observation.corrections.reserve(6);
    engine.report.observations.push_back(std::move(observation));
    return engine.report.observations.size() - 1;
}

void correct(Engine& engine, std::size_t rowIndex, CorrectionKind kind, double amount,
             std::optional<double> factor = std::nullopt, std::string note = {})
{
    ReportObservation& observation = row(engine, rowIndex);
    observation.corrections.push_back(AppliedCorrection{kind, amount, factor, std::move(note)});
    if (observation.reduced) {
        *observation.reduced += amount;
    }
}

void setReduced(Engine& engine, std::size_t rowIndex, double value)
{
    row(engine, rowIndex).reduced = value;
}

void reject(Engine& engine, std::size_t rowIndex, std::string reason)
{
    ReportObservation& observation = row(engine, rowIndex);
    observation.rejected = true;
    observation.rejectionReason = std::move(reason);
}

void rejectShot(Engine& engine, const Shot& shot, const std::string& reason)
{
    for (const std::size_t index : {shot.directionRow, shot.zenithRow, shot.distanceRow}) {
        if (index != kNoRow) {
            reject(engine, index, reason);
        }
    }
}

// Adds the rows of one shot to a pointing's bookkeeping.
void collectRows(ReducedPointing& pointing, const Shot& shot)
{
    for (const std::size_t index : {shot.directionRow, shot.zenithRow, shot.distanceRow}) {
        if (index != kNoRow && pointing.rawRowCount < pointing.rawRows.size()) {
            pointing.rawRows[pointing.rawRowCount++] = index;
        }
    }
    if (shot.directionRow != kNoRow) {
        pointing.directionRows[pointing.directionRowCount++] = shot.directionRow;
    }
    if (shot.distanceRow != kNoRow) {
        pointing.distanceRows[pointing.distanceRowCount++] = shot.distanceRow;
    }
}

const SourceRecord* shotSource(const Shot& shot)
{
    if (shot.direction) {
        return &shot.direction->source;
    }
    if (shot.distance) {
        return &shot.distance->source;
    }
    if (shot.zenith) {
        return &shot.zenith->source;
    }
    return shot.vertical ? &shot.vertical->source : nullptr;
}

// Face-right single: the reading less 180 degrees, the zenith folded.
void reduceSingle(Engine& engine, ReducedPointing& pointing, const Shot& shot)
{
    const ReductionSettings& settings = engine.settings;
    collectRows(pointing, shot);
    pointing.face = shot.face;
    pointing.leftIndex = shot.pointingIndex;
    pointing.source = shotSource(shot);
    if (shot.direction) {
        double direction = shot.direction->direction;
        if (shot.face == Face::Right) {
            direction = normalizeAngle(direction - kPi);
            correct(engine, shot.directionRow, CorrectionKind::FaceMean, -kPi, std::nullopt,
                    "face right only");
        }
        pointing.direction = normalizeAngle(direction);
        setReduced(engine, shot.directionRow, *pointing.direction);
        pointing.sigmaDirection = settings.apriori.direction;
    }
    if (shot.zenith || shot.vertical) {
        pointing.zenith = shot.zenithValue;
        pointing.sigmaZenith = settings.apriori.zenith;
    }
    if (shot.distance) {
        pointing.slope = shot.correctedDistance;
        pointing.sigmaDistance = distanceSigma(settings.apriori, shot.correctedDistance);
        pointing.recordedHorizontal = shot.distance->kind == DistanceKind::Horizontal;
        pointing.targetHeight = shot.distance->targetHeight;
    } else if (shot.zenith) {
        pointing.targetHeight = shot.zenith->targetHeight;
    } else if (shot.vertical) {
        pointing.targetHeight = shot.vertical->targetHeight;
    }
}

// Face left with face right: the means, the spreads, and the check.
void reducePair(Engine& engine, std::size_t setupIndex, ReducedPointing& pointing,
                const Shot& left, const Shot& right)
{
    const ReductionSettings& settings = engine.settings;
    const SurveyStation& station = engine.raw.stations[setupIndex];
    collectRows(pointing, left);
    collectRows(pointing, right);
    pointing.paired = true;
    pointing.face = Face::Left;
    pointing.leftIndex = left.pointingIndex;
    pointing.rightIndex = right.pointingIndex;
    pointing.source = shotSource(left);

    FacePairCheck check;
    check.stationId = station.setup.id;
    check.targetId = left.target;
    check.leftPointing = left.pointingIndex;
    check.rightPointing = right.pointingIndex;
    if (const SourceRecord* source = shotSource(left)) {
        check.left = *source;
    }
    if (const SourceRecord* source = shotSource(right)) {
        check.right = *source;
    }

    // Horizontal: FL and FR - 180, meaned by half the wrapped difference so
    // that readings either side of zero mean correctly.
    if (left.direction && right.direction) {
        const double l = left.direction->direction;
        double r = right.direction->direction;
        // A reading half a turn from its partner is raw face right; one within
        // a quarter turn was already reduced by the file.
        const bool folded = std::abs(normalizeAngleSigned(r - l)) > kHalfPi;
        if (folded) {
            r -= kPi;
        }
        const double difference = normalizeAngleSigned(r - l);
        const double mean = normalizeAngle(l + 0.5 * difference);
        check.horizontalSpread = std::abs(difference);
        pointing.direction = mean;
        pointing.sigmaDirection = settings.apriori.direction / std::sqrt(2.0);
        correct(engine, left.directionRow, CorrectionKind::FaceMean,
                normalizeAngleSigned(mean - l));
        // Written as -180 deg and the small part, the way it is worked in a
        // field book, rather than wrapped to +179 59 58.
        correct(engine, right.directionRow, CorrectionKind::FaceMean,
                (folded ? -kPi : 0.0) + normalizeAngleSigned(mean - r), std::nullopt,
                folded ? "less 180 deg" : "");
        setReduced(engine, left.directionRow, mean);
        setReduced(engine, right.directionRow, mean);
    } else if (left.direction || right.direction) {
        const Shot& one = left.direction ? left : right;
        double direction = one.direction->direction;
        if (&one == &right) {
            direction = normalizeAngle(direction - kPi);
            correct(engine, one.directionRow, CorrectionKind::FaceMean, -kPi, std::nullopt,
                    "face right only");
        }
        pointing.direction = normalizeAngle(direction);
        pointing.sigmaDirection = settings.apriori.direction;
        setReduced(engine, one.directionRow, *pointing.direction);
    }

    // Zenith: the mean of FL and the folded FR removes the index error
    // i = (FL - FR') / 2 (with FR' = 360 - FR).
    const bool leftZenith = left.zenith || left.vertical;
    const bool rightZenith = right.zenith || right.vertical;
    if (leftZenith && rightZenith) {
        const double mean = 0.5 * (left.zenithValue + right.zenithValue);
        check.zenithSpread = std::abs(left.zenithValue - right.zenithValue);
        pointing.zenith = mean;
        pointing.sigmaZenith = settings.apriori.zenith / std::sqrt(2.0);
        const double leftRaw = left.zenith ? left.zenith->angle : left.vertical->angle;
        const double rightRaw = right.zenith ? right.zenith->angle : right.vertical->angle;
        const double leftBefore = row(engine, left.zenithRow).reduced.value_or(leftRaw);
        const double rightBefore = row(engine, right.zenithRow).reduced.value_or(rightRaw);
        // The face-left row's amount is minus the index error.
        correct(engine, left.zenithRow, CorrectionKind::FaceMean, mean - leftBefore);
        correct(engine, right.zenithRow, CorrectionKind::FaceMean, mean - rightBefore);
        setReduced(engine, left.zenithRow, mean);
        setReduced(engine, right.zenithRow, mean);
    } else if (leftZenith || rightZenith) {
        pointing.zenith = leftZenith ? left.zenithValue : right.zenithValue;
        pointing.sigmaZenith = settings.apriori.zenith;
    }

    // Distance: the mean of the two corrected distances.
    if (left.distance && right.distance) {
        const double mean = 0.5 * (left.correctedDistance + right.correctedDistance);
        check.distanceSpread = std::abs(left.correctedDistance - right.correctedDistance);
        pointing.slope = mean;
        pointing.sigmaDistance = distanceSigma(settings.apriori, mean);
        pointing.recordedHorizontal = left.distance->kind == DistanceKind::Horizontal;
        pointing.targetHeight = 0.5 * (left.distance->targetHeight + right.distance->targetHeight);
        correct(engine, left.distanceRow, CorrectionKind::FaceMean, mean - left.correctedDistance);
        correct(engine, right.distanceRow, CorrectionKind::FaceMean,
                mean - right.correctedDistance);
    } else if (left.distance || right.distance) {
        const Shot& one = left.distance ? left : right;
        pointing.slope = one.correctedDistance;
        pointing.sigmaDistance = distanceSigma(settings.apriori, one.correctedDistance);
        pointing.recordedHorizontal = one.distance->kind == DistanceKind::Horizontal;
        pointing.targetHeight = one.distance->targetHeight;
    } else if (left.zenith) {
        pointing.targetHeight = left.zenith->targetHeight;
    }

    const FaceTolerances& tolerance = settings.faceTolerances;
    check.withinTolerance =
        (!check.horizontalSpread || *check.horizontalSpread <= tolerance.horizontal) &&
        (!check.zenithSpread || *check.zenithSpread <= tolerance.zenith) &&
        (!check.distanceSpread || *check.distanceSpread <= tolerance.distance);
    if (!check.withinTolerance) {
        std::string what;
        const auto add = [&what](const std::string& part) {
            what += what.empty() ? "" : ", ";
            what += part;
        };
        if (check.horizontalSpread && *check.horizontalSpread > tolerance.horizontal) {
            add("horizontal " + formatSeconds(*check.horizontalSpread));
        }
        if (check.zenithSpread && *check.zenithSpread > tolerance.zenith) {
            add("zenith " + formatSeconds(*check.zenithSpread));
        }
        if (check.distanceSpread && *check.distanceSpread > tolerance.distance) {
            add("distance " + formatMillimetres(*check.distanceSpread));
        }
        const std::string text = "face pair outside tolerance (" + what + ")";
        if (tolerance.excludeOutside) {
            pointing.rejected = true;
            rejectShot(engine, left, text);
            rejectShot(engine, right, text);
            engine.warn("Setup " + station.setup.id + ", " + std::string(left.target) + ": " +
                            text + "; the pair was excluded.",
                        check.left);
        } else {
            engine.warn("Setup " + station.setup.id + ", " + std::string(left.target) + ": " +
                            text + "; the pair was used.",
                        check.left);
        }
    }
    engine.report.facePairs.push_back(std::move(check));
}

// Slope to horizontal, curvature and refraction, and the height difference.
void reduceToHorizontal(Engine& engine, std::size_t setupIndex, ReducedPointing& pointing,
                        bool& warnedSlope)
{
    const ReductionSettings& settings = engine.settings;
    const SurveyStation& station = engine.raw.stations[setupIndex];
    if (pointing.rejected || !pointing.slope) {
        return;
    }
    const double instrumentHeight = station.setup.instrumentHeight;
    if (pointing.recordedHorizontal) {
        pointing.horizontal = *pointing.slope;
        if (pointing.zenith && std::sin(*pointing.zenith) > 0.0) {
            // A recorded horizontal distance with its zenith still gives a height.
            const double vertical = *pointing.slope / std::tan(*pointing.zenith);
            pointing.measuredVertical = vertical;
            const SlopeReduction curvature =
                reduceSlope(std::hypot(vertical, *pointing.slope), *pointing.zenith,
                            settings.curvatureAndRefraction, settings.refractionCoefficient,
                            settings.earthRadius);
            pointing.heightDifference = vertical + curvature.verticalCorrection +
                                        instrumentHeight - pointing.targetHeight;
        }
        return;
    }
    if (!pointing.zenith) {
        for (std::size_t i = 0; i < pointing.distanceRowCount; ++i) {
            reject(engine, pointing.distanceRows[i],
                   "no zenith angle in the same pointing to reduce it to horizontal");
        }
        pointing.slope.reset();
        return;
    }
    if (!settings.slopeToHorizontal) {
        if (!warnedSlope) {
            warnedSlope = true;
            engine.warn("Setup " + station.setup.id +
                            ": slope to horizontal is off, so its slope distances stay slope "
                            "distances and position nothing.",
                        station.source);
        }
        return;
    }

    const SlopeReduction reduction =
        reduceSlope(*pointing.slope, *pointing.zenith, settings.curvatureAndRefraction,
                    settings.refractionCoefficient, settings.earthRadius);
    pointing.measuredVertical = reduction.measuredVertical;
    pointing.horizontal = reduction.measuredHorizontal + reduction.horizontalCorrection;
    for (std::size_t i = 0; i < pointing.distanceRowCount; ++i) {
        const std::size_t index = pointing.distanceRows[i];
        correct(engine, index, CorrectionKind::SlopeToHorizontal,
                reduction.measuredHorizontal - *pointing.slope);
        if (settings.curvatureAndRefraction) {
            correct(engine, index, CorrectionKind::CurvatureRefraction,
                    reduction.horizontalCorrection);
        }
        setReduced(engine, index, *pointing.horizontal);
    }

    // The height difference mark to mark, as a row of its own: it is derived
    // from the slope distance and the zenith of this pointing together.
    pointing.heightDifference = reduction.measuredVertical + reduction.verticalCorrection +
                                instrumentHeight - pointing.targetHeight;
    ReportObservation height;
    height.stationId = station.setup.id;
    height.kind = "height difference";
    height.from = station.setup.pointId;
    height.to = pointing.target;
    height.pointing = Pointing{pointing.leftIndex, pointing.face};
    height.raw = reduction.measuredVertical;
    height.corrections.reserve(2);
    if (settings.curvatureAndRefraction) {
        height.corrections.push_back(AppliedCorrection{CorrectionKind::CurvatureRefraction,
                                                       reduction.verticalCorrection,
                                                       std::nullopt, {}});
    }
    height.corrections.push_back(AppliedCorrection{CorrectionKind::InstrumentAndTargetHeight,
                                                   instrumentHeight - pointing.targetHeight,
                                                   std::nullopt, {}});
    height.reduced = pointing.heightDifference;
    if (pointing.source) {
        height.source = *pointing.source;
    }
    engine.report.observations.push_back(std::move(height));
    pointing.heightRow = engine.report.observations.size() - 1;
}

// Everything that is not a pointing: kept as recorded, with a row that says so.
void recordLoose(Engine& engine, const Observation& observation, std::size_t setupIndex,
                 const std::string& stationId)
{
    ReportObservation reportRow;
    reportRow.stationId = stationId;
    reportRow.kind = observationKindName(observation);
    reportRow.source = observationSource(observation);
    std::visit(
        [&](const auto& o) {
            using T = std::decay_t<decltype(o)>;
            if constexpr (std::is_same_v<T, DistanceObservation>) {
                reportRow.from = o.from;
                reportRow.to = o.to;
                reportRow.raw = o.distance;
            } else if constexpr (std::is_same_v<T, HorizontalAngleObservation>) {
                reportRow.from = o.at;
                reportRow.to = o.to;
                reportRow.angular = true;
                reportRow.raw = o.angle;
                engine.angles.push_back(RecordedAngle{setupIndex, o.at, o.from, o.to, o.angle,
                                                      o.sigma,
                                                      engine.report.observations.size()});
            } else if constexpr (std::is_same_v<T, AzimuthObservation>) {
                reportRow.from = o.from;
                reportRow.to = o.to;
                reportRow.angular = true;
                reportRow.raw = o.azimuth;
                engine.angles.push_back(RecordedAngle{setupIndex, o.from, {}, o.to, o.azimuth,
                                                      o.sigma,
                                                      engine.report.observations.size()});
            } else if constexpr (std::is_same_v<T, LevelDifferenceObservation>) {
                reportRow.from = o.from;
                reportRow.to = o.to;
                reportRow.raw = o.heightDifference;
            } else if constexpr (std::is_same_v<T, GnssBaselineObservation> ||
                                 std::is_same_v<T, GnssGeocentricBaselineObservation>) {
                reportRow.from = o.from;
                reportRow.to = o.to;
            } else if constexpr (std::is_same_v<T, GnssPositionObservation> ||
                                 std::is_same_v<T, GnssGlobalPositionObservation>) {
                reportRow.to = o.point;
            } else if constexpr (std::is_same_v<T, HorizontalDirectionObservation>) {
                reportRow.from = o.at;
                reportRow.to = o.to;
                reportRow.angular = true;
                reportRow.raw = o.direction;
            } else if constexpr (std::is_same_v<T, ZenithAngleObservation> ||
                                 std::is_same_v<T, VerticalAngleObservation>) {
                reportRow.from = o.from;
                reportRow.to = o.to;
                reportRow.angular = true;
                reportRow.raw = o.angle;
            }
        },
        observation);
    reportRow.reduced = reportRow.raw;
    engine.report.observations.push_back(std::move(reportRow));
}

void reduceSetup(Engine& engine, std::size_t setupIndex, std::vector<Shot>& shots,
                 std::unordered_map<std::size_t, std::size_t>& byPointing)
{
    const ReductionSettings& settings = engine.settings;
    const SurveyStation& station = engine.raw.stations[setupIndex];
    SetupState& state = engine.setups[setupIndex];

    ReportSetup setup;
    setup.stationId = station.setup.id;
    setup.pointId = station.setup.pointId;
    setup.instrumentHeight = station.setup.instrumentHeight;
    setup.backsightPointId = station.backsightPointId;
    setup.instrument = station.instrument;
    setup.source = station.source;
    engine.report.setups.push_back(std::move(setup));
    state.reportIndex = engine.report.setups.size() - 1;
    engine.occupiedBy[station.setup.pointId].push_back(setupIndex);

    const AtmosphereDecision atmosphere = decideAtmosphere(engine, station);
    bool warnedPrism = false;
    bool warnedSlope = false;

    // Group by pointing, in the order of the file.
    shots.clear();
    byPointing.clear();
    const auto shotFor = [&](const Pointing& pointing, std::string_view target) -> Shot& {
        if (pointing.index != 0) {
            const auto [it, inserted] = byPointing.try_emplace(pointing.index, shots.size());
            if (!inserted && shots[it->second].target == target) {
                return shots[it->second];
            }
            if (!inserted) {
                // One pointing index naming two targets: the file is
                // inconsistent; the second is a shot of its own.
                engine.warn("Setup " + station.setup.id + ": pointing " +
                                std::to_string(pointing.index) + " names two targets; they were "
                                "reduced separately.",
                            station.source);
            }
        }
        Shot shot;
        shot.target = target;
        shot.face = pointing.face;
        shot.pointingIndex = pointing.index;
        shots.push_back(shot);
        return shots.back();
    };

    const std::string_view stationPoint = station.setup.pointId;
    for (const Observation& observation : station.observations) {
        if (const auto* direction = std::get_if<HorizontalDirectionObservation>(&observation)) {
            Shot& shot = shotFor(direction->pointing, direction->to);
            if (shot.direction) {
                Shot& fresh = shotFor(Pointing{}, direction->to);
                fresh.face = direction->pointing.face;
                fresh.pointingIndex = direction->pointing.index;
                fresh.direction = direction;
                fresh.directionRow = addRow(engine, station, "horizontal direction", direction->to,
                                            direction->pointing, true, direction->direction,
                                            direction->source);
                continue;
            }
            shot.direction = direction;
            shot.directionRow = addRow(engine, station, "horizontal direction", direction->to,
                                       direction->pointing, true, direction->direction,
                                       direction->source);
        } else if (const auto* zenith = std::get_if<ZenithAngleObservation>(&observation)) {
            Shot& shot = shotFor(zenith->pointing, zenith->to);
            Shot& target = (shot.zenith || shot.vertical) ? shotFor(Pointing{}, zenith->to) : shot;
            target.face = zenith->pointing.face;
            target.zenith = zenith;
            target.zenithRow = addRow(engine, station, "zenith angle", zenith->to,
                                      zenith->pointing, true, zenith->angle, zenith->source);
            // A face-right zenith is stored folded (360 - reading) because the
            // model keeps zeniths in [0, 180]; a reading above 180 is folded here.
            target.zenithValue = zenith->angle;
            if (zenith->angle > kPi) {
                target.zenithValue = kTwoPi - zenith->angle;
                correct(engine, target.zenithRow, CorrectionKind::Other,
                        target.zenithValue - zenith->angle, std::nullopt, "360 less reading");
            }
        } else if (const auto* vertical = std::get_if<VerticalAngleObservation>(&observation)) {
            Shot& shot = shotFor(vertical->pointing, vertical->to);
            Shot& target = (shot.zenith || shot.vertical) ? shotFor(Pointing{}, vertical->to)
                                                          : shot;
            target.face = vertical->pointing.face;
            target.vertical = vertical;
            target.zenithRow = addRow(engine, station, "vertical angle", vertical->to,
                                      vertical->pointing, true, vertical->angle, vertical->source);
            target.zenithValue = kHalfPi - vertical->angle;
            correct(engine, target.zenithRow, CorrectionKind::Other,
                    target.zenithValue - vertical->angle, std::nullopt, "as zenith");
        } else if (const auto* distance = std::get_if<DistanceObservation>(&observation)) {
            Shot& shot = shotFor(distance->pointing, distance->to);
            Shot& target = shot.distance ? shotFor(Pointing{}, distance->to) : shot;
            target.face = distance->pointing.face;
            target.distance = distance;
            target.distanceRow = addRow(
                engine, station,
                distance->kind == DistanceKind::Slope ? "slope distance" : "horizontal distance",
                distance->to, distance->pointing, false, distance->distance, distance->source);
            double value = distance->distance;
            if (const auto prism = prismCorrection(engine, station, *distance, warnedPrism)) {
                correct(engine, target.distanceRow, prism->kind, prism->amount, std::nullopt,
                        prism->note);
                value += prism->amount;
            }
            if (atmosphere.apply) {
                const double factor = 1.0 + atmosphere.ppm * 1e-6;
                correct(engine, target.distanceRow, CorrectionKind::Atmospheric,
                        value * (factor - 1.0), factor, atmosphere.note);
                value *= factor;
            }
            target.correctedDistance = value;
        } else {
            recordLoose(engine, observation, setupIndex, station.setup.id);
        }
    }

    // Pair by target: the i-th face-left pointing of a target with its i-th
    // face-right one, in the order observed.
    std::vector<std::pair<std::string_view, std::vector<std::size_t>>> byTarget;
    std::unordered_map<std::string_view, std::size_t> targetSlot;
    for (std::size_t i = 0; i < shots.size(); ++i) {
        if (shots[i].target == stationPoint) {
            rejectShot(engine, shots[i], "the target is the occupied point");
            continue;
        }
        const auto [it, inserted] = targetSlot.try_emplace(shots[i].target, byTarget.size());
        if (inserted) {
            byTarget.push_back({shots[i].target, {}});
        }
        byTarget[it->second].second.push_back(i);
    }

    for (const auto& [target, indices] : byTarget) {
        std::vector<std::size_t> lefts;
        std::vector<std::size_t> rights;
        std::vector<std::size_t> singles;
        std::vector<std::size_t> unknowns;
        for (const std::size_t i : indices) {
            if (settings.faces == FaceHandling::Separate) {
                singles.push_back(i);
            } else if (shots[i].face == Face::Left) {
                lefts.push_back(i);
            } else if (shots[i].face == Face::Right) {
                rights.push_back(i);
            } else {
                unknowns.push_back(i);
            }
        }
        // A file that does not state the face: two readings of one target
        // half a turn apart can only be the two faces, and meaning them as
        // one face would put the target in the wrong direction. The first
        // reading's side is taken as face left.
        std::optional<double> firstReading;
        bool opposite = false;
        for (const std::size_t i : unknowns) {
            if (shots[i].direction) {
                if (!firstReading) {
                    firstReading = shots[i].direction->direction;
                } else if (std::abs(normalizeAngleSigned(shots[i].direction->direction -
                                                         *firstReading)) > kHalfPi) {
                    opposite = true;
                }
            }
        }
        for (const std::size_t i : unknowns) {
            if (!opposite || !shots[i].direction) {
                singles.push_back(i);
                continue;
            }
            const bool right = std::abs(normalizeAngleSigned(shots[i].direction->direction -
                                                             *firstReading)) > kHalfPi;
            shots[i].face = right ? Face::Right : Face::Left;
            (right ? rights : lefts).push_back(i);
        }
        if (opposite) {
            engine.warnSetup(station, "the file does not state the faces; readings of one target "
                                      "half a turn apart were taken as face left and face right.");
        }
        std::size_t pairs = 0;
        if (settings.faces == FaceHandling::Average) {
            pairs = std::min(lefts.size(), rights.size());
            for (std::size_t p = 0; p < pairs; ++p) {
                ReducedPointing pointing;
                pointing.setup = setupIndex;
                pointing.target = target;
                reducePair(engine, setupIndex, pointing, shots[lefts[p]], shots[rights[p]]);
                reduceToHorizontal(engine, setupIndex, pointing, warnedSlope);
                state.pointings.push_back(engine.pointings.size());
                engine.pointings.push_back(pointing);
            }
        }
        for (std::size_t p = pairs; p < lefts.size(); ++p) {
            singles.push_back(lefts[p]);
        }
        for (std::size_t p = pairs; p < rights.size(); ++p) {
            if (settings.faces == FaceHandling::FaceLeftOnly) {
                rejectShot(engine, shots[rights[p]], "face right not used (face left only)");
                continue;
            }
            singles.push_back(rights[p]);
        }
        std::sort(singles.begin(), singles.end());
        for (const std::size_t i : singles) {
            ReducedPointing pointing;
            pointing.setup = setupIndex;
            pointing.target = target;
            reduceSingle(engine, pointing, shots[i]);
            reduceToHorizontal(engine, setupIndex, pointing, warnedSlope);
            state.pointings.push_back(engine.pointings.size());
            engine.pointings.push_back(pointing);
        }
    }
}

// ---- Phase B helpers ---------------------------------------------------------------------

bool isPhaseB(CorrectionKind kind)
{
    return kind == CorrectionKind::HeightReduction || kind == CorrectionKind::GridScale ||
           kind == CorrectionKind::CombinedFactor || kind == CorrectionKind::Orientation;
}

// Rewinds a row to its phase A value, so phase B can run again after an
// adjustment moved the stations.
void rewind(Engine& engine, std::size_t rowIndex)
{
    ReportObservation& observation = row(engine, rowIndex);
    auto& corrections = observation.corrections;
    double removed = 0.0;
    std::erase_if(corrections, [&removed](const AppliedCorrection& correction) {
        if (isPhaseB(correction.kind)) {
            removed += correction.amount;
            return true;
        }
        return false;
    });
    if (observation.reduced && removed != 0.0) {
        *observation.reduced -= removed;
    }
}

// The geoid separation a setup at `here` needs for its distances: for a height
// reduction to the ellipsoid or a grid scale from the projection, and never
// with the combined factor, which stands in for both.
std::optional<double> geoidFor(const Engine& engine, const Position& here)
{
    const ReductionSettings& settings = engine.settings;
    if ((settings.heightReduction == HeightReduction::Ellipsoid ||
         settings.gridScale == GridScale::FromProjection) &&
        engine.context.geoidSeparation && !settings.useCombinedFactor) {
        return engine.context.geoidSeparation(here.northing, here.easting);
    }
    return std::nullopt;
}

// Phase B's factors on one pointing's horizontal distance from a station at
// `here`: the combined factor, or the height reduction and the grid scale -
// at the line's mid-point, or at the setup while the pointing has no azimuth.
// With `record`, each factor is written on the pointing's distance rows and
// the setup's notices are given; without, the grid distance is only returned:
// what a resection solves with before the station it computes has a position
// (resectSetup), orientAndRadiate recording the factors once it has one.
std::optional<double> gridDistanceOf(Engine& engine, std::size_t setupIndex,
                                     const ReducedPointing& pointing, const Position& here,
                                     const std::optional<double>& geoid, bool record)
{
    if (!pointing.horizontal) {
        return std::nullopt;
    }
    const ReductionSettings& settings = engine.settings;
    const ReductionContext& context = engine.context;
    const SurveyStation& station = engine.raw.stations[setupIndex];
    SetupState& state = engine.setups[setupIndex];
    const double instrumentHeight = station.setup.instrumentHeight;
    double distance = *pointing.horizontal;
    const auto factor = [&](CorrectionKind kind, double value, std::string note) {
        if (record) {
            for (std::size_t i = 0; i < pointing.distanceRowCount; ++i) {
                correct(engine, pointing.distanceRows[i], kind, distance * (value - 1.0), value,
                        note);
            }
        }
        distance *= value;
    };
    const auto notice = [&](bool& given, const std::string& text) {
        if (record && !given) {
            given = true;
            engine.warn("Setup " + station.setup.id + text, station.source);
        }
    };
    if (settings.useCombinedFactor) {
        factor(CorrectionKind::CombinedFactor, settings.combinedFactor, {});
    } else {
        if (settings.heightReduction != HeightReduction::None) {
            if (!here.height) {
                notice(state.warnedHeight, ": its point " + station.setup.pointId +
                                               " has no height, so its distances were not "
                                               "reduced for height.");
            } else {
                double height = *here.height + instrumentHeight + 0.5 * pointing.measuredVertical;
                bool ok = true;
                if (settings.heightReduction == HeightReduction::Ellipsoid) {
                    if (geoid) {
                        height += *geoid;
                    } else {
                        ok = false;
                        notice(state.warnedGeoid, ": the drawing gives no geoid separation there, "
                                                  "so its distances were not reduced to the "
                                                  "ellipsoid.");
                    }
                }
                if (ok) {
                    factor(CorrectionKind::HeightReduction,
                           heightReductionFactor(height, settings.earthRadius),
                           "h " + formatNumber(height, 1) + " m");
                }
            }
        }
        if (settings.gridScale == GridScale::Fixed) {
            factor(CorrectionKind::GridScale, settings.fixedGridScaleFactor, "fixed");
        } else if (settings.gridScale == GridScale::FromProjection) {
            double northing = here.northing;
            double easting = here.easting;
            const char* where = "at setup";
            if (pointing.azimuth) {
                northing += 0.5 * distance * std::cos(*pointing.azimuth);
                easting += 0.5 * distance * std::sin(*pointing.azimuth);
                where = "at mid-point";
            }
            double ellipsoidal = here.height.value_or(0.0) + geoid.value_or(0.0);
            if (pointing.heightDifference) {
                ellipsoidal += 0.5 * *pointing.heightDifference;
            }
            const std::optional<double> scale =
                context.gridScaleFactor(northing, easting, ellipsoidal);
            if (scale && std::isfinite(*scale) && *scale > 0.0) {
                factor(CorrectionKind::GridScale, *scale, where);
            } else {
                notice(state.warnedScale, ": the drawing's projection gives no scale factor "
                                          "there, so its distances were not reduced to grid.");
            }
        }
    }
    if (record) {
        for (std::size_t i = 0; i < pointing.distanceRowCount; ++i) {
            setReduced(engine, pointing.distanceRows[i], distance);
        }
    }
    return distance;
}

// The orientation of a resected setup (SetupState::resected) standing at
// `here`: the weighted mean of azimuth less reading over the directions its
// resection used, wrapped about the first - at the resection's own station,
// its least-squares orientation.
std::optional<double> resectionOrientation(const Engine& engine, const SetupState& state,
                                           const Position& here)
{
    std::optional<double> first;
    double offsets = 0.0;
    double weights = 0.0;
    for (const auto& [p, sigma] : state.resectionDirections) {
        const ReducedPointing& pointing = engine.pointings[p];
        const auto target = engine.positions.find(pointing.target);
        if (target == engine.positions.end() || !pointing.direction) {
            continue;
        }
        const double deltaNorthing = target->second.northing - here.northing;
        const double deltaEasting = target->second.easting - here.easting;
        if (std::hypot(deltaNorthing, deltaEasting) <= katana::math::tolerance::kCoordinate) {
            continue;
        }
        const double value =
            normalizeAngle(std::atan2(deltaEasting, deltaNorthing) - *pointing.direction);
        if (!first) {
            first = value;
        }
        const double weight = 1.0 / (sigma * sigma);
        offsets += weight * normalizeAngleSigned(value - *first);
        weights += weight;
    }
    if (!first) {
        return std::nullopt;
    }
    return normalizeAngleSigned(*first + offsets / weights);
}

} // namespace

std::optional<double> gridDistanceFrom(Engine& engine, std::size_t setupIndex,
                                       const ReducedPointing& pointing, const Position& here)
{
    return gridDistanceOf(engine, setupIndex, pointing, here, geoidFor(engine, here), false);
}

std::optional<double> meanDirection(const Engine& engine, std::size_t setupIndex,
                                    std::string_view target)
{
    std::optional<double> first;
    double sum = 0.0;
    std::size_t count = 0;
    for (const std::size_t p : engine.setups[setupIndex].pointings) {
        const ReducedPointing& pointing = engine.pointings[p];
        if (pointing.rejected || pointing.target != target || !pointing.direction) {
            continue;
        }
        if (!first) {
            first = *pointing.direction;
        }
        sum += normalizeAngleSigned(*pointing.direction - *first);
        ++count;
    }
    if (!first) {
        return std::nullopt;
    }
    return normalizeAngle(*first + sum / static_cast<double>(count));
}

void orientAndRadiate(Engine& engine, std::size_t setupIndex,
                      const std::unordered_set<std::string_view>* reradiate)
{
    const SurveyStation& station = engine.raw.stations[setupIndex];
    SetupState& state = engine.setups[setupIndex];
    ReportSetup& setup = engine.report.setups[state.reportIndex];
    const Position* stationPosition = engine.find(station.setup.pointId);
    if (stationPosition == nullptr) {
        return;
    }
    const Position here = *stationPosition; // a copy: placing targets may rehash
    state.positioned = true;

    // ---- orientation ----
    const std::string_view backsight = station.backsightPointId;
    const bool namedBacksight = !backsight.empty() && backsight != station.setup.pointId;
    const Position* backsightPosition = namedBacksight ? engine.find(backsight) : nullptr;
    std::optional<double> backsightAzimuth;
    state.orientationAssumed = false;
    state.orientationStated = false;
    // No backsight named, one placed on top of the setup, or one nothing
    // will place: its coordinates will never orient this setup.
    const bool coordinatesWillNotOrient =
        !namedBacksight || backsightPosition != nullptr || state.acceptCircleAsSet;
    if (backsightPosition != nullptr &&
        std::hypot(backsightPosition->northing - here.northing,
                   backsightPosition->easting - here.easting) > 1e-4) {
        backsightAzimuth = normalizeAngle(std::atan2(backsightPosition->easting - here.easting,
                                                     backsightPosition->northing - here.northing));
    } else if (station.statedBacksightAzimuth && coordinatesWillNotOrient) {
        // The azimuth the file states for the backsight: only where no
        // coordinates of the backsight can orient the setup (see
        // SetupState::acceptCircleAsSet for why not sooner). Less the
        // reading on the backsight below, it is the field software's own
        // orientation correction.
        backsightAzimuth = normalizeAngle(*station.statedBacksightAzimuth);
        state.orientationAssumed = true;
        state.orientationStated = true;
    } else if (station.backsightAzimuth && coordinatesWillNotOrient) {
        // The circle as set, taken as the grid azimuth, on the same terms.
        backsightAzimuth = normalizeAngle(*station.backsightAzimuth);
        state.orientationAssumed = true;
    }
    const std::optional<double> backsightReading =
        backsight.empty() ? std::nullopt : meanDirection(engine, setupIndex, backsight);
    setup.backsightAzimuth = backsightAzimuth;
    setup.backsightReading = backsightReading;
    state.orientation.reset();
    if (backsightAzimuth) {
        if (backsightReading) {
            state.orientation = normalizeAngleSigned(*backsightAzimuth - *backsightReading);
        } else if (station.backsightAzimuth) {
            // The circle was set to this reading on the backsight.
            state.orientation =
                normalizeAngleSigned(*backsightAzimuth - normalizeAngle(*station.backsightAzimuth));
        }
    }
    if (state.resected) {
        // Oriented by its resection, whatever its backsight: a named one is
        // a target like the others there.
        state.orientation = resectionOrientation(engine, state, here);
        state.orientationAssumed = false;
        state.orientationStated = false;
    }
    setup.orientationCorrection = state.orientation;

    // ---- per pointing: factors, azimuth, position ----
    // Per target, the means of what this setup measured to it. Directions
    // and distances are meaned apart, so a file that does not group a shot's
    // observations into one pointing still radiates.
    struct Accumulator {
        std::string_view target;
        double firstAzimuth = 0.0;
        double azimuthOffsets = 0.0; // sum of (azimuth - first), wrapped
        std::size_t azimuthCount = 0;
        double distance = 0.0;
        std::size_t distanceCount = 0;
        double heightDifference = 0.0;
        std::size_t heightCount = 0;
    };
    std::vector<Accumulator> radiated;
    std::unordered_map<std::string_view, std::size_t> radiatedSlot;
    const std::optional<double> geoid = geoidFor(engine, here);

    for (const std::size_t p : state.pointings) {
        ReducedPointing& pointing = engine.pointings[p];
        if (pointing.rejected) {
            continue;
        }
        for (std::size_t i = 0; i < pointing.distanceRowCount; ++i) {
            rewind(engine, pointing.distanceRows[i]);
        }
        for (std::size_t i = 0; i < pointing.directionRowCount; ++i) {
            rewind(engine, pointing.directionRows[i]);
        }

        pointing.azimuth.reset();
        if (pointing.direction && state.orientation) {
            pointing.azimuth = normalizeAngle(*pointing.direction + *state.orientation);
            for (std::size_t i = 0; i < pointing.directionRowCount; ++i) {
                const std::size_t index = pointing.directionRows[i];
                correct(engine, index, CorrectionKind::Orientation, *state.orientation,
                        std::nullopt,
                        state.orientationStated     ? "azimuth as the file states"
                        : state.orientationAssumed ? "circle as set"
                                                   : std::string{});
                setReduced(engine, index, *pointing.azimuth);
            }
        }

        pointing.gridDistance = gridDistanceOf(engine, setupIndex, pointing, here, geoid, true);

        if (reradiate != nullptr && reradiate->count(pointing.target) == 0 &&
            pointing.target != backsight) {
            continue;
        }
        if (state.resectionPointings.count(p) != 0) {
            continue; // its resection used it: its residuals are reported there, not as a check
        }
        if (!pointing.azimuth && !pointing.gridDistance && !pointing.heightDifference) {
            continue;
        }
        const auto [slot, inserted] = radiatedSlot.try_emplace(pointing.target, radiated.size());
        if (inserted) {
            radiated.push_back(Accumulator{pointing.target});
        }
        Accumulator& a = radiated[slot->second];
        if (pointing.azimuth) {
            if (a.azimuthCount == 0) {
                a.firstAzimuth = *pointing.azimuth;
            }
            a.azimuthOffsets += normalizeAngleSigned(*pointing.azimuth - a.firstAzimuth);
            ++a.azimuthCount;
        }
        if (pointing.gridDistance) {
            a.distance += *pointing.gridDistance;
            ++a.distanceCount;
        }
        if (pointing.heightDifference) {
            a.heightDifference += *pointing.heightDifference;
            ++a.heightCount;
        }
    }

    // ---- place or check ----
    for (const Accumulator& a : radiated) {
        if (a.azimuthCount == 0 || a.distanceCount == 0) {
            continue; // an angle or a distance alone positions nothing
        }
        const double count = static_cast<double>(a.distanceCount);
        const double azimuth =
            a.firstAzimuth + a.azimuthOffsets / static_cast<double>(a.azimuthCount);
        Position computed;
        computed.northing = here.northing + a.distance / count * std::cos(azimuth);
        computed.easting = here.easting + a.distance / count * std::sin(azimuth);
        if (here.height && a.heightCount > 0) {
            computed.height = *here.height + a.heightDifference / static_cast<double>(a.heightCount);
        }
        computed.origin = PositionOrigin::Computed;
        computed.method = ComputationMethod::Radiation;
        computed.fromSetup = setupIndex;

        Position* existing = engine.find(a.target);
        if (a.target == backsight && existing != nullptr) {
            // The backsight check: measured against the coordinates.
            const double known = std::hypot(existing->northing - here.northing,
                                            existing->easting - here.easting);
            setup.backsightDistanceDifference = a.distance / count - known;
            if (here.height && existing->height && a.heightCount > 0) {
                setup.backsightHeightDifference = *computed.height - *existing->height;
            }
            continue;
        }
        if (reradiate != nullptr) {
            // A point its resection was computed from is held by that
            // resection: a later pointing to it checks the station, and
            // places nothing.
            if (state.resectionTargets.count(a.target) == 0) {
                engine.place(a.target, computed);
            }
            continue;
        }
        if (existing == nullptr || existing->origin == PositionOrigin::FileOnly) {
            engine.place(a.target, computed);
            continue;
        }
        if (existing->origin == PositionOrigin::Computed && !existing->height && computed.height) {
            existing->height = computed.height;
        }
        MisclosureReport check;
        check.name = std::string(a.target) + " from setup " + station.setup.id;
        check.northing = computed.northing - existing->northing;
        check.easting = computed.easting - existing->easting;
        check.linear = std::hypot(*check.northing, *check.easting);
        check.length = a.distance / count;
        if (*check.linear > 0.0) {
            check.precisionRatio = check.length / *check.linear;
        }
        if (computed.height && existing->height) {
            check.height = *computed.height - *existing->height;
        }
        engine.report.misclosures.push_back(std::move(check));
    }
}

namespace {

// ---- Seeds: control, entered points, GNSS -------------------------------------------

Status seedControl(Engine& engine, std::unordered_set<std::string_view>& drawingOnly)
{
    for (const ControlSelection& selection : engine.settings.control) {
        const std::string& id = selection.point.pointId;
        const SurveyPoint* point = nullptr;
        if (selection.origin == ControlOrigin::File) {
            const auto it = engine.filePoints.find(id);
            point = it == engine.filePoints.end() ? nullptr : it->second;
        } else {
            for (const SurveyPoint& candidate : engine.context.drawingPoints) {
                if (candidate.id == id) {
                    point = &candidate;
                    break;
                }
            }
            if (point != nullptr && engine.filePoints.find(id) == engine.filePoints.end()) {
                drawingOnly.insert(point->id);
            }
        }
        if (point == nullptr) {
            return makeError(ErrorCode::NotFound,
                             "Control point " + id + " is not " +
                                 (selection.origin == ControlOrigin::File
                                      ? "a point with coordinates in the file"
                                      : "on the drawing") +
                                 ", so it cannot be held.");
        }
        const bool heldHeight = selection.point.elevation.constraint != ControlConstraint::Free;
        if (heldHeight && !point->elevation) {
            return makeError(ErrorCode::InvalidArgument,
                             "Control point " + id +
                                 " is held in height but has no height; give it one or free "
                                 "its height.");
        }
        Position position;
        position.northing = point->northing;
        position.easting = point->easting;
        position.height = point->elevation;
        position.origin = PositionOrigin::Control;
        position.method = ComputationMethod::Control;
        position.heldHorizontal = selection.point.northing.constraint != ControlConstraint::Free ||
                                  selection.point.easting.constraint != ControlConstraint::Free;
        position.heldHeight = heldHeight;
        const auto sigmaOf = [](const ControlComponent& component) -> std::optional<double> {
            switch (component.constraint) {
            case ControlConstraint::Fixed:
                return 0.0;
            case ControlConstraint::Weighted:
                return component.sigma;
            case ControlConstraint::Free:
                return std::nullopt;
            }
            return std::nullopt;
        };
        position.sigmaNorthing = sigmaOf(selection.point.northing);
        position.sigmaEasting = sigmaOf(selection.point.easting);
        position.sigmaHeight = sigmaOf(selection.point.elevation);
        engine.place(point->id, position);
    }
    return {};
}

// The record a file's resection block ends at, from the setup's
// kResectionEndMetadata ("record N"): absent where the setup has none, or its
// value does not end in a record number - and then every pointing of the
// setup may enter its resection, as for a file that marks no block.
std::optional<std::size_t> resectionBlockEndOf(const SurveyStation& station)
{
    const auto it = station.metadata.find(kResectionEndMetadata);
    if (it == station.metadata.end()) {
        return std::nullopt;
    }
    const std::string_view value = katana::core::trimmed(it->second);
    const std::size_t space = value.find_last_of(' ');
    const std::optional<std::int64_t> record = katana::core::parseInteger(
        space == std::string_view::npos ? value : value.substr(space + 1));
    if (!record || *record <= 0) {
        return std::nullopt;
    }
    return static_cast<std::size_t>(*record);
}

void seedEntered(Engine& engine)
{
    for (const SurveyPoint& point : engine.raw.points) {
        if (point.coordinateSource != CoordinateSource::Entered || engine.find(point.id)) {
            continue;
        }
        Position position;
        position.northing = point.northing;
        position.easting = point.easting;
        position.height = point.elevation;
        position.origin = PositionOrigin::Entered;
        position.method = ComputationMethod::Control;
        engine.place(point.id, position);
    }
}

// What seedGnss found worth one sentence each rather than one per position.
struct GnssSeedNotices {
    std::size_t toReferencePoint = 0; // antenna heights to the ARP, not the phase centre
};

// GNSS positions: the grid ones as they are, the global ones through the
// context's conversion. The first position of a point places it, unless
// control or an entered point already holds it; a later one of a point an
// earlier position placed is a check on it, with its misclosure. Each is
// kept in engine.globalPositions, and the network takes every one of a
// point GNSS placed, each with its own value.
//
// A global position is where the receiver was: at its antenna. The file's
// antenna height takes it down to the mark, the way reduction_gnss.cpp takes
// a vector's ends down, so a position and a vector from it agree on what
// height they mean. A reader whose position is already of the mark gives no
// antenna (height 0).
void seedGnss(Engine& engine, std::size_t rawIndex, std::size_t rowIndex, GnssSeedNotices& notices)
{
    const Observation& observation = engine.raw.observations[rawIndex];
    const ReductionSettings& settings = engine.settings;
    const ReductionContext& context = engine.context;
    GlobalGridPosition converted;
    converted.row = rowIndex;
    converted.sigmaHorizontal = settings.apriori.gnssHorizontal;
    converted.sigmaVertical = settings.apriori.gnssVertical;
    if (const auto* position = std::get_if<GnssPositionObservation>(&observation)) {
        converted.point = position->point;
        converted.northing = position->northing;
        converted.easting = position->easting;
        converted.height = position->elevation;
        converted.sigmaHorizontal = std::max(position->sigmaNorthing, position->sigmaEasting);
        converted.sigmaVertical = position->sigmaElevation;
        converted.source = &position->source;
    } else if (const auto* global = std::get_if<GnssGlobalPositionObservation>(&observation)) {
        converted.point = global->point;
        converted.source = &global->source;
        const std::optional<GridPosition> grid = global->geocentric
                                                     ? gridOfGeocentric(context, *global->geocentric)
                                                     : gridOfGeodetic(context, *global->geodetic);
        if (!grid) {
            engine.warn("GNSS position of " + global->point +
                            " not used: the drawing's coordinate system cannot convert it to "
                            "grid.",
                        global->source);
            reject(engine, rowIndex, "no conversion to the drawing's grid");
            return;
        }
        if (settings.useFileCovariances && global->covariance.stated()) {
            // A geodetic value's covariance is already local north / east /
            // up; a geocentric one's is X / Y / Z and is turned at the point.
            double north = global->covariance.xx;
            double east = global->covariance.yy;
            double up = global->covariance.zz;
            if (global->geocentric) {
                const LocalVariances local =
                    localVariances(global->covariance, geodeticFromGeocentric(*global->geocentric));
                north = local.north;
                east = local.east;
                up = local.up;
            }
            if (north > 0.0 && east > 0.0 && up > 0.0) {
                converted.sigmaHorizontal = std::sqrt(std::max(north, east));
                converted.sigmaVertical = std::sqrt(up);
            }
        }
        converted.northing = grid->northing;
        converted.easting = grid->easting;
        // The row follows the height, the one value the reduction changes:
        // from the ellipsoid at the antenna down to the mark.
        ReportObservation& reportRow = row(engine, rowIndex);
        reportRow.raw = grid->ellipsoidalHeight;
        reportRow.reduced = grid->ellipsoidalHeight;
        double height = grid->ellipsoidalHeight;
        if (context.geoidSeparation) {
            if (const auto separation = context.geoidSeparation(grid->northing, grid->easting)) {
                correct(engine, rowIndex, CorrectionKind::Other, -*separation, std::nullopt,
                        "geoid separation " + formatNumber(*separation, 3) + " m");
                height -= *separation;
            }
        }
        // The antenna: the horizontal position needs nothing (the plumb line
        // through a 2 m pole moves 0.3 mm over the grid's convergence), the
        // height comes down by the antenna height.
        const GnssAntenna& antenna = global->antenna;
        std::string gridWords = "to grid N " + formatNumber(grid->northing, 3) + " E " +
                                formatNumber(grid->easting, 3);
        if (antenna.height != 0.0 && reducibleAntenna(antenna)) {
            correct(engine, rowIndex, CorrectionKind::InstrumentAndTargetHeight, -antenna.height,
                    std::nullopt, "antenna " + antennaWords(antenna));
            height -= antenna.height;
            if (antenna.method == AntennaHeightMethod::Vertical) {
                ++notices.toReferencePoint;
            }
            converted.height = height;
        } else if (antenna.height != 0.0) {
            engine.warn("GNSS position of " + global->point + ": its antenna height (" +
                            antennaWords(antenna) +
                            ") is not a vertical one, so the point's height was left out; its "
                            "horizontal position was used.",
                        global->source);
            gridWords += "; height left out, antenna " + antennaWords(antenna);
        } else {
            converted.height = height;
        }
        correct(engine, rowIndex, CorrectionKind::Other, 0.0, std::nullopt, std::move(gridWords));
        if (global->solution == GnssSolution::Float || global->solution == GnssSolution::Autonomous) {
            engine.warn("GNSS position of " + global->point + " is a " +
                            toString(global->solution) + " solution.",
                        global->source);
        }
    } else {
        return;
    }
    engine.globalPositions[rawIndex] = converted;

    if (const Position* existing = engine.find(converted.point)) {
        if (existing->origin != PositionOrigin::Gnss) {
            return; // control or an entered point wins; the position still serves a vector
        }
        // An earlier occupation placed it: this one is a check on it, never
        // silently unused.
        MisclosureReport check;
        check.name = std::string(converted.point) + " by a second GNSS position" +
                     (converted.source->recordNumber != 0
                          ? " (record " + std::to_string(converted.source->recordNumber) + ")"
                          : std::string{});
        check.northing = converted.northing - existing->northing;
        check.easting = converted.easting - existing->easting;
        check.linear = std::hypot(*check.northing, *check.easting);
        if (converted.height && existing->height) {
            check.height = *converted.height - *existing->height;
        }
        engine.report.misclosures.push_back(std::move(check));
        return;
    }
    Position position;
    position.northing = converted.northing;
    position.easting = converted.easting;
    position.height = converted.height;
    position.origin = PositionOrigin::Gnss;
    position.method = ComputationMethod::Gnss;
    position.sigmaNorthing = converted.sigmaHorizontal;
    position.sigmaEasting = converted.sigmaHorizontal;
    position.sigmaHeight = converted.sigmaVertical;
    engine.place(converted.point, position);
}

// ---- Phase B over the setups ------------------------------------------------------------

// Phase B for every setup: each in file order once its station has a
// position, with GNSS vectors radiated as their bases get one (a setup may
// stand on an RTK point, a vector may start at a point a setup radiated).
//
// A setup whose named backsight has no position yet waits for it: a setup on
// control that backsights a traverse point is oriented once a later setup
// has radiated that point. It is tried again when THAT point is placed, not
// whenever anything is: with a thousand setups placed one at a time, trying
// every waiting setup after every placement took 12 s, where waiting on the
// one point that matters costs nothing. Each setup is tried at most twice.
//
// When nothing more can be tried, in this order:
//   0. a setup whose station nothing positions, and which observes enough
//      points that are placed - two with a direction and a distance, or three
//      with a direction - is resected from them (resectSetup), one at a time
//      in file order: its station is then placed, and it orients and radiates
//      as any other setup;
//   1. a setup whose station nothing positions stands on the file's own
//      coordinates for it, one at a time (its radiation may position others);
//   2. a setup still waiting for its backsight, where the file records the
//      circle set on it, is oriented on that circle taken as a grid azimuth,
//      one at a time in file order (it may place another setup's backsight),
//      and the report says so;
//   3. the rest are given up, and the report says why.
//
// The resection is a fallback, not a first choice: a station another setup
// radiates is placed that way as it always was, and its setup's pointings to
// placed points are checks of it. It comes before the file's own coordinates
// because it is computed from what was measured, which is what the reduction
// is for, where those coordinates are the field software's or a person's,
// unchecked - the same order as a radiated point, which replaces the file's
// coordinates of a point (PositionOrigin::FileOnly). So a setup that stood on
// the file's own coordinates before (step 1) and observes enough placed
// points is now resected instead: the file's coordinates are then reported as
// a check of the resection, with a warning, never dropped unseen
// (resectSetup). It comes before the circle as set, which is an assumption
// about the instrument. A resection that is refused (resectSetup says why)
// leaves the setup to the later steps, which name the refusal.
void placeSetups(Engine& engine)
{
    const std::vector<SurveyStation>& stations = engine.raw.stations;
    const std::size_t count = stations.size();
    constexpr std::size_t kNone = static_cast<std::size_t>(-1);
    using MinHeap = std::priority_queue<std::size_t, std::vector<std::size_t>, std::greater<>>;

    std::vector<bool> done(count, false);
    std::vector<bool> tried(count, false);
    std::vector<bool> queued(count, false);
    std::size_t remaining = count;
    // Setups by the point they wait for: their station, or, once tried, their
    // backsight.
    std::unordered_map<std::string_view, std::vector<std::size_t>> waitingFor;
    // Waiting setups whose file records the circle set on the backsight.
    MinHeap circleCandidates;
    // A pass tries setups in file order; one woken by a setup later in the
    // file than itself waits for the next pass, as a sweep through the file
    // would leave it.
    MinHeap thisPass;
    MinHeap nextPass;
    // Entries of engine.positionOrder already looked up in waitingFor.
    std::size_t woken = 0;

    // Resection candidates (step 0): setups whose station has no position,
    // counted as the points they observe are placed, so that a placement
    // costs only the setups watching that point.
    struct Watch {
        std::size_t setup = 0;
        bool direction = false;
        bool distance = false;
    };
    std::unordered_map<std::string_view, std::vector<Watch>> watching;
    std::vector<std::size_t> placedWithDirection(count, 0);
    std::vector<std::size_t> placedWithBoth(count, 0);
    std::vector<bool> resectionQueued(count, false);
    MinHeap resectionCandidates;
    std::vector<std::string> resectionRefused(count); // why, for the later steps' words
    const auto countPlaced = [&](const Watch& watch) {
        const std::size_t s = watch.setup;
        placedWithDirection[s] += watch.direction ? 1 : 0;
        placedWithBoth[s] += watch.direction && watch.distance ? 1 : 0;
        if (!resectionQueued[s] && (placedWithBoth[s] >= 2 || placedWithDirection[s] >= 3)) {
            resectionQueued[s] = true;
            resectionCandidates.push(s);
        }
    };
    const auto watchTargets = [&](std::size_t s) {
        std::unordered_map<std::string_view, Watch> targets;
        for (const std::size_t p : engine.setups[s].pointings) {
            const ReducedPointing& pointing = engine.pointings[p];
            if (pointing.rejected || pointing.target == stations[s].setup.pointId ||
                !inResectionBlock(engine, s, pointing)) {
                continue;
            }
            Watch& watch = targets.try_emplace(pointing.target, Watch{s, false, false}).first->second;
            watch.direction = watch.direction || pointing.direction.has_value();
            watch.distance = watch.distance || pointing.horizontal.has_value();
        }
        for (const auto& [target, watch] : targets) {
            if (engine.find(target) != nullptr) {
                countPlaced(watch);
            } else {
                watching[target].push_back(watch);
            }
        }
    };

    const auto wake = [&](std::size_t current) {
        while (woken < engine.positionOrder.size()) {
            const std::string_view placed = engine.positionOrder[woken++];
            if (const auto seen = watching.find(placed); seen != watching.end()) {
                for (const Watch& watch : seen->second) {
                    if (!done[watch.setup]) {
                        countPlaced(watch);
                    }
                }
                watching.erase(seen);
            }
            const auto it = waitingFor.find(placed);
            if (it == waitingFor.end()) {
                continue;
            }
            for (const std::size_t s : it->second) {
                if (done[s] || queued[s]) {
                    continue;
                }
                queued[s] = true;
                (current == kNone || s > current ? thisPass : nextPass).push(s);
            }
            waitingFor.erase(it);
        }
    };
    const auto waitsForBacksight = [&](std::size_t s) {
        const SurveyStation& station = stations[s];
        const std::string& backsight = station.backsightPointId;
        return !engine.setups[s].orientation && !backsight.empty() &&
               backsight != station.setup.pointId && engine.find(backsight) == nullptr;
    };
    const auto warnUnoriented = [&](std::size_t s) {
        const SurveyStation& station = stations[s];
        const std::string& backsight = station.backsightPointId;
        std::string why;
        if (backsight.empty()) {
            why = " has no backsight";
            // A setup like a second resection block on a station already
            // placed observes placed points that could orient it, which the
            // reduction does not do (docs/survey.md, "The reduction's
            // resection", Not done): named, so the warning is not read as a
            // setup that observed nothing known.
            std::vector<std::string_view> placed;
            for (const std::size_t p : engine.setups[s].pointings) {
                const ReducedPointing& pointing = engine.pointings[p];
                if (!pointing.rejected && pointing.direction &&
                    engine.find(pointing.target) != nullptr &&
                    std::find(placed.begin(), placed.end(), pointing.target) == placed.end()) {
                    placed.push_back(pointing.target);
                }
            }
            if (!placed.empty()) {
                why += "; it observes " + std::to_string(placed.size()) + " placed point" +
                       (placed.size() == 1 ? "" : "s") + " (";
                for (std::size_t i = 0; i < placed.size() && i < 5; ++i) {
                    why += (i == 0 ? "" : ", ") + std::string(placed[i]);
                }
                why += placed.size() > 5 ? ", ...)" : ")";
                why += ", but a setup on a placed station is not oriented on the points it "
                       "observes";
            }
        } else if (backsight == station.setup.pointId) {
            why = " cannot be oriented: its backsight is the point it stands on and no circle "
                  "setting was recorded";
        } else if (engine.find(backsight) == nullptr) {
            why = " cannot be oriented: its backsight " + backsight +
                  " has no position and no circle setting was recorded";
        } else {
            why = " cannot be oriented on its backsight " + backsight +
                  ": it has no direction to it, or stands on the same position, and no circle "
                  "setting was recorded";
        }
        engine.warn("Setup " + station.setup.id + why +
                        ", so its directions are not oriented and its targets are not radiated.",
                    station.source);
    };
    const auto finish = [&](std::size_t s) {
        done[s] = true;
        --remaining;
        const SetupState& state = engine.setups[s];
        if (state.orientationAssumed && state.orientation && state.orientationStated) {
            engine.warnSetup(
                stations[s],
                state.acceptCircleAsSet
                    ? "its backsight has no position, so its directions were oriented on the "
                      "azimuth the file states for the backsight, less the reading on it: the "
                      "bearings are the file's, not ones computed from coordinates."
                    : "no backsight coordinates orient it, so its directions were oriented on "
                      "the azimuth the file states for its backsight, less the reading on it: "
                      "the bearings are the file's, not ones computed from coordinates.");
        } else if (state.orientationAssumed && state.orientation) {
            engine.warnSetup(
                stations[s],
                state.acceptCircleAsSet
                    ? "its backsight has no position, so its directions were oriented on the "
                      "circle reading set on the backsight, taken as a grid azimuth: the "
                      "bearings are right only if the circle was set to one."
                    : "no backsight coordinates orient it, so its directions were oriented on "
                      "the circle reading as set, taken as a grid azimuth: the bearings are "
                      "right only if the circle was set to one.");
        } else if (!state.orientation) {
            warnUnoriented(s);
        }
    };
    const auto attempt = [&](std::size_t s) {
        queued[s] = false;
        if (done[s]) {
            return;
        }
        tried[s] = true;
        orientAndRadiate(engine, s);
        if (waitsForBacksight(s)) {
            waitingFor[stations[s].backsightPointId].push_back(s);
            if (stations[s].backsightAzimuth || stations[s].statedBacksightAzimuth) {
                circleCandidates.push(s);
            }
            return;
        }
        finish(s);
    };

    radiateGnssVectors(engine);
    for (std::size_t s = 0; s < count; ++s) {
        if (engine.find(stations[s].setup.pointId) != nullptr) {
            queued[s] = true;
            thisPass.push(s);
        } else {
            waitingFor[stations[s].setup.pointId].push_back(s);
            watchTargets(s);
        }
    }
    // Everything placed so far was placed before anything waited for it.
    woken = engine.positionOrder.size();

    std::size_t fallbackCursor = 0; // setups before it can never fall back
    while (remaining > 0) {
        while (!nextPass.empty()) {
            thisPass.push(nextPass.top());
            nextPass.pop();
        }
        radiateGnssVectors(engine);
        wake(kNone);
        if (!thisPass.empty()) {
            while (!thisPass.empty()) {
                const std::size_t s = thisPass.top();
                thisPass.pop();
                attempt(s);
                wake(s);
            }
            continue;
        }

        // 0. A resection, for the first setup in the file that can have one.
        // Its station, once placed, wakes it as any placement wakes a setup.
        bool resected = false;
        while (!resectionCandidates.empty() && !resected) {
            const std::size_t s = resectionCandidates.top();
            resectionCandidates.pop();
            resectionQueued[s] = false;
            if (done[s] || engine.find(stations[s].setup.pointId) != nullptr) {
                continue;
            }
            std::string why;
            resected = resectSetup(engine, s, why);
            if (!resected) {
                resectionRefused[s] = std::move(why);
            }
        }
        if (resected) {
            continue;
        }

        // 1. The file's own coordinates for a station nothing positions. A
        // setup already standing on a position keeps it.
        bool fellBack = false;
        for (; fallbackCursor < count && !fellBack; ++fallbackCursor) {
            const std::size_t s = fallbackCursor;
            const std::string& pointId = stations[s].setup.pointId;
            if (done[s] || engine.find(pointId) != nullptr) {
                continue;
            }
            const auto it = engine.filePoints.find(pointId);
            if (it == engine.filePoints.end()) {
                continue;
            }
            Position position;
            position.northing = it->second->northing;
            position.easting = it->second->easting;
            position.height = it->second->elevation;
            position.origin = PositionOrigin::FileOnly;
            engine.place(it->second->id, position);
            engine.warn("Setup " + stations[s].setup.id + " stands on " + pointId +
                            ", which is not control and was not computed" +
                            (resectionRefused[s].empty()
                                 ? std::string{}
                                 : " (its resection was refused: " + resectionRefused[s] + ")") +
                            "; the file's own coordinates for it were used.",
                        stations[s].source);
            fellBack = true;
        }
        if (fellBack) {
            continue;
        }

        // 2. The circle as set - or the azimuth the file states for the backsight -
        // for the first setup still waiting that has one.
        while (!circleCandidates.empty() && done[circleCandidates.top()]) {
            circleCandidates.pop();
        }
        if (!circleCandidates.empty()) {
            const std::size_t s = circleCandidates.top();
            circleCandidates.pop();
            engine.setups[s].acceptCircleAsSet = true;
            orientAndRadiate(engine, s);
            finish(s);
            continue;
        }

        // 3. Nothing more can be done.
        for (std::size_t s = 0; s < count; ++s) {
            if (!done[s] && tried[s]) {
                done[s] = true;
                --remaining;
                warnUnoriented(s);
            }
        }
        for (std::size_t s = 0; s < count; ++s) {
            if (!done[s]) {
                done[s] = true;
                --remaining;
                // Why it was not resected, where it observes any placed point.
                std::string why = resectionRefused[s];
                if (why.empty()) {
                    why = resectionShortfall(engine, s);
                }
                engine.warn("Setup " + stations[s].setup.id + " stands on " +
                                stations[s].setup.pointId +
                                ", which has no position: nothing was computed from it." +
                                (why.empty() ? std::string{} : " It was not resected: " + why + "."),
                            stations[s].source);
            }
        }
    }
}

// ---- Outputs -----------------------------------------------------------------------------

// The raw project with every station's pointings replaced by the reduced
// ones. Built field by field rather than copied and edited: copying 100 000
// raw observations only to throw them away cost as much as reducing them.
SurveyProject reducedProject(const Engine& engine)
{
    const SurveyProject& raw = engine.raw;
    SurveyProject reduced;
    reduced.name = raw.name;
    reduced.coordinateSystem = raw.coordinateSystem;
    reduced.units = raw.units;
    reduced.points = raw.points;
    reduced.unpositionedPoints = raw.unpositionedPoints;
    reduced.observations = raw.observations;
    reduced.traverses = raw.traverses;
    reduced.features = raw.features;
    reduced.metadata = raw.metadata;
    reduced.source = raw.source;
    reduced.controlPoints = raw.controlPoints;
    reduced.gnssSessions = raw.gnssSessions;
    reduced.stations.resize(raw.stations.size());
    for (std::size_t s = 0; s < raw.stations.size(); ++s) {
        const SurveyStation& from = raw.stations[s];
        SurveyStation& station = reduced.stations[s];
        station.setup = from.setup;
        station.backsightPointId = from.backsightPointId;
        station.backsightAzimuth = from.backsightAzimuth;
        station.statedBacksightAzimuth = from.statedBacksightAzimuth;
        station.metadata = from.metadata;
        station.source = from.source;
        station.instrument = from.instrument;
        std::vector<Observation> kept;
        kept.reserve(3 * engine.setups[s].pointings.size() + 4);
        for (const Observation& observation : from.observations) {
            const bool pointingKind =
                std::holds_alternative<HorizontalDirectionObservation>(observation) ||
                std::holds_alternative<ZenithAngleObservation>(observation) ||
                std::holds_alternative<VerticalAngleObservation>(observation) ||
                std::holds_alternative<DistanceObservation>(observation);
            if (!pointingKind) {
                kept.push_back(observation);
            }
        }
        const std::string& at = station.setup.pointId;
        for (const std::size_t p : engine.setups[s].pointings) {
            const ReducedPointing& pointing = engine.pointings[p];
            if (pointing.rejected) {
                continue;
            }
            const SourceRecord source = pointing.source ? *pointing.source : SourceRecord{};
            const Pointing group{pointing.leftIndex, Face::Unknown};
            if (pointing.azimuth) {
                kept.push_back(AzimuthObservation{at, std::string(pointing.target),
                                                  *pointing.azimuth, pointing.sigmaDirection,
                                                  source});
            } else if (pointing.direction) {
                HorizontalDirectionObservation direction;
                direction.at = at;
                direction.to = pointing.target;
                direction.direction = *pointing.direction;
                direction.sigma = pointing.sigmaDirection;
                direction.source = source;
                direction.pointing = group;
                kept.push_back(std::move(direction));
            }
            const std::optional<double> distance =
                pointing.gridDistance ? pointing.gridDistance : pointing.horizontal;
            if (distance && *distance > 0.0 && pointing.sigmaDistance > 0.0) {
                DistanceObservation horizontal;
                horizontal.from = at;
                horizontal.to = pointing.target;
                horizontal.distance = *distance;
                horizontal.sigma = pointing.sigmaDistance;
                horizontal.kind = DistanceKind::Horizontal;
                horizontal.instrumentHeight = station.setup.instrumentHeight;
                horizontal.targetHeight = pointing.targetHeight;
                horizontal.source = source;
                horizontal.pointing = group;
                kept.push_back(std::move(horizontal));
            }
            if (pointing.zenith && *pointing.zenith >= 0.0 && *pointing.zenith <= kPi &&
                pointing.sigmaZenith > 0.0) {
                ZenithAngleObservation zenith;
                zenith.from = at;
                zenith.to = pointing.target;
                zenith.angle = *pointing.zenith;
                zenith.sigma = pointing.sigmaZenith;
                zenith.instrumentHeight = station.setup.instrumentHeight;
                zenith.targetHeight = pointing.targetHeight;
                zenith.source = source;
                zenith.pointing = group;
                kept.push_back(std::move(zenith));
            }
        }
        station.observations = std::move(kept);
    }
    return reduced;
}

} // namespace
} // namespace detail

Result<ReductionOutcome> reduceAndAdjust(const SurveyProject& raw,
                                         const ReductionSettings& settings,
                                         const ReductionContext& context)
{
    using namespace detail;
    if (Status status = validateReductionSettings(settings); !status) {
        return status.error();
    }
    if (!settings.useCombinedFactor) {
        if (settings.heightReduction == HeightReduction::Ellipsoid && !context.geoidSeparation) {
            return makeError(ErrorCode::InvalidArgument,
                             "Height reduction to the ellipsoid needs the geoid separation, and "
                             "the drawing's coordinate system cannot give one. Reduce to the "
                             "geoid, or use a combined scale factor.",
                             "height reduction");
        }
        if (settings.gridScale == GridScale::FromProjection && !context.gridScaleFactor) {
            return makeError(ErrorCode::InvalidArgument,
                             "Grid scale from the projection needs the drawing's coordinate "
                             "system, and the drawing has none. Give a fixed scale factor or a "
                             "combined factor instead.",
                             "grid scale");
        }
    }

    Engine engine(raw, settings, context);
    engine.report.createdUtc = context.createdUtc;
    engine.report.input = context.input;
    engine.report.settings = settings;
    engine.setups.resize(raw.stations.size());
    for (std::size_t s = 0; s < raw.stations.size(); ++s) {
        engine.setups[s].resectionBlockEnd = resectionBlockEndOf(raw.stations[s]);
    }
    engine.filePoints.reserve(raw.points.size());
    for (const SurveyPoint& point : raw.points) {
        engine.filePoints.emplace(point.id, &point);
    }
    std::size_t observationCount = raw.observations.size();
    for (const SurveyStation& station : raw.stations) {
        observationCount += station.observations.size();
    }
    engine.report.observations.reserve(observationCount + observationCount / 3 + 8);
    engine.report.facePairs.reserve(observationCount / 6 + 4);
    engine.pointings.reserve(observationCount / 3 + 8);
    engine.positions.reserve(raw.points.size() + raw.unpositionedPoints.size() + 8);

    std::unordered_set<std::string_view> drawingOnly;
    if (Status status = seedControl(engine, drawingOnly); !status) {
        return status.error();
    }
    seedEntered(engine);

    // Phase A.
    {
        std::vector<Shot> shots;
        std::unordered_map<std::size_t, std::size_t> byPointing;
        for (std::size_t s = 0; s < raw.stations.size(); ++s) {
            reduceSetup(engine, s, shots, byPointing);
        }
    }
    // Observations belonging to no setup, GNSS positions, and GNSS vectors in
    // grid terms.
    {
        std::vector<std::size_t> looseRows;
        looseRows.reserve(raw.observations.size());
        engine.globalPositions.resize(raw.observations.size());
        GnssSeedNotices notices;
        for (std::size_t i = 0; i < raw.observations.size(); ++i) {
            recordLoose(engine, raw.observations[i], static_cast<std::size_t>(-1), {});
            looseRows.push_back(engine.report.observations.size() - 1);
            seedGnss(engine, i, looseRows.back(), notices);
        }
        if (notices.toReferencePoint > 0) {
            engine.warn(std::to_string(notices.toReferencePoint) +
                        " GNSS position(s) have antenna heights to the antenna's reference "
                        "point: the offset from there to the phase centre, a few centimetres "
                        "and not in the file, remains in their heights.");
        }
        convertGnssVectors(engine, looseRows);
    }

    placeSetups(engine);
    radiateGnssVectors(engine);

    engine.flushSetupNotices();

    // The adjustment.
    if (settings.method == AdjustmentMethod::Traverse) {
        if (Status status = adjustAsTraverse(engine); !status) {
            return status.error();
        }
    } else if (settings.method == AdjustmentMethod::Network) {
        if (Status status = adjustAsNetwork(engine); !status) {
            return status.error();
        }
    }

    engine.flushSetupNotices();

    // ---- Outputs ----
    ReductionOutcome outcome;
    std::unordered_map<std::string_view, const ComputedPoint*> previous;
    for (const ComputedPoint& point : context.previous) {
        previous.emplace(point.id, &point);
    }
    outcome.points.reserve(engine.positionOrder.size());
    engine.report.coordinates.reserve(engine.positionOrder.size());
    for (const std::string_view id : engine.positionOrder) {
        const Position& position = engine.positions.at(id);
        if (position.origin == PositionOrigin::FileOnly) {
            continue;
        }
        CoordinateReport coordinate;
        coordinate.pointId = id;
        coordinate.northing = position.northing;
        coordinate.easting = position.easting;
        coordinate.elevation = position.height;
        coordinate.sigmaNorthing = position.sigmaNorthing;
        coordinate.sigmaEasting = position.sigmaEasting;
        coordinate.sigmaElevation = position.sigmaHeight;
        coordinate.method = position.method;
        if (const auto it = previous.find(id); it != previous.end()) {
            coordinate.shiftNorthing = position.northing - it->second->northing;
            coordinate.shiftEasting = position.easting - it->second->easting;
            if (position.height && it->second->elevation) {
                coordinate.shiftElevation = *position.height - *it->second->elevation;
            }
        }
        if (drawingOnly.count(id) == 0) {
            ComputedPoint point;
            point.id = id;
            point.northing = position.northing;
            point.easting = position.easting;
            point.elevation = position.height;
            point.sigmaNorthing = position.sigmaNorthing;
            point.sigmaEasting = position.sigmaEasting;
            point.sigmaElevation = position.sigmaHeight;
            point.method = position.method;
            outcome.points.push_back(std::move(point));
        }
        engine.report.coordinates.push_back(std::move(coordinate));
    }

    outcome.reduced = reducedProject(engine);
    SurveyProject& reduced = outcome.reduced;
    {
        std::unordered_map<std::string_view, const ComputedPoint*> computed;
        computed.reserve(outcome.points.size());
        for (const ComputedPoint& point : outcome.points) {
            if (point.method != ComputationMethod::Control) {
                computed.emplace(point.id, &point);
            }
        }
        for (SurveyPoint& point : reduced.points) {
            if (const auto it = computed.find(point.id); it != computed.end()) {
                point.northing = it->second->northing;
                point.easting = it->second->easting;
                if (it->second->elevation) {
                    point.elevation = it->second->elevation;
                }
                point.coordinateSource = CoordinateSource::Calculated;
            }
        }
        std::vector<UnpositionedPoint> stillUnpositioned;
        for (UnpositionedPoint& unpositioned : reduced.unpositionedPoints) {
            const auto it = computed.find(unpositioned.id);
            if (it == computed.end()) {
                stillUnpositioned.push_back(std::move(unpositioned));
                continue;
            }
            SurveyPoint point;
            point.id = std::move(unpositioned.id);
            point.northing = it->second->northing;
            point.easting = it->second->easting;
            point.elevation = it->second->elevation;
            point.code = std::move(unpositioned.code);
            point.description = std::move(unpositioned.description);
            point.metadata = std::move(unpositioned.metadata);
            point.coordinateSource = CoordinateSource::Calculated;
            point.source = std::move(unpositioned.source);
            reduced.points.push_back(std::move(point));
        }
        reduced.unpositionedPoints = std::move(stillUnpositioned);
    }

    outcome.report = std::move(engine.report);
    return outcome;
}

} // namespace katana::survey
