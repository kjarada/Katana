#pragma once

// The working state of one reduceAndAdjust call, shared by the files that
// implement it: reduction.cpp (pairing, corrections, orientation, radiation),
// reduction_adjust.cpp (traverse and network). Private to src/katana_survey.
//
// Ids are held as string_views into the raw project, the settings and the
// context, which all outlive the call: at 100 000 observations a copied id per
// observation is the largest cost of the whole reduction.

#include <array>
#include <cstddef>
#include <optional>
#include <string>
#include <string_view>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#include "katana/survey/reduction.hpp"

namespace katana::survey::detail {

// Where a position came from, in the order of authority.
enum class PositionOrigin {
    Control,  // a ControlSelection: held
    Entered,  // the file's point, keyed in or published (CoordinateSource::Entered)
    Gnss,     // a GNSS position converted to grid
    Computed, // radiated or adjusted here
    FileOnly, // the file's own coordinates, used only because nothing better exists
};

struct Position {
    double northing = 0.0;
    double easting = 0.0;
    std::optional<double> height{};
    PositionOrigin origin = PositionOrigin::Computed;
    ComputationMethod method = ComputationMethod::Radiation;
    bool heldHorizontal = false;
    bool heldHeight = false;
    std::optional<double> sigmaNorthing{};
    std::optional<double> sigmaEasting{};
    std::optional<double> sigmaHeight{};
    // The setup it was radiated from, for "side shot" bookkeeping.
    std::size_t fromSetup = static_cast<std::size_t>(-1);
};

// One reduced pointing: a face-left / face-right mean, or a single face.
struct ReducedPointing {
    std::size_t setup = 0;
    std::string_view target;
    Face face = Face::Unknown; // Left for a pair
    bool paired = false;
    std::size_t leftIndex = 0;
    std::size_t rightIndex = 0;

    // FL-equivalent values after the face mean.
    std::optional<double> direction{}; // circle reading
    std::optional<double> zenith{};
    std::optional<double> slope{}; // corrected slope distance
    // After slope to horizontal and curvature: ground, at the instrument.
    std::optional<double> horizontal{};
    // Mark to mark, after heights and curvature.
    std::optional<double> heightDifference{};
    double measuredVertical = 0.0; // X = S cos z, for the line's mean height
    double targetHeight = 0.0;
    bool recordedHorizontal = false; // the file gave a horizontal distance

    // After phase B.
    std::optional<double> gridDistance{};
    std::optional<double> azimuth{};

    // A-priori standard deviations of the reduced values.
    double sigmaDirection = 0.0; // pointing only, radians
    double sigmaZenith = 0.0;
    double sigmaDistance = 0.0; // EDM part, metres

    // Report rows (indices into ReductionReport::observations) whose chains
    // continue with this pointing's later corrections.
    std::array<std::size_t, 2> directionRows{};
    std::size_t directionRowCount = 0;
    std::array<std::size_t, 2> distanceRows{};
    std::size_t distanceRowCount = 0;
    std::optional<std::size_t> heightRow{};
    // Every raw row this pointing came from, for a rejection by the adjustment.
    std::array<std::size_t, 6> rawRows{};
    std::size_t rawRowCount = 0;

    bool rejected = false;
    SourceRecord const* source = nullptr; // the first raw observation's record
};

// A horizontal angle or azimuth the file recorded as such, kept for the
// network and for radiation.
struct RecordedAngle {
    std::size_t setup = 0;
    std::string_view at;
    std::string_view from; // empty for an azimuth
    std::string_view to;
    double value = 0.0;
    double sigma = 0.0;
    std::size_t row = 0;
};

// Per setup, after phase B.
struct SetupState {
    bool positioned = false;
    std::optional<double> orientation{}; // added to a circle reading to give an azimuth
    bool orientationAssumed = false;     // from a set circle, not from coordinates
    std::size_t reportIndex = 0;
    std::vector<std::size_t> pointings{}; // indices into Engine::pointings
};

struct Engine {
    const SurveyProject& raw;
    const ReductionSettings& settings;
    const ReductionContext& context;
    ReductionReport report;

    std::vector<ReducedPointing> pointings;
    std::vector<RecordedAngle> angles;
    std::vector<SetupState> setups;

    std::unordered_map<std::string_view, Position> positions;
    // The order points were first positioned in: the order of the output.
    std::vector<std::string_view> positionOrder;
    // The file's own coordinates, by id.
    std::unordered_map<std::string_view, const SurveyPoint*> filePoints;
    // Setup indices occupying each point.
    std::unordered_map<std::string_view, std::vector<std::size_t>> occupiedBy;

    Engine(const SurveyProject& project, const ReductionSettings& reductionSettings,
           const ReductionContext& reductionContext)
        : raw(project), settings(reductionSettings), context(reductionContext)
    {
    }

    void warn(std::string text, SourceRecord source = {})
    {
        report.warnings.push_back(ReportMessage{std::move(text), std::move(source)});
    }

    Position* find(std::string_view id)
    {
        const auto it = positions.find(id);
        return it == positions.end() ? nullptr : &it->second;
    }

    // Sets a position; a first positioning also records the output order.
    void place(std::string_view id, const Position& position)
    {
        auto [it, inserted] = positions.try_emplace(id, position);
        if (inserted) {
            positionOrder.push_back(id);
        } else {
            it->second = position;
        }
    }
};

// ---- reduction.cpp ------------------------------------------------------------------

// Phase B for one setup: orientation, the distance factors that need a
// position, and radiation of its targets. With `reradiate` null (the first
// pass) a target is placed only where no position exists yet, and a target
// that already has one becomes a check. After an adjustment moved the
// stations, `reradiate` names the side shots, and only those are placed again
// (overwriting what the first pass gave them).
void orientAndRadiate(Engine& engine, std::size_t setupIndex,
                      const std::unordered_set<std::string_view>* reradiate = nullptr);

// The mean circle reading of a setup's reduced pointings to `target`, absent
// when none has a direction.
[[nodiscard]] std::optional<double> meanDirection(const Engine& engine, std::size_t setupIndex,
                                                  std::string_view target);

// ---- reduction_adjust.cpp ----------------------------------------------------------

// AdjustmentMethod::Traverse: finds the traverse in the setups, adjusts it by
// the chosen rule, and re-radiates the side shots from the adjusted stations.
[[nodiscard]] katana::core::Status adjustAsTraverse(Engine& engine);

// AdjustmentMethod::Network: least squares over every observation that is not
// a side shot, then the side shots radiated from the adjusted stations.
[[nodiscard]] katana::core::Status adjustAsNetwork(Engine& engine);

// Text helpers shared by the two.
[[nodiscard]] std::string formatSeconds(double radians, int decimals = 1);
[[nodiscard]] std::string formatMillimetres(double metres, int decimals = 1);
[[nodiscard]] std::string formatNumber(double value, int decimals);

} // namespace katana::survey::detail
