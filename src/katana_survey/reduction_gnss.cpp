// GNSS vectors in reduceAndAdjust: what an RTK controller records between a
// base and a rover (a Trimble job's ECEF deltas, a Topcon RW5 vector) turned
// into grid terms, then radiated from the base like a total-station shot, or
// handed to the network as a baseline.
//
// A geocentric vector is converted at its base: the base's global position
// (a GnssGlobalPositionObservation of it in the file, or the far end of an
// earlier vector) and base + delta both go through the context's
// geocentricToGrid (or its geodeticToGrid after GRS80, when that is the one
// the drawing gives), and the vector in grid is their difference. The
// difference is taken on the drawing's own projection, so the grid scale and
// the convergence are in it; no scale factor is applied to it again.
//
// Heights: the difference of the two ellipsoidal heights, less the change in
// geoid separation when the drawing gives one, plus the base's antenna height
// less the rover's - an RTK vector runs between the antennas' phase centres
// (the receiver can measure nothing else), so the antenna heights take it down
// to the marks. The horizontal part needs no such reduction: the two plumb
// lines are parallel to 0.2 mm over a kilometre at 2 m.

#include <cmath>
#include <string>

#include "katana/math/numerics.hpp"
#include "reduction_engine.hpp"
#include "reduction_formulas.hpp"

