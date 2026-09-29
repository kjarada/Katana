#include "katana/survey/data_model.hpp"

#include <algorithm>
#include <array>
#include <charconv>
#include <cmath>
#include <initializer_list>
#include <set>
#include <utility>

#include "katana/math/numerics.hpp"

namespace katana::survey {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;
using katana::math::kHalfPi;
using katana::math::kPi;
using katana::math::normalizeAngle;

namespace {

template <typename... Visitors> struct Overloaded : Visitors... {
    using Visitors::operator()...;
};
template <typename... Visitors> Overloaded(Visitors...) -> Overloaded<Visitors...>;

std::string describe(const Observation& observation)
{
    std::string text = observationKindName(observation);
    const char* separator = " ";
    for (const std::string& id : referencedPoints(observation)) {
        text += separator;
        text += id.empty() ? "<empty>" : id;
        separator = " -> ";
    }
    return text;
}

Status invalid(const Observation& observation, std::string message)
{
    return makeError(ErrorCode::InvalidSurveyObservation, std::move(message),
                     describe(observation));
}

// Empty string when every check passes, otherwise the reason.
std::string checkFinite(std::initializer_list<std::pair<const char*, double>> values)
{
    for (const auto& [name, value] : values) {
        if (!std::isfinite(value)) {
            return std::string(name) + " is not finite";
        }
    }
    return {};
}

std::string checkSigmas(std::initializer_list<std::pair<const char*, double>> sigmas)
{
    for (const auto& [name, sigma] : sigmas) {
        if (!std::isfinite(sigma) || !(sigma > 0.0)) {
            return std::string(name) + " must be a positive finite standard deviation";
        }
    }
    return {};
}

std::string checkStations(const std::vector<std::string>& ids)
{
    for (std::size_t i = 0; i < ids.size(); ++i) {
        if (ids[i].empty()) {
            return "station id is empty";
        }
        for (std::size_t j = i + 1; j < ids.size(); ++j) {
            if (ids[i] == ids[j]) {
                return "stations of an observation must be distinct";
            }
        }
    }
    return {};
}

// A covariance the file did not state is allowed (the reduction weights the
// value from ObservationPrecision); a stated one needs a positive diagonal.
std::string checkCovariance(const GnssCovariance3& covariance)
{
    if (!covariance.stated()) {
        return {};
    }
    if (std::string reason = checkFinite({{"covariance XY", covariance.xy},
                                          {"covariance XZ", covariance.xz},
                                          {"covariance YZ", covariance.yz}});
        !reason.empty()) {
        return reason;
    }
    for (const auto& [name, variance] : {std::pair{"variance X", covariance.xx},
                                         std::pair{"variance Y", covariance.yy},
                                         std::pair{"variance Z", covariance.zz}}) {
        if (!std::isfinite(variance) || !(variance > 0.0)) {
            return std::string(name) + " must be a positive finite variance";
        }
    }
    return {};
}

// First non-empty reason wins.
std::string firstOf(std::initializer_list<std::string> reasons)
{
    for (const std::string& reason : reasons) {
        if (!reason.empty()) {
            return reason;
        }
    }
    return {};
}

std::string valueProblem(const Observation& observation)
{
    return std::visit(
        Overloaded{
            [](const DistanceObservation& o) {
                return firstOf({checkFinite({{"distance", o.distance},
                                             {"instrument height", o.instrumentHeight},
                                             {"target height", o.targetHeight}}),
                                checkSigmas({{"sigma", o.sigma}}),
                                o.distance > 0.0 ? std::string{}
                                                 : std::string("distance must be positive")});
            },
            [](const HorizontalAngleObservation& o) {
                return firstOf({checkFinite({{"angle", o.angle}}), checkSigmas({{"sigma", o.sigma}})});
            },
            [](const VerticalAngleObservation& o) {
                return firstOf(
                    {checkFinite({{"angle", o.angle},
                                  {"instrument height", o.instrumentHeight},
                                  {"target height", o.targetHeight}}),
                     checkSigmas({{"sigma", o.sigma}}),
                     std::abs(o.angle) <= kHalfPi
                         ? std::string{}
                         : std::string("vertical angle must lie in [-pi/2, pi/2]")});
            },
            [](const ZenithAngleObservation& o) {
                return firstOf({checkFinite({{"angle", o.angle},
                                             {"instrument height", o.instrumentHeight},
                                             {"target height", o.targetHeight}}),
                                checkSigmas({{"sigma", o.sigma}}),
                                (o.angle >= 0.0 && o.angle <= kPi)
                                    ? std::string{}
                                    : std::string("zenith angle must lie in [0, pi]")});
            },
            [](const AzimuthObservation& o) {
                return firstOf(
                    {checkFinite({{"azimuth", o.azimuth}}), checkSigmas({{"sigma", o.sigma}})});
            },
            [](const GnssBaselineObservation& o) {
                return firstOf({checkFinite({{"delta northing", o.deltaNorthing},
                                             {"delta easting", o.deltaEasting},
                                             {"delta up", o.deltaUp}}),
                                checkSigmas({{"sigma northing", o.sigmaNorthing},
                                             {"sigma easting", o.sigmaEasting},
                                             {"sigma up", o.sigmaUp}})});
            },
            [](const GnssPositionObservation& o) {
                return firstOf({checkFinite({{"northing", o.northing},
                                             {"easting", o.easting},
                                             {"elevation", o.elevation}}),
                                checkSigmas({{"sigma northing", o.sigmaNorthing},
                                             {"sigma easting", o.sigmaEasting},
                                             {"sigma elevation", o.sigmaElevation}})});
            },
            [](const LevelDifferenceObservation& o) {
                return firstOf({checkFinite({{"height difference", o.heightDifference},
                                             {"length", o.length}}),
                                checkSigmas({{"sigma", o.sigma}}),
                                o.length >= 0.0 ? std::string{}
                                                : std::string("length must not be negative")});
            },
            [](const HorizontalDirectionObservation& o) {
                return firstOf(
                    {checkFinite({{"direction", o.direction}}), checkSigmas({{"sigma", o.sigma}})});
            },
            [](const GnssGlobalPositionObservation& o) {
                if (o.geocentric.has_value() == o.geodetic.has_value()) {
                    return std::string(
                        "a GNSS position must be either geocentric or geodetic, not both or neither");
                }
                std::string reason;
                if (o.geocentric) {
                    reason = checkFinite({{"X", o.geocentric->x},
                                          {"Y", o.geocentric->y},
                                          {"Z", o.geocentric->z}});
                } else {
                    reason = firstOf(
                        {checkFinite({{"latitude", o.geodetic->latitude},
                                      {"longitude", o.geodetic->longitude},
                                      {"ellipsoidal height", o.geodetic->ellipsoidalHeight}}),
                         std::abs(o.geodetic->latitude) <= kHalfPi
                             ? std::string{}
                             : std::string("latitude must lie in [-pi/2, pi/2]")});
                }
                return firstOf({reason, checkCovariance(o.covariance)});
            },
            [](const GnssGeocentricBaselineObservation& o) {
                return firstOf({checkFinite({{"delta X", o.delta.x},
                                             {"delta Y", o.delta.y},
                                             {"delta Z", o.delta.z}}),
                                checkCovariance(o.covariance)});
            },
        },
        observation);
}

// A point id set, ordered so that the first error reported for a bad project is
// the same one on every run (PLAN.MD section 35 and the ordering convention at
// the top of data_model.hpp).
using PointIdSet = std::set<std::string, std::less<>>;

Status missingPoint(std::string_view pointId, std::string context)
{
    return makeError(ErrorCode::NotFound,
                     "point '" + std::string(pointId) + "' is not in the project",
                     std::move(context));
}

// The ids of a project's points as views into the project, sorted once and
// searched by bisection.
//
// Every import is validated (surveyio::readSurvey), so this is on the import
// path of every file. The earlier std::set<std::string> copied every id and
// referencedPoints() copied every observation's ids again; on a synthetic job
// of 2,000 setups and 600,000 raw observations that was 2.5 s of a Debug build
// before a single value had been looked at.
class PointIds {
  public:
    void reserve(std::size_t count) { ids_.reserve(count); }
    void add(std::string_view id) { ids_.push_back(id); }
    // Sorts; true when no id occurs twice.
    [[nodiscard]] bool seal()
    {
        std::sort(ids_.begin(), ids_.end());
        return std::adjacent_find(ids_.begin(), ids_.end()) == ids_.end();
    }
    [[nodiscard]] bool contains(std::string_view id) const
    {
        return std::binary_search(ids_.begin(), ids_.end(), id);
    }

