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
#include <string>
#include <type_traits>
#include <unordered_set>
#include <utility>
#include <variant>

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
    const std::string& id = station.setup.id;
    AtmosphereDecision decision;

    std::optional<double> computed;
    std::string weather;
    if (instrument.temperatureCelsius && instrument.pressureHectopascals) {
        double humidity = kAssumedHumidityPercent;
        std::string humidityNote = " (humidity not recorded, 60 % assumed)";
        if (instrument.relativeHumidityPercent) {
            humidity = *instrument.relativeHumidityPercent;
            humidityNote = " and " + formatNumber(humidity, 0) + " %";
        }
        computed = atmosphericPpm(*instrument.temperatureCelsius,
                                  *instrument.pressureHectopascals, humidity);
        weather = formatNumber(*instrument.temperatureCelsius, 1) + " C, " +
                  formatNumber(*instrument.pressureHectopascals, 1) + " hPa" + humidityNote;
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
            engine.warn("Setup " + id +
                            ": the file does not say whether the instrument applied an "
                            "atmospheric correction, so none was applied. Choose Recompute or "
                            "Fixed if the distances are raw.",
                        station.source);
            return decision;
        }
        if (computed) {
            decision = {true, *computed, ppmText(*computed)};
            engine.report.setups.back().instrument.atmosphericPpm = *computed;
            engine.warn("Setup " + id + ": atmospheric correction " + ppmText(*computed) +
                            " computed from " + weather + " (IUGG 1999, 658 nm, reference 12 C, "
                            "1013.25 hPa, 60 %).",
                        station.source);
            return decision;
        }
        if (instrument.atmosphericPpm) {
            decision = {true, *instrument.atmosphericPpm,
                        ppmText(*instrument.atmosphericPpm) + " as recorded"};
            return decision;
        }
        engine.warn("Setup " + id +
                        ": the instrument did not apply an atmospheric correction and the file "
                        "records no weather to compute one, so none was applied.",
                    station.source);
        return decision;
    case AtmosphericCorrection::Recompute: {
        if (!computed) {
            engine.warn("Setup " + id +
                            ": no temperature and pressure recorded, so the atmospheric "
                            "correction cannot be recomputed; distances used as recorded.",
                        station.source);
            return decision;
        }
        double net = *computed;
        std::string note = ppmText(*computed);
        if (instrument.atmosphericPpmState == CorrectionState::Applied) {
            if (!instrument.atmosphericPpm) {
                engine.warn("Setup " + id +
                                ": the instrument applied an atmospheric correction but the file "
                                "does not say how much, so it cannot be taken out; distances "
                                "used as recorded.",
                            station.source);
                return decision;
            }
            // (1 + new) / (1 + old), as ppm.
            net = ((1.0 + *computed * 1e-6) / (1.0 + *instrument.atmosphericPpm * 1e-6) - 1.0) *
                  1e6;
            note = ppmText(*computed) + " for " + ppmText(*instrument.atmosphericPpm);
        } else if (instrument.atmosphericPpmState == CorrectionState::Unknown) {
            engine.warn("Setup " + id +
                            ": the file does not say whether the instrument applied an "
                            "atmospheric correction; the recomputed one was applied as if it had "
                            "not.",
                        station.source);
        }
        engine.warn("Setup " + id + ": atmospheric correction recomputed as " +
                        ppmText(*computed) + " from " + weather + ".",
                    station.source);
        return AtmosphereDecision{true, net, note};
    }
    case AtmosphericCorrection::Fixed:
        if (instrument.atmosphericPpmState == CorrectionState::Applied) {
            return decision;
        }
        if (instrument.atmosphericPpmState == CorrectionState::Unknown) {
            engine.warn("Setup " + id +
                            ": the file does not say whether the instrument applied an "
                            "atmospheric correction; the fixed value was applied as if it had "
                            "not.",
                        station.source);
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
            engine.warn("Setup " + station.setup.id + ": " + what, station.source);
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
            correct(engine, shot.directionRow, CorrectionKind::FaceMean,
                    normalizeAngleSigned(direction - shot.direction->direction), std::nullopt,
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
        if (std::abs(normalizeAngleSigned(r - l)) > kHalfPi) {
            r -= kPi;
        }
        const double difference = normalizeAngleSigned(r - l);
        const double mean = normalizeAngle(l + 0.5 * difference);
        check.horizontalSpread = std::abs(difference);
        pointing.direction = mean;
        pointing.sigmaDirection = settings.apriori.direction / std::sqrt(2.0);
        correct(engine, left.directionRow, CorrectionKind::FaceMean,
                normalizeAngleSigned(mean - l));
        correct(engine, right.directionRow, CorrectionKind::FaceMean,
                normalizeAngleSigned(mean - right.direction->direction), std::nullopt,
                "less 180 deg");
        setReduced(engine, left.directionRow, mean);
        setReduced(engine, right.directionRow, mean);
    } else if (left.direction || right.direction) {
        const Shot& one = left.direction ? left : right;
        double direction = one.direction->direction;
        if (&one == &right) {
            direction = normalizeAngle(direction - kPi);
            correct(engine, one.directionRow, CorrectionKind::FaceMean,
                    normalizeAngleSigned(direction - one.direction->direction), std::nullopt,
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
        correct(engine, left.zenithRow, CorrectionKind::FaceMean, mean - leftBefore,
                std::nullopt, "index " + formatSeconds(0.5 * (left.zenithValue - right.zenithValue)));
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
        for (const std::size_t i : indices) {
            if (settings.faces == FaceHandling::Separate) {
                singles.push_back(i);
            } else if (shots[i].face == Face::Left) {
                lefts.push_back(i);
            } else if (shots[i].face == Face::Right) {
                rights.push_back(i);
            } else {
                singles.push_back(i);
            }
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

} // namespace

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
    const ReductionSettings& settings = engine.settings;
    const ReductionContext& context = engine.context;
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
    const Position* backsightPosition =
        backsight.empty() || backsight == station.setup.pointId ? nullptr : engine.find(backsight);
    std::optional<double> backsightAzimuth;
    state.orientationAssumed = false;
    if (backsightPosition != nullptr &&
        std::hypot(backsightPosition->northing - here.northing,
                   backsightPosition->easting - here.easting) > 1e-4) {
        backsightAzimuth = normalizeAngle(std::atan2(backsightPosition->easting - here.easting,
                                                     backsightPosition->northing - here.northing));
    } else if (station.backsightAzimuth) {
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
    setup.orientationCorrection = state.orientation;
    if (!state.orientation && reradiate == nullptr) {
        engine.warn("Setup " + station.setup.id +
                        (backsight.empty()
                             ? std::string(" has no backsight")
                             : " cannot be oriented: its backsight " + std::string(backsight) +
                                   " has no position and no circle setting was recorded") +
                        ", so its directions are not oriented and its targets are not radiated.",
                    station.source);
    }

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
    std::optional<double> geoid;
    if ((settings.heightReduction == HeightReduction::Ellipsoid ||
         settings.gridScale == GridScale::FromProjection) &&
        context.geoidSeparation && !settings.useCombinedFactor) {
        geoid = context.geoidSeparation(here.northing, here.easting);
    }
    bool warnedHeight = false;
    bool warnedGeoid = false;
    bool warnedScale = false;
    const double instrumentHeight = station.setup.instrumentHeight;

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
                        std::nullopt, state.orientationAssumed ? "circle as set" : std::string{});
                setReduced(engine, index, *pointing.azimuth);
            }
        }

        pointing.gridDistance.reset();
        if (pointing.horizontal) {
            double distance = *pointing.horizontal;
            const auto factor = [&](CorrectionKind kind, double value, std::string note) {
                for (std::size_t i = 0; i < pointing.distanceRowCount; ++i) {
                    correct(engine, pointing.distanceRows[i], kind, distance * (value - 1.0), value,
                            note);
                }
                distance *= value;
            };
            if (settings.useCombinedFactor) {
                factor(CorrectionKind::CombinedFactor, settings.combinedFactor, {});
            } else {
                if (settings.heightReduction != HeightReduction::None) {
                    if (!here.height) {
                        if (!warnedHeight) {
                            warnedHeight = true;
                            engine.warn("Setup " + station.setup.id + ": its point " +
                                            station.setup.pointId +
                                            " has no height, so its distances were not reduced for "
                                            "height.",
                                        station.source);
                        }
                    } else {
                        double height =
                            *here.height + instrumentHeight + 0.5 * pointing.measuredVertical;
                        bool ok = true;
                        if (settings.heightReduction == HeightReduction::Ellipsoid) {
                            if (geoid) {
                                height += *geoid;
                            } else {
                                ok = false;
                                if (!warnedGeoid) {
                                    warnedGeoid = true;
                                    engine.warn("Setup " + station.setup.id +
                                                    ": the drawing gives no geoid separation there, so "
                                                    "its distances were not reduced to the ellipsoid.",
                                                station.source);
                                }
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
                    } else if (!warnedScale) {
                        warnedScale = true;
                        engine.warn("Setup " + station.setup.id +
                                        ": the drawing's projection gives no scale factor there, so "
                                        "its distances were not reduced to grid.",
                                    station.source);
                    }
                }
            }
            pointing.gridDistance = distance;
            for (std::size_t i = 0; i < pointing.distanceRowCount; ++i) {
                setReduced(engine, pointing.distanceRows[i], distance);
            }
        }

        if (reradiate != nullptr && reradiate->count(pointing.target) == 0 &&
            pointing.target != backsight) {
            continue;
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
            engine.place(a.target, computed);
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

// GNSS positions: the grid ones as they are, the global ones through the
// context's conversion.
void seedGnss(Engine& engine, const Observation& observation, std::size_t rowIndex)
{
    const ReductionSettings& settings = engine.settings;
    const ReductionContext& context = engine.context;
    std::optional<GridPosition> grid;
    std::string_view pointId;
    double sigmaHorizontal = settings.apriori.gnssHorizontal;
    double sigmaVertical = settings.apriori.gnssVertical;
    std::optional<double> height;
    if (const auto* position = std::get_if<GnssPositionObservation>(&observation)) {
        pointId = position->point;
        grid = GridPosition{position->northing, position->easting, position->elevation};
        height = position->elevation;
        sigmaHorizontal = std::max(position->sigmaNorthing, position->sigmaEasting);
        sigmaVertical = position->sigmaElevation;
    } else if (const auto* global = std::get_if<GnssGlobalPositionObservation>(&observation)) {
        pointId = global->point;
        if (global->geocentric && context.geocentricToGrid) {
            grid = context.geocentricToGrid(*global->geocentric);
        } else if (global->geodetic && context.geodeticToGrid) {
            grid = context.geodeticToGrid(*global->geodetic);
        }
        if (!grid) {
            engine.warn("GNSS position of " + global->point +
                            " not used: the drawing's coordinate system cannot convert it to "
                            "grid.",
                        global->source);
            reject(engine, rowIndex, "no conversion to the drawing's grid");
            return;
        }
        if (global->geodetic && settings.useFileCovariances && global->covariance.stated()) {
            // Local north / east / up.
            sigmaHorizontal = std::sqrt(std::max(global->covariance.xx, global->covariance.yy));
            sigmaVertical = std::sqrt(global->covariance.zz);
        }
        height = grid->ellipsoidalHeight;
        if (context.geoidSeparation) {
            if (const auto separation = context.geoidSeparation(grid->northing, grid->easting)) {
                height = grid->ellipsoidalHeight - *separation;
            }
        }
        if (global->solution == GnssSolution::Float || global->solution == GnssSolution::Autonomous) {
            engine.warn("GNSS position of " + global->point + " is a " +
                            toString(global->solution) + " solution.",
                        global->source);
        }
        ReportObservation& reportRow = row(engine, rowIndex);
        reportRow.corrections.push_back(AppliedCorrection{
            CorrectionKind::Other, 0.0, std::nullopt,
            "to grid N " + formatNumber(grid->northing, 3) + " E " +
                formatNumber(grid->easting, 3) + " H " + formatNumber(*height, 3)});
    } else {
        return;
    }
    if (engine.find(pointId)) {
        return; // control or an entered point wins
    }
    Position position;
    position.northing = grid->northing;
    position.easting = grid->easting;
    position.height = height;
    position.origin = PositionOrigin::Gnss;
    position.method = ComputationMethod::Gnss;
    position.sigmaNorthing = sigmaHorizontal;
    position.sigmaEasting = sigmaHorizontal;
    position.sigmaHeight = sigmaVertical;
    engine.place(pointId, position);
}

// ---- Outputs -----------------------------------------------------------------------------

void addReducedObservations(const Engine& engine, SurveyProject& reduced)
{
    for (std::size_t s = 0; s < reduced.stations.size(); ++s) {
        SurveyStation& station = reduced.stations[s];
        std::vector<Observation> kept;
        for (Observation& observation : station.observations) {
            const bool pointingKind =
                std::holds_alternative<HorizontalDirectionObservation>(observation) ||
                std::holds_alternative<ZenithAngleObservation>(observation) ||
                std::holds_alternative<VerticalAngleObservation>(observation) ||
                std::holds_alternative<DistanceObservation>(observation);
            if (!pointingKind) {
                kept.push_back(std::move(observation));
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
    engine.filePoints.reserve(raw.points.size());
    for (const SurveyPoint& point : raw.points) {
        engine.filePoints.emplace(point.id, &point);
    }
    std::size_t observationCount = raw.observations.size();
    for (const SurveyStation& station : raw.stations) {
        observationCount += station.observations.size();
    }
    engine.report.observations.reserve(observationCount + observationCount / 3 + 8);
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
    // Observations belonging to no setup, and GNSS positions.
    for (const Observation& observation : raw.observations) {
        recordLoose(engine, observation, static_cast<std::size_t>(-1), {});
        seedGnss(engine, observation, engine.report.observations.size() - 1);
    }

    // Phase B: setups in file order as their stations become known; a setup
    // whose point nothing positions falls back to the file's own coordinates
    // for that point, and the report says so.
    std::vector<bool> done(raw.stations.size(), false);
    std::size_t remaining = raw.stations.size();
    while (remaining > 0) {
        bool progress = false;
        for (std::size_t s = 0; s < raw.stations.size(); ++s) {
            if (done[s] || !engine.find(raw.stations[s].setup.pointId)) {
                continue;
            }
            orientAndRadiate(engine, s);
            done[s] = true;
            --remaining;
            progress = true;
        }
        if (progress) {
            continue;
        }
        bool fellBack = false;
        for (std::size_t s = 0; s < raw.stations.size() && !fellBack; ++s) {
            if (done[s]) {
                continue;
            }
            const std::string& pointId = raw.stations[s].setup.pointId;
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
            engine.warn("Setup " + raw.stations[s].setup.id + " stands on " + pointId +
                            ", which is not control and was not computed; the file's own "
                            "coordinates for it were used.",
                        raw.stations[s].source);
            fellBack = true;
        }
        if (!fellBack) {
            for (std::size_t s = 0; s < raw.stations.size(); ++s) {
                if (!done[s]) {
                    engine.warn("Setup " + raw.stations[s].setup.id + " stands on " +
                                    raw.stations[s].setup.pointId +
                                    ", which has no position: nothing was computed from it.",
                                raw.stations[s].source);
                }
            }
            break;
        }
    }

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

    outcome.reduced = raw;
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
    addReducedObservations(engine, reduced);

    outcome.report = std::move(engine.report);
    return outcome;
}

} // namespace katana::survey