namespace katana::survey::detail {

namespace {

void rejectRow(Engine& engine, std::size_t index, std::string reason)
{
    ReportObservation& row = engine.report.observations[index];
    row.rejected = true;
    row.rejectionReason = std::move(reason);
}

} // namespace

std::optional<GridPosition> gridOfGeocentric(const ReductionContext& context,
                                             const GeocentricCoordinate& point)
{
    if (context.geocentricToGrid) {
        return context.geocentricToGrid(point);
    }
    if (context.geodeticToGrid) {
        return context.geodeticToGrid(geodeticFromGeocentric(point));
    }
    return std::nullopt;
}

std::optional<GridPosition> gridOfGeodetic(const ReductionContext& context,
                                           const GeodeticCoordinate& point)
{
    if (context.geodeticToGrid) {
        return context.geodeticToGrid(point);
    }
    if (context.geocentricToGrid) {
        return context.geocentricToGrid(geocentricFromGeodetic(point));
    }
    return std::nullopt;
}

bool reducibleAntenna(const GnssAntenna& antenna)
{
    switch (antenna.method) {
    case AntennaHeightMethod::Vertical:
    case AntennaHeightMethod::PhaseCentre:
        return true;
    case AntennaHeightMethod::Unknown:
        // Nothing stated and nothing to reduce: the value is taken as it is.
        // A height with no method could be slant: not guessed.
        return antenna.height == 0.0;
    case AntennaHeightMethod::Slant:
    case AntennaHeightMethod::Other:
        return false;
    }
    return false;
}

std::string antennaWords(const GnssAntenna& antenna)
{
    std::string words = formatNumber(antenna.height, 3) + " m " + toString(antenna.method);
    if (!antenna.measuredTo.empty()) {
        words += " (" + antenna.measuredTo + ")";
    }
    return words;
}

void convertGnssVectors(Engine& engine, const std::vector<std::size_t>& rows)
{
    const ReductionSettings& settings = engine.settings;
    const ReductionContext& context = engine.context;
    const std::vector<Observation>& observations = engine.raw.observations;

    // Grid baselines: as the file gave them.
    for (std::size_t i = 0; i < observations.size(); ++i) {
        const auto* baseline = std::get_if<GnssBaselineObservation>(&observations[i]);
        if (baseline == nullptr) {
            continue;
        }
        ReportObservation& row = engine.report.observations[rows[i]];
        row.raw = std::hypot(baseline->deltaNorthing, baseline->deltaEasting);
        row.reduced = row.raw;
        GridVector vector;
        vector.from = baseline->from;
        vector.to = baseline->to;
        vector.deltaNorthing = baseline->deltaNorthing;
        vector.deltaEasting = baseline->deltaEasting;
        vector.deltaHeight = baseline->deltaUp;
        vector.sigmaNorthing = baseline->sigmaNorthing;
        vector.sigmaEasting = baseline->sigmaEasting;
        vector.sigmaHeight = baseline->sigmaUp;
        vector.row = rows[i];
        vector.source = &baseline->source;
        engine.vectors.push_back(vector);
    }

    // Geocentric vectors: the global position of every point that has one.
    std::unordered_map<std::string_view, GeocentricCoordinate> global;
    std::vector<std::size_t> pending;
    for (std::size_t i = 0; i < observations.size(); ++i) {
        const Observation& observation = observations[i];
        if (const auto* position = std::get_if<GnssGlobalPositionObservation>(&observation)) {
            const GeocentricCoordinate xyz = position->geocentric
                                                 ? *position->geocentric
                                                 : geocentricFromGeodetic(*position->geodetic);
            global.try_emplace(position->point, xyz);
        } else if (std::holds_alternative<GnssGeocentricBaselineObservation>(observation)) {
            pending.push_back(i);
            const auto& vector = std::get<GnssGeocentricBaselineObservation>(observation);
            ReportObservation& row = engine.report.observations[rows[i]];
            row.raw = std::sqrt(vector.delta.x * vector.delta.x + vector.delta.y * vector.delta.y +
                                vector.delta.z * vector.delta.z);
            row.reduced = row.raw;
        }
    }
    if (pending.empty()) {
        return;
    }
    // Either conversion will do: a drawing that converts latitude and
    // longitude converts X, Y, Z after GRS80, as the base's own position is.
    if (!context.geocentricToGrid && !context.geodeticToGrid) {
        for (const std::size_t i : pending) {
            rejectRow(engine, rows[i], "the drawing's coordinate system cannot convert it to grid");
        }
        engine.warn(std::to_string(pending.size()) +
                    " GNSS vector(s) were not used: the drawing has no coordinate system to "
                    "convert earth-centred vectors to grid. Give the drawing one and adjust "
                    "again.");
        return;
    }

    bool warnedGeoid = false;
    // A vector's base may be the far end of another: convert in passes until
    // nothing more can be.
    bool progress = true;
    while (progress && !pending.empty()) {
        progress = false;
        std::vector<std::size_t> still;
        for (const std::size_t i : pending) {
            const auto& vector = std::get<GnssGeocentricBaselineObservation>(observations[i]);
            const auto base = global.find(vector.from);
            if (base == global.end()) {
                still.push_back(i);
                continue;
            }
            progress = true;
            const GeocentricCoordinate from = base->second;
            const GeocentricCoordinate to{from.x + vector.delta.x, from.y + vector.delta.y,
                                          from.z + vector.delta.z};
            global.try_emplace(vector.to, to);
            const std::optional<GridPosition> gridFrom = gridOfGeocentric(context, from);
            const std::optional<GridPosition> gridTo = gridOfGeocentric(context, to);
            if (!gridFrom || !gridTo) {
                rejectRow(engine, rows[i], "the drawing's projection cannot convert it to grid");
                engine.warn("GNSS vector " + vector.from + " -> " + vector.to +
                                " was not used: the drawing's projection cannot convert it to "
                                "grid there.",
                            vector.source);
                continue;
            }

            GridVector grid;
            grid.from = vector.from;
            grid.to = vector.to;
            grid.deltaNorthing = gridTo->northing - gridFrom->northing;
            grid.deltaEasting = gridTo->easting - gridFrom->easting;
            grid.row = rows[i];
            grid.source = &vector.source;

            ReportObservation& row = engine.report.observations[rows[i]];
            const double horizontal = std::hypot(grid.deltaNorthing, grid.deltaEasting);
            row.corrections.push_back(AppliedCorrection{
                CorrectionKind::Other, horizontal - row.raw, std::nullopt,
                "to grid: dN " + formatNumber(grid.deltaNorthing, 4) + " dE " +
                    formatNumber(grid.deltaEasting, 4)});
            row.reduced = horizontal;

            // The height difference mark to mark, as a row of its own, the
            // way a total-station pointing's is.
            ReportObservation height;
            height.kind = "GNSS height difference";
            height.from = vector.from;
            height.to = vector.to;
            height.raw = gridTo->ellipsoidalHeight - gridFrom->ellipsoidalHeight;
            height.source = vector.source;
            double deltaHeight = height.raw;
            if (context.geoidSeparation) {
                const std::optional<double> separationFrom =
                    context.geoidSeparation(gridFrom->northing, gridFrom->easting);
                const std::optional<double> separationTo =
                    context.geoidSeparation(gridTo->northing, gridTo->easting);
                if (separationFrom && separationTo) {
                    const double change = -(*separationTo - *separationFrom);
                    height.corrections.push_back(AppliedCorrection{
                        CorrectionKind::Other, change, std::nullopt,
                        "geoid separation " + formatNumber(*separationFrom, 3) + " to " +
                            formatNumber(*separationTo, 3) + " m"});
                    deltaHeight += change;
                } else if (!warnedGeoid) {
                    warnedGeoid = true;
                    engine.warn("The drawing gives no geoid separation at some GNSS vectors; "
                                "their height differences are ellipsoidal.",
                                vector.source);
                }
            } else if (!warnedGeoid) {
                warnedGeoid = true;
                engine.warn("The drawing gives no geoid separation, so GNSS height differences "
                            "are ellipsoidal (the geoid's slope over the vector is in them).");
            }
            if (reducibleAntenna(vector.fromAntenna) && reducibleAntenna(vector.toAntenna)) {
                const double antennas = vector.fromAntenna.height - vector.toAntenna.height;
                height.corrections.push_back(AppliedCorrection{
                    CorrectionKind::InstrumentAndTargetHeight, antennas, std::nullopt,
                    "antennas: base " + antennaWords(vector.fromAntenna) + ", rover " +
                        antennaWords(vector.toAntenna)});
                deltaHeight += antennas;
                if (vector.fromAntenna.method == AntennaHeightMethod::Vertical &&
                    vector.toAntenna.method == AntennaHeightMethod::Vertical &&
                    vector.fromAntenna.type != vector.toAntenna.type) {
                    engine.warn("GNSS vector " + vector.from + " -> " + vector.to +
                                    ": the antenna heights are to the reference points of two "
                                    "different antennas, whose phase-centre offsets do not "
                                    "cancel and are not in the file.",
                                vector.source);
                }
                height.reduced = deltaHeight;
                grid.deltaHeight = deltaHeight;
            } else {
                height.rejected = true;
                height.rejectionReason = "an antenna height that is not vertical";
                engine.warn("GNSS vector " + vector.from + " -> " + vector.to +
                                ": an antenna height is not a vertical one (base " +
                                antennaWords(vector.fromAntenna) + ", rover " +
                                antennaWords(vector.toAntenna) +
                                "), so its height difference was not used; its horizontal "
                                "position was.",
                            vector.source);
            }
            engine.report.observations.push_back(std::move(height));
            grid.heightRow = engine.report.observations.size() - 1;

            // Precision: the file's X/Y/Z covariance turned to local north,
            // east and up at the base (the grid's convergence, a degree or
            // two, mixes north and east by less than the covariance is known).
            grid.sigmaNorthing = settings.apriori.gnssHorizontal;
            grid.sigmaEasting = settings.apriori.gnssHorizontal;
            grid.sigmaHeight = settings.apriori.gnssVertical;
            if (settings.useFileCovariances && vector.covariance.stated()) {
                const LocalVariances local =
                    localVariances(vector.covariance, geodeticFromGeocentric(from));
                if (local.north > 0.0 && local.east > 0.0 && local.up > 0.0) {
                    grid.sigmaNorthing = std::sqrt(local.north);
                    grid.sigmaEasting = std::sqrt(local.east);
                    grid.sigmaHeight = std::sqrt(local.up);
                }
            }
            if (vector.solution == GnssSolution::Float ||
                vector.solution == GnssSolution::Autonomous) {
                engine.warn("GNSS vector " + vector.from + " -> " + vector.to + " is a " +
                                toString(vector.solution) + " solution.",
                            vector.source);
            }
            engine.vectors.push_back(grid);
        }
        pending = std::move(still);
    }
    // The base's global position must come as a GNSS position observation
    // (or the far end of another vector). A reader that keeps a keyed-in
    // latitude and longitude only as the point's metadata has not handed one
    // over, so the sentence says what is missing from the observations, not
    // that the file has no such position anywhere.
    for (const std::size_t i : pending) {
        const auto& vector = std::get<GnssGeocentricBaselineObservation>(observations[i]);
        rejectRow(engine, rows[i], "its base has no global position given as an observation");
        engine.warn("GNSS vector " + vector.from + " -> " + vector.to +
                        " was not used: its base " + vector.from +
                        " has no global position (latitude, longitude or X, Y, Z) among the "
                        "observations read, and one is needed to turn an earth-centred vector "
                        "into grid. A position the file only keys in for the base is not yet "
                        "read as one.",
                    vector.source);
    }
}

bool radiateGnssVectors(Engine& engine, const std::unordered_set<std::string_view>* reradiate)
{
    bool placedAny = false;
    bool progress = true;
    while (progress) {
        progress = false;
        for (GridVector& vector : engine.vectors) {
            if (engine.report.observations[vector.row].rejected) {
                continue;
            }
            if (reradiate != nullptr ? reradiate->count(vector.to) == 0 : vector.radiated) {
                continue;
            }
            const Position* base = engine.find(vector.from);
            if (base == nullptr) {
                continue;
            }
            Position computed;
            computed.northing = base->northing + vector.deltaNorthing;
            computed.easting = base->easting + vector.deltaEasting;
            if (base->height && vector.deltaHeight) {
                computed.height = *base->height + *vector.deltaHeight;
            }
            computed.origin = PositionOrigin::Computed;
            computed.method = ComputationMethod::Gnss;
            if (reradiate != nullptr) {
                engine.place(vector.to, computed);
                continue;
            }
            vector.radiated = true;
            progress = true;
            Position* existing = engine.find(vector.to);
            if (existing == nullptr || existing->origin == PositionOrigin::FileOnly) {
                engine.place(vector.to, computed);
                placedAny = true;
                continue;
            }
            if (existing->origin == PositionOrigin::Computed && !existing->height &&
                computed.height) {
                existing->height = computed.height;
            }
            MisclosureReport check;
            check.name = std::string(vector.to) + " by GNSS vector from " + std::string(vector.from);
            check.northing = computed.northing - existing->northing;
            check.easting = computed.easting - existing->easting;
            check.linear = std::hypot(*check.northing, *check.easting);
            check.length = std::hypot(vector.deltaNorthing, vector.deltaEasting);
            if (*check.linear > 0.0) {
                check.precisionRatio = check.length / *check.linear;
            }
            if (computed.height && existing->height) {
                check.height = *computed.height - *existing->height;
            }
            engine.report.misclosures.push_back(std::move(check));
        }
        if (reradiate != nullptr) {
            break; // one pass: the bases have already moved
        }
    }
    return placedAny;
}

} // namespace katana::survey::detail
