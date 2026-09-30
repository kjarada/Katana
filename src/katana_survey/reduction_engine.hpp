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

// A GNSS vector in grid terms: a grid baseline as the file gave it, or a
// geocentric one converted through the context (reduction_gnss.cpp).
struct GridVector {
    std::string_view from;
    std::string_view to;
    double deltaNorthing = 0.0;
    double deltaEasting = 0.0;
    // Mark to mark; absent when an antenna height could not be reduced.
    std::optional<double> deltaHeight{};
    double sigmaNorthing = 0.0;
    double sigmaEasting = 0.0;
    double sigmaHeight = 0.0;
    std::size_t row = 0;                  // the vector's report row
    std::optional<std::size_t> heightRow{}; // its height difference row
    SourceRecord const* source = nullptr;
    bool radiated = false; // placed its far end, or checked it
};

// A GNSS position of a point converted to grid and taken down to the mark,
// one per position observation: a point occupied twice has two, and each is
// checked against the first (radiation) or enters the network with its own
// value.
struct GlobalGridPosition {
    std::string_view point;
    double northing = 0.0;
    double easting = 0.0;
    std::optional<double> height{}; // of the mark; absent when the antenna could not be reduced
    double sigmaHorizontal = 0.0;
    double sigmaVertical = 0.0;
    std::size_t row = 0;
    const SourceRecord* source = nullptr;
};

// Per setup, after phase B.
struct SetupState {
    bool positioned = false;
    std::optional<double> orientation{}; // added to a circle reading to give an azimuth
    bool orientationAssumed = false;     // from a set circle, not from coordinates
    // Of those, the ones oriented on the azimuth the file states for the
    // backsight (SurveyStation::statedBacksightAzimuth) rather than the circle.
    bool orientationStated = false;
    // Whether a circle set on a NAMED backsight that has no position may
    // stand in for its grid azimuth - or the azimuth the file states for it
    // be used. Off until nothing else can place the backsight: a controller
    // records the circle it set (often 0 00 00) whether or not it is a grid
    // azimuth, so taking it at once would orient the setup wrongly and leave
    // the right orientation, which a later setup may give by radiating the
    // backsight, unused; and coordinates, where they come, are the
    // reduction's own azimuth, so a stated one waits for them as well.
    bool acceptCircleAsSet = false;
    // Notices already given for this setup: orientAndRadiate runs again
    // after the backsight is placed and after an adjustment, and the report
    // should say each thing once.
    bool warnedHeight = false;
    bool warnedGeoid = false;
    bool warnedScale = false;
    std::size_t reportIndex = 0;
    std::vector<std::size_t> pointings{}; // indices into Engine::pointings
    // Set when resectSetup positioned the station. Such a setup is oriented
    // by its resection, not its backsight: on the weighted mean of azimuth
    // less reading over these pointings, each with the standard deviation its
    // direction had in the resection - the least-squares orientation for the
    // station where it stands, so it follows the station when an adjustment
    // moves it. The pointings its resection used are not checks when it
    // radiates - their residuals are the resection's - but a later pointing
    // to one of the same points (a controller's check after its block) is.
    bool resected = false;
    std::vector<std::pair<std::size_t, double>> resectionDirections{};
    std::unordered_set<std::size_t> resectionPointings{};
    std::unordered_set<std::string_view> resectionTargets{};
    // The record that ends the observations the file's own resection was
    // computed from (kResectionEndMetadata), when the file says: only the
    // pointings before it may enter the reduction's resection.
    std::optional<std::size_t> resectionBlockEnd{};
};

struct Engine {
    const SurveyProject& raw;
    const ReductionSettings& settings;
    const ReductionContext& context;
    ReductionReport report;

    std::vector<ReducedPointing> pointings;
    std::vector<RecordedAngle> angles;
    std::vector<SetupState> setups;
    std::vector<GridVector> vectors;
    // Indexed like raw.observations: the converted GNSS position of each
    // position observation, empty for every other kind and for one that
    // could not be converted.
    std::vector<std::optional<GlobalGridPosition>> globalPositions;

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

    // A notice that is the same sentence for many setups ("the file does not
    // say whether the prism constant is in the distances") is collected and
    // written once, naming the setups: a thousand identical lines would bury
    // the one warning that matters.
    void warnSetup(const SurveyStation& station, std::string text)
    {
        const auto [it, inserted] = setupNoticeSlot.try_emplace(text, setupNotices.size());
        if (inserted) {
            setupNotices.push_back({std::move(text), {}});
        }
        setupNotices[it->second].second.push_back(&station);
    }

    void flushSetupNotices()
    {
        for (auto& [text, stations] : setupNotices) {
            std::string who;
            if (stations.size() == 1) {
                who = "Setup " + stations.front()->setup.id;
            } else {
                who = std::to_string(stations.size()) + " setups (";
                for (std::size_t i = 0; i < stations.size() && i < 5; ++i) {
                    who += (i == 0 ? "" : ", ") + stations[i]->setup.id;
                }
                who += stations.size() > 5 ? ", ...)" : ")";
            }
            warn(who + ": " + text, stations.front()->source);
        }
        setupNotices.clear();
        setupNoticeSlot.clear();
    }