  private:
    std::vector<std::string_view> ids_;
};

// The points an observation names, as views: never more than three.
struct PointRefs {
    std::array<std::string_view, 3> ids{};
    std::size_t count = 0;
};

PointRefs referencedPointViews(const Observation& observation)
{
    return std::visit(
        Overloaded{
            [](const HorizontalAngleObservation& o) { return PointRefs{{o.at, o.from, o.to}, 3}; },
            [](const HorizontalDirectionObservation& o) { return PointRefs{{o.at, o.to, {}}, 2}; },
            [](const GnssPositionObservation& o) { return PointRefs{{o.point, {}, {}}, 1}; },
            [](const GnssGlobalPositionObservation& o) {
                return PointRefs{{o.point, {}, {}}, 1};
            },
            [](const auto& o) { return PointRefs{{o.from, o.to, {}}, 2}; },
        },
        observation);
}

// validateObservation() plus "every point it names is in the project", without
// a heap allocation on the path every valid observation takes. A failure is
// worded by validateObservation() itself, so the two cannot disagree.
Status checkProjectObservation(const Observation& observation, const PointIds& points,
                               const std::string& context)
{
    const PointRefs refs = referencedPointViews(observation);
    bool stationsUsable = true;
    for (std::size_t i = 0; i < refs.count && stationsUsable; ++i) {
        stationsUsable = !refs.ids[i].empty();
        for (std::size_t j = i + 1; j < refs.count && stationsUsable; ++j) {
            stationsUsable = refs.ids[i] != refs.ids[j];
        }
    }
    if (!stationsUsable || !valueProblem(observation).empty()) {
        return validateObservation(observation);
    }
    for (std::size_t i = 0; i < refs.count; ++i) {
        if (!points.contains(refs.ids[i])) {
            return missingPoint(refs.ids[i], context + ": " + describe(observation));
        }
    }
    return {};
}

// The per-point checks. Duplicates are looked for only when the sorted ids
// have already shown there is one, and then in file order, so the error is
// the one the file's order makes first - the same one on every run.
Status checkPoints(const SurveyProject& project, bool lookForDuplicates)
{
    PointIdSet seen;
    for (const SurveyPoint& point : project.points) {
        if (point.id.empty()) {
            return makeError(ErrorCode::InvalidArgument, "a point has an empty id",
                             describeSource(point.source));
        }
        if (lookForDuplicates && !seen.insert(point.id).second) {
            return makeError(ErrorCode::AlreadyExists, "duplicate point id '" + point.id + "'",
                             describeSource(point.source));
        }
        std::string reason = checkFinite({{"northing", point.northing}, {"easting", point.easting}});
        if (reason.empty() && point.elevation) {
            reason = checkFinite({{"elevation", *point.elevation}});
        }
        if (!reason.empty()) {
            return makeError(ErrorCode::InvalidArgument, "point '" + point.id + "': " + reason,
                             describeSource(point.source));
        }
    }
    for (const UnpositionedPoint& point : project.unpositionedPoints) {
        if (point.id.empty()) {
            return makeError(ErrorCode::InvalidArgument, "an unpositioned point has an empty id",
                             describeSource(point.source));
        }
        if (lookForDuplicates && !seen.insert(point.id).second) {
            return makeError(ErrorCode::AlreadyExists,
                             "duplicate point id '" + point.id +
                                 "' (a point may not be both positioned and unpositioned)",
                             describeSource(point.source));
        }
    }
    return {};
}

} // namespace

// ---- Provenance ----------------------------------------------------------------

std::string sourceFileName(std::string_view supplied)
{
    const std::size_t cut = supplied.find_last_of("/\\:");
    const std::string_view name =
        cut == std::string_view::npos ? supplied : supplied.substr(cut + 1);
    if (name == "." || name == "..") {
        return {};
    }
    return std::string(name);
}

std::string describeSource(const SourceRecord& source)
{
    if (!source.known()) {
        return "unknown source";
    }
    std::string text = source.fileName.empty() ? std::string("<unnamed file>") : source.fileName;
    if (source.recordNumber != 0) {
        text += " record " + std::to_string(source.recordNumber);
    }
    std::string format = source.manufacturer;
    for (const std::string* part : {&source.format, &source.formatVersion}) {
        if (part->empty()) {
            continue;
        }
        format += format.empty() ? *part : " " + *part;
    }
    if (!format.empty()) {
        text += " (" + format + ")";
    }
    return text;
}

const char* toString(CorrectionState state)
{
    switch (state) {
    case CorrectionState::Unknown:
        return "not stated";
    case CorrectionState::Applied:
        return "applied by the instrument";
    case CorrectionState::NotApplied:
        return "not applied";
    }
    return "not stated";
}

const char* toString(Face face)
{
    switch (face) {
    case Face::Unknown:
        return "unknown face";
    case Face::Left:
        return "face left";
    case Face::Right:
        return "face right";
    }
    return "unknown face";
}

const char* toString(AntennaHeightMethod method)
{
    switch (method) {
    case AntennaHeightMethod::Unknown:
        return "not stated";
    case AntennaHeightMethod::Vertical:
        return "vertical to the antenna reference point";
    case AntennaHeightMethod::Slant:
        return "slant";
    case AntennaHeightMethod::PhaseCentre:
        return "to the phase centre";
    case AntennaHeightMethod::Other:
        return "other";
    }
    return "not stated";
}

const char* toString(GnssSolution solution)
{
    switch (solution) {
    case GnssSolution::Unknown:
        return "not stated";
    case GnssSolution::Fixed:
        return "fixed";
    case GnssSolution::Float:
        return "float";
    case GnssSolution::Differential:
        return "differential";
    case GnssSolution::Autonomous:
        return "autonomous";
    }
    return "not stated";
}

std::string toString(const SurveyTimestamp& timestamp)
{
    if (!timestamp.known()) {
        return {};
    }
    // Shortest text that reads back as the same double, so a stored second is
    // not rounded to whatever a fixed precision thought was enough.
    std::array<char, 64> second{};
    const auto written =
        std::to_chars(second.data(), second.data() + second.size(), timestamp.second);
    const std::string_view secondText(
        second.data(), written.ec == std::errc{} ? static_cast<std::size_t>(written.ptr - second.data()) : 0);
    auto twoDigits = [](int value) {
        std::string text = std::to_string(value);
        return text.size() < 2 ? "0" + text : text;
    };
    std::string text = std::to_string(timestamp.year) + "-" + twoDigits(timestamp.month) + "-" +
                       twoDigits(timestamp.day) + "T" + twoDigits(timestamp.hour) + ":" +
                       twoDigits(timestamp.minute) + ":";
    // Two integer digits, as ISO 8601 has them: "05.5", not "5.5".
    if (timestamp.second < 10.0) {
        text += '0';
    }
    text += secondText;
    if (!timestamp.timeSystem.empty()) {
        text += ' ';
        text += timestamp.timeSystem;
    }
    return text;
}

double distanceSigma(const ObservationPrecision& precision, double distance)
{
    return std::hypot(precision.distanceConstant, precision.distancePpm * 1e-6 * distance);
}

const char* toString(CoordinateSource source)
{
    switch (source) {
    case CoordinateSource::Unknown:
        return "unknown";
    case CoordinateSource::FieldObserved:
        return "field observed";
    case CoordinateSource::Calculated:
        return "calculated";
    case CoordinateSource::Entered:
        return "entered";
    }
    return "unknown";
}

const char* toString(LinearUnit unit)
{
    switch (unit) {
    case LinearUnit::Unknown:
        return "unknown";
    case LinearUnit::Metres:
        return "metres";
    case LinearUnit::Feet:
        return "feet";
    case LinearUnit::UsSurveyFeet:
        return "US survey feet";
    case LinearUnit::Links:
        return "links";
    }
    return "unknown";
}

katana::core::Result<katana::math::UnitRatio> metresPer(LinearUnit unit)
{
    namespace units = katana::math::units;
    switch (unit) {
    case LinearUnit::Unknown:
        break;
    case LinearUnit::Metres:
        return units::kMetre;
    case LinearUnit::Feet:
        return units::kInternationalFoot;
    case LinearUnit::UsSurveyFeet:
        return units::kUsSurveyFoot;
    case LinearUnit::Links:
        return units::kInternationalLink;
    }
    return makeError(ErrorCode::InvalidArgument,
                     "the linear unit is unknown, so its numbers cannot be converted to metres",
                     "a unit must be declared; none is assumed");
}

const char* toString(AngularUnit unit)
{
    switch (unit) {
    case AngularUnit::Unknown:
        return "unknown";
    case AngularUnit::Radians:
        return "radians";
    case AngularUnit::DecimalDegrees:
        return "decimal degrees";
    case AngularUnit::DegreesMinutesSeconds:
        return "degrees, minutes and seconds";
    case AngularUnit::Gons:
        return "gons";
    case AngularUnit::Mils:
        return "mils";
    }
    return "unknown";
}

std::string observationKindName(const Observation& observation)
{
    return std::visit(
        Overloaded{
            [](const DistanceObservation& o) {
                return std::string(o.kind == DistanceKind::Horizontal ? "horizontal distance"
                                                                      : "slope distance");
            },
            [](const HorizontalAngleObservation&) { return std::string("horizontal angle"); },
            [](const VerticalAngleObservation&) { return std::string("vertical angle"); },
            [](const ZenithAngleObservation&) { return std::string("zenith angle"); },
            [](const AzimuthObservation&) { return std::string("azimuth"); },
            [](const GnssBaselineObservation&) { return std::string("GNSS baseline"); },
            [](const GnssPositionObservation&) { return std::string("GNSS position"); },
            [](const LevelDifferenceObservation&) { return std::string("level difference"); },
            [](const HorizontalDirectionObservation&) {
                return std::string("horizontal direction");
            },
            [](const GnssGlobalPositionObservation& o) {
                return std::string(o.geocentric ? "GNSS geocentric position"
                                                : "GNSS geodetic position");
            },
            [](const GnssGeocentricBaselineObservation&) {
                return std::string("GNSS geocentric baseline");
            },
        },
        observation);
}

std::vector<std::string> referencedPoints(const Observation& observation)
{
    using Ids = std::vector<std::string>;
    return std::visit(
        Overloaded{
            [](const HorizontalAngleObservation& o) { return Ids{o.at, o.from, o.to}; },
            [](const GnssPositionObservation& o) { return Ids{o.point}; },
            [](const GnssGlobalPositionObservation& o) { return Ids{o.point}; },
            [](const HorizontalDirectionObservation& o) { return Ids{o.at, o.to}; },
            [](const auto& o) { return Ids{o.from, o.to}; },
        },
        observation);
}

Status validateObservation(const Observation& observation)
{
    if (std::string reason = checkStations(referencedPoints(observation)); !reason.empty()) {
        return invalid(observation, std::move(reason));
    }
    if (std::string reason = valueProblem(observation); !reason.empty()) {
        return invalid(observation, std::move(reason));
    }
    return {};
}

Observation normalizedObservation(Observation observation)
{
    if (auto* angle = std::get_if<HorizontalAngleObservation>(&observation)) {
        angle->angle = normalizeAngle(angle->angle);
    } else if (auto* azimuth = std::get_if<AzimuthObservation>(&observation)) {
        azimuth->azimuth = normalizeAngle(azimuth->azimuth);
    } else if (auto* direction = std::get_if<HorizontalDirectionObservation>(&observation)) {
        direction->direction = normalizeAngle(direction->direction);
    }
    return observation;
}

const Pointing* observationPointing(const Observation& observation)
{
    return std::visit(
        Overloaded{
            [](const DistanceObservation& o) -> const Pointing* { return &o.pointing; },
            [](const HorizontalAngleObservation& o) -> const Pointing* { return &o.pointing; },
            [](const VerticalAngleObservation& o) -> const Pointing* { return &o.pointing; },
            [](const ZenithAngleObservation& o) -> const Pointing* { return &o.pointing; },
            [](const HorizontalDirectionObservation& o) -> const Pointing* {
                return &o.pointing;
            },
            [](const auto&) -> const Pointing* { return nullptr; },
        },
        observation);
}

const SourceRecord& observationSource(const Observation& observation)
{
    return std::visit([](const auto& o) -> const SourceRecord& { return o.source; }, observation);
}

void setObservationSource(Observation& observation, SourceRecord source)
{
    std::visit([&source](auto& o) { o.source = std::move(source); }, observation);
}

// ---- ControlPoint ------------------------------------------------------------

ControlPoint ControlPoint::fixedHorizontal(std::string pointId)
{
    ControlPoint control;
    control.pointId = std::move(pointId);
    control.northing.constraint = ControlConstraint::Fixed;
    control.easting.constraint = ControlConstraint::Fixed;
    return control;
}

ControlPoint ControlPoint::fixedVertical(std::string pointId)
{
    ControlPoint control;
    control.pointId = std::move(pointId);
    control.elevation.constraint = ControlConstraint::Fixed;
    return control;
}

ControlPoint ControlPoint::fixed3d(std::string pointId)
{
    ControlPoint control = fixedHorizontal(std::move(pointId));
    control.elevation.constraint = ControlConstraint::Fixed;
    return control;
}

ControlPoint ControlPoint::weightedHorizontal(std::string pointId, double sigmaNorthing,
                                              double sigmaEasting)
{
    ControlPoint control;
    control.pointId = std::move(pointId);
    control.northing = ControlComponent{ControlConstraint::Weighted, sigmaNorthing};
    control.easting = ControlComponent{ControlConstraint::Weighted, sigmaEasting};
    return control;
}

ControlPoint ControlPoint::weightedVertical(std::string pointId, double sigma)
{
    ControlPoint control;
    control.pointId = std::move(pointId);
    control.elevation = ControlComponent{ControlConstraint::Weighted, sigma};
    return control;
}

// ---- Declared coordinate system --------------------------------------------------

DeclaredCoordinateSystem DeclaredCoordinateSystem::named(std::string name)
{
    DeclaredCoordinateSystem system;
    system.unknown = name.empty(); // a blank declaration declares nothing
    system.name = std::move(name);
    return system;
}

DeclaredCoordinateSystem DeclaredCoordinateSystem::epsg(int code, std::string name)
{
    DeclaredCoordinateSystem system = named(std::move(name));
    // EPSG dataset codes are positive integers; 0 is the "none given" marker of
    // the field itself, so a file stating 0 has stated nothing usable.
    if (code > 0) {
        system.epsgCode = code;
        system.unknown = false;
    }
    return system;
}

// ---- SurveyProject ---------------------------------------------------------------

Status validateProject(const SurveyProject& project)
{
    PointIds points;
    points.reserve(project.points.size() + project.unpositionedPoints.size());
    for (const SurveyPoint& point : project.points) {
        points.add(point.id);
    }
    for (const UnpositionedPoint& point : project.unpositionedPoints) {
        points.add(point.id);
    }
    const bool unique = points.seal();
    if (Status status = checkPoints(project, !unique); !status.ok()) {
        return status;
    }

    PointIdSet stationIds;
    for (const SurveyStation& station : project.stations) {
        const Station& setup = station.setup;
        if (setup.id.empty()) {
            return makeError(ErrorCode::InvalidArgument, "a station has an empty id",
                             describeSource(station.source));
        }
        if (!stationIds.insert(setup.id).second) {
            return makeError(ErrorCode::AlreadyExists, "duplicate station id '" + setup.id + "'",
                             describeSource(station.source));
        }
        if (!points.contains(setup.pointId)) {
            return missingPoint(setup.pointId, "station " + setup.id);
        }
        if (std::string reason = checkFinite({{"instrument height", setup.instrumentHeight}});
            !reason.empty()) {
            return makeError(ErrorCode::InvalidArgument, "station " + setup.id + ": " + reason,
                             describeSource(station.source));
        }
        if (!station.backsightPointId.empty() && !points.contains(station.backsightPointId)) {
            return missingPoint(station.backsightPointId, "backsight of station " + setup.id);
        }
        if (station.backsightAzimuth && !std::isfinite(*station.backsightAzimuth)) {
            return makeError(ErrorCode::InvalidArgument,
                             "station " + setup.id + ": backsight azimuth is not finite",
                             describeSource(station.source));
        }
        if (station.statedBacksightAzimuth && !std::isfinite(*station.statedBacksightAzimuth)) {
            return makeError(ErrorCode::InvalidArgument,
                             "station " + setup.id +
                                 ": the azimuth the file states for the backsight is not finite",
                             describeSource(station.source));
        }
        const std::string context = "station " + setup.id;
        for (const Observation& observation : station.observations) {
            if (Status status = checkProjectObservation(observation, points, context);
                !status.ok()) {
                return status;
            }
        }
    }

    const std::string looseContext = "project observation";
    for (const Observation& observation : project.observations) {
        if (Status status = checkProjectObservation(observation, points, looseContext);
            !status.ok()) {
            return status;
        }
    }

    PointIds positioned;
    if (!project.controlPoints.empty()) {
        positioned.reserve(project.points.size());
        for (const SurveyPoint& point : project.points) {
            positioned.add(point.id);
        }
        (void)positioned.seal(); // duplicates were refused above
    }
    for (const ControlPoint& control : project.controlPoints) {
        if (!positioned.contains(control.pointId)) {
            return makeError(ErrorCode::NotFound,
                             "control point '" + control.pointId +
                                 "' is not a positioned point of the project",
                             "control needs a published value to hold");
        }
        for (const auto& [name, component] :
             {std::pair{"northing", &control.northing}, std::pair{"easting", &control.easting},
              std::pair{"elevation", &control.elevation}}) {
            if (component->constraint == ControlConstraint::Weighted &&
                (!std::isfinite(component->sigma) || !(component->sigma > 0.0))) {
                return makeError(ErrorCode::InvalidArgument,
                                 "control point '" + control.pointId + "': the weighted " +
                                     name + " needs a positive standard deviation");
            }
        }
    }

    for (const SurveyFeature& feature : project.features) {
        const std::string label =
            "'" + (feature.name.empty() ? feature.code : feature.name) + "'";
        if (feature.pointIds.empty()) {
            return makeError(ErrorCode::InvalidArgument, "feature " + label + " names no points",
                             describeSource(feature.source));
        }
        for (const std::string& pointId : feature.pointIds) {
            if (!points.contains(pointId)) {
                return missingPoint(pointId, "feature " + label);
            }
        }
    }

    return {};
}

} // namespace katana::survey