    std::vector<std::pair<std::string, std::vector<const SurveyStation*>>> setupNotices;
    std::unordered_map<std::string, std::size_t> setupNoticeSlot;

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
//
// The orientation comes from the backsight's coordinates. Only where no
// coordinates can come - no backsight is named, or
// SetupState::acceptCircleAsSet says the named one will not be placed - does
// the azimuth the file states for the backsight give it, or failing that the
// circle set on the backsight, standing in for its grid azimuth. Otherwise a
// setup whose backsight has no position yet is left unoriented and radiates
// nothing, to be tried again.
void orientAndRadiate(Engine& engine, std::size_t setupIndex,
                      const std::unordered_set<std::string_view>* reradiate = nullptr);

// Whether an adjustment has rejected the direction, the distance or the height
// of `pointing` - marked its report rows, as a resection's outlier test does
// before the network runs. What a resection rejected, its setup's orientation
// leaves out (resectSetup), a mean direction leaves out (meanDirection), and
// the network takes no more of, nor as its reference direction
// (adjustAsNetwork). (A pointing the face-pair test excluded is `rejected` as
// a whole instead.)
[[nodiscard]] bool directionRejected(const Engine& engine, const ReducedPointing& pointing);
[[nodiscard]] bool distanceRejected(const Engine& engine, const ReducedPointing& pointing);
[[nodiscard]] bool heightRejected(const Engine& engine, const ReducedPointing& pointing);

// The mean circle reading of a setup's reduced pointings to `target`, absent
// when none has a direction that was not rejected.
[[nodiscard]] std::optional<double> meanDirection(const Engine& engine, std::size_t setupIndex,
                                                  std::string_view target);

// A pointing's horizontal distance with phase B's factors as they would be from
// a station at `here` (at the setup, the pointing having no azimuth yet),
// written nowhere: what a resection solves with before its station exists.
// Absent without a horizontal distance.
[[nodiscard]] std::optional<double> gridDistanceFrom(Engine& engine, std::size_t setupIndex,
                                                     const ReducedPointing& pointing,
                                                     const Position& here);

// ---- reduction_gnss.cpp ------------------------------------------------------------

// The drawing's grid position of an earth-centred point, through
// geocentricToGrid or, when the drawing gives only geodeticToGrid, through
// that after the GRS80 conversion (the ellipsoid GNSS frames use). Absent
// when the drawing can do neither.
[[nodiscard]] std::optional<GridPosition> gridOfGeocentric(const ReductionContext& context,
                                                           const GeocentricCoordinate& point);
// The same for a geodetic point.
[[nodiscard]] std::optional<GridPosition> gridOfGeodetic(const ReductionContext& context,
                                                         const GeodeticCoordinate& point);

// Whether an antenna height can be taken off as a vertical offset: a
// vertical or phase-centre height, or no height at all. A slant height or
// one to a mark the model does not name cannot, and neither can a height
// with no stated method (it could be slant).
[[nodiscard]] bool reducibleAntenna(const GnssAntenna& antenna);
// "1.500 m Vertical (bottom of mount)", for the report.
[[nodiscard]] std::string antennaWords(const GnssAntenna& antenna);

// Turns the file's GNSS vectors into grid vectors: a grid baseline as it is,
// a geocentric one through the context at its base's global position.
// `rows[i]` is the report row of raw.observations[i]. A vector that cannot be
// converted is rejected on its row with the reason, never dropped.
void convertGnssVectors(Engine& engine, const std::vector<std::size_t>& rows);

// Places the far end of every vector whose base has a position, following
// chains; a far end already positioned becomes a check. With `reradiate`
// (after an adjustment moved the bases) only those targets are placed again.
// Returns whether anything was placed.
bool radiateGnssVectors(Engine& engine,
                        const std::unordered_set<std::string_view>* reradiate = nullptr);

// ---- reduction_adjust.cpp ----------------------------------------------------------

// AdjustmentMethod::Traverse: finds the traverse in the setups, adjusts it by
// the chosen rule, and re-radiates the side shots from the adjusted stations.
[[nodiscard]] katana::core::Status adjustAsTraverse(Engine& engine);

// AdjustmentMethod::Network: least squares over every observation that is not
// a side shot, then the side shots radiated from the adjusted stations.
[[nodiscard]] katana::core::Status adjustAsNetwork(Engine& engine);

// Whether `pointing` of setup `setupIndex` may enter the setup's resection:
// every pointing, unless the file marks where the observations its field
// software resected from end (SetupState::resectionBlockEnd) - then those
// read before that record. The ones after are checks of the station.
[[nodiscard]] bool inResectionBlock(const Engine& engine, std::size_t setupIndex,
                                    const ReducedPointing& pointing);

// Positions the station of setup `setupIndex` by resection from its reduced
// pointings to points already placed, held as they are: the least squares of
// its directions (one set, one orientation unknown) and horizontal distances,
// run from every start its observations give, and then of its trigonometric
// height differences to the targets with heights, weighted by the reduction's
// a-priori precision for an instrument that is itself the unknown (its
// centring and height go to the station's precision, not to each pointing).
// The station has no position, or one another setup radiated, which it
// replaces and reports as a check. Places the station
// (ComputationMethod::Resection), marks the setup resected (SetupState),
// reports it (ReductionReport::resections) and returns true; or returns false
// and puts in `why`, as a clause, what refused it: too few placed points, a
// value that is not finite, a distance that is not positive, directions alone
// exactly on one line with the station or on the circle through it, a least
// squares that fails from every start, two positions its observations fit
// alike, or a station its observations' a-priori precision leaves uncertain
// by more than its least squares' linear model holds over. A refusal leaves
// nothing behind: no warning of its least squares, no rejected observation.
[[nodiscard]] bool resectSetup(Engine& engine, std::size_t setupIndex, std::string& why);

// Why a setup whose station has no position was not a resection candidate, as
// a clause - the placed points it observes against what a resection needs -
// or empty when it observes none.
[[nodiscard]] std::string resectionShortfall(const Engine& engine, std::size_t setupIndex);

// Text helpers shared by the two.
[[nodiscard]] std::string formatSeconds(double radians, int decimals = 1);
[[nodiscard]] std::string formatMillimetres(double metres, int decimals = 1);
[[nodiscard]] std::string formatNumber(double value, int decimals);

} // namespace katana::survey::detail
