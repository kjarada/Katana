#include "katana/cad/survey_tools.hpp"

#include <algorithm>
#include <cmath>
#include <format>
#include <map>
#include <memory>
#include <span>
#include <utility>

#include "katana/cad/survey_import.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/geodesy/coordinate_reference_system.hpp"
#include "katana/geodesy/coordinate_transformer.hpp"
#include "katana/math/numerics.hpp"
#include "katana/survey/angles.hpp"
#include "katana/survey/cogo.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::survey::Coordinate2;

namespace {

// ---- the one crossing between the drawing and the survey library ----------------------------
//
// The drawing is (x, y) = (easting, northing); survey::Coordinate2 is
// (northing, easting). Every conversion goes through these two, so there is
// exactly one place where the order could be wrong - and the tests use
// coordinates whose eastings and northings cannot be confused.
Coordinate2 toSurvey(const Point2& point) { return Coordinate2{point.y, point.x}; }
Point2 fromSurvey(const Coordinate2& coordinate)
{
    return Point2(coordinate.easting, coordinate.northing);
}

// The property a point's name or number is read from and written to: the one
// the survey import writes, taken from its options rather than spelt again, so
// that a point this file creates and one a field file created are named alike.
const std::string& pointNameProperty()
{
    static const std::string name = SurveyImportOptions{}.pointNumberProperty;
    return name;
}

// A value that rounds to zero is printed without a sign. PROJ and the
// adjustments return -1e-10 for an exact zero often enough that "-0.000"
// would appear in reports, where it reads as a small negative misclosure.
std::string unsignedZero(std::string text)
{
    if (text.find_first_of("123456789") == std::string::npos &&
        (text.front() == '-' || text.front() == '+')) {
        text.erase(0, 1);
    }
    return text;
}
std::string fixed(double value, int decimals)
{
    return unsignedZero(std::format("{:.{}f}", value, decimals));
}
std::string signedFixed(double value, int decimals)
{
    return unsignedZero(std::format("{:+.{}f}", value, decimals));
}

// DMS and bearing text. The survey formatters fail only for a non-finite angle,
// which nothing here produces; if one ever did, the report says so in place
// rather than printing a plausible angle.
std::string dms(double radians)
{
    const auto text = survey::formatDms(radians, kSurveySecondsDecimals);
    return text ? *text : "(" + text.error().message + ")";
}
std::string bearing(double azimuth)
{
    const auto text = survey::formatBearing(azimuth, kSurveySecondsDecimals);
    return text ? *text : "(" + text.error().message + ")";
}

std::string coordinateText(const Point2& point)
{
    return "E " + fixed(point.x, 3) + " N " + fixed(point.y, 3);
}

// Blank- or comma-separated fields of one line of a field book, with a '#'
// comment cut off. core::isAsciiSpace rather than std::isspace: the C locale
// must not decide what separates two readings (core/text.hpp says why).
std::vector<std::string_view> fields(std::string_view line)
{
    if (const auto hash = line.find('#'); hash != std::string_view::npos) {
        line = line.substr(0, hash);
    }
    std::vector<std::string_view> out;
    std::size_t start = 0;
    while (start < line.size()) {
        while (start < line.size() && (katana::core::isAsciiSpace(line[start]) || line[start] == ',')) {
            ++start;
        }
        std::size_t end = start;
        while (end < line.size() && !katana::core::isAsciiSpace(line[end]) && line[end] != ',') {
            ++end;
        }
        if (end > start) {
            out.push_back(line.substr(start, end - start));
        }
        start = end;
    }
    return out;
}

std::string lineContext(std::size_t lineNumber, std::string_view line)
{
    return "line " + std::to_string(lineNumber) + ": " + std::string(katana::core::trimmed(line));
}

const Entity* findEntity(const Document& document, EntityId id)
{
    return document.model().entities.find(id);
}

std::string nameOf(const Entity& entity)
{
    const auto found = entity.properties.find(pointNameProperty());
    return found == entity.properties.end() ? std::string{}
                                            : katana::entity::toString(found->second);
}

// A direction given as a mark or an azimuth; see TraverseOrientation.
Result<double> orientationAzimuth(const Point2& station, const TraverseOrientation& orientation,
                                  std::string_view what)
{
    if (!orientation.referenceMark) {
        if (!std::isfinite(orientation.azimuth)) {
            return makeError(ErrorCode::InvalidArgument,
                             std::string(what) + " azimuth is not finite");
        }
        return survey::normalizeAzimuth(orientation.azimuth);
    }
    const auto inverse = survey::inverse(toSurvey(station), toSurvey(*orientation.referenceMark));
    if (!inverse) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string(what) + " reference mark gives no direction: " +
                             inverse.error().message,
                         coordinateText(*orientation.referenceMark));
    }
    return inverse->azimuth;
}

} // namespace

// ---- text in -------------------------------------------------------------------------------

std::string_view surveyAngleFormats()
{
    return "an angle is degrees, minutes and seconds with a mark after each - 36\xC2\xB0"
           "52'11.63\", 36d52m11.63s, 36:52:11.63 or 36-52-11.63 - or decimal degrees "
           "(36.8699); a direction may also be a quadrant bearing, N 36d52m11.63s E";
}

Result<double> parseSurveyAngle(std::string_view text)
{
    auto angle = survey::parseDms(text);
    if (!angle) {
        return makeError(ErrorCode::ParseFailure,
                         angle.error().message + "; " + std::string(surveyAngleFormats()),
                         std::string(text));
    }
    return *angle;
}

Result<double> parseSurveyDirection(std::string_view text)
{
    const std::string_view body = katana::core::trimmed(text);
    const auto isLetter = [](char c, char upper) { return c == upper || c == upper + ('a' - 'A'); };
    // A quadrant bearing starts with its meridian: nothing else an angle can
    // start with is a letter, so the first character decides.
    if (!body.empty() && (isLetter(body.front(), 'N') || isLetter(body.front(), 'S'))) {
        auto azimuth = survey::parseBearing(body);
        if (!azimuth) {
            return makeError(ErrorCode::ParseFailure,
                             azimuth.error().message + "; " + std::string(surveyAngleFormats()),
                             std::string(text));
        }
        return *azimuth;
    }
    auto angle = parseSurveyAngle(body);
    if (!angle) {
        return angle.error();
    }
    return survey::normalizeAzimuth(*angle);
}

Result<double> parseSurveyNumber(std::string_view text, std::string_view what)
{
    if (const auto value = katana::core::parseFiniteDouble(katana::core::trimmed(text))) {
        return *value;
    }
    return makeError(ErrorCode::ParseFailure,
                     std::string(what) + " must be a number ('.' is the decimal point)",
                     std::string(text));
}

// ---- positions -----------------------------------------------------------------------------

std::string SurveyPosition::label() const
{
    if (!entity) {
        std::string text = coordinateText(point);
        if (elevation) {
            text += " Z " + fixed(*elevation, 3);
        }
        return text;
    }
    std::string text = source.empty() ? "entity " + std::to_string(*entity) : source;
    if (!name.empty()) {
        text += " (" + name + ")";
    }
    return text;
}

Result<SurveyPosition> positionOfPoint(const Document& document, EntityId id)
{
    const Entity* entity = findEntity(document, id);
    if (entity == nullptr) {
        return makeError(ErrorCode::NotFound, "no entity with that id", std::to_string(id));
    }
    const auto* point = std::get_if<katana::entity::PointGeometry>(&entity->geometry);
    if (point == nullptr) {
        return makeError(ErrorCode::InvalidArgument,
                         "a survey position must be a point entity; this is a " +
                             std::string(katana::entity::toString(entity->type())),
                         std::to_string(id));
    }
    SurveyPosition position;
    position.point = point->position;
    position.elevation = katana::entity::heightsOf(entity->properties, 1).front();
    position.entity = id;
    position.source = "point " + std::to_string(id);
    position.name = nameOf(*entity);
    return position;
}

Result<std::pair<SurveyPosition, SurveyPosition>> endsOfLine(const Document& document,
                                                             EntityId id)
{
    const Entity* entity = findEntity(document, id);
    if (entity == nullptr) {
        return makeError(ErrorCode::NotFound, "no entity with that id", std::to_string(id));
    }
    std::vector<Point2> vertices;
    std::string kind;
    if (const auto* segment = std::get_if<katana::geometry::Segment2>(&entity->geometry)) {
        vertices = {segment->start, segment->end};
        kind = "line ";
    } else if (const auto* polyline = std::get_if<katana::geometry::Polyline2>(&entity->geometry);
               polyline != nullptr && !polyline->closed && polyline->vertices.size() >= 2) {
        vertices = polyline->vertices;
        kind = "polyline ";
    } else {
        return makeError(ErrorCode::InvalidArgument,
                         "the two ends of a line or of an open polyline are needed; this is a " +
                             std::string(katana::entity::toString(entity->type())),
                         std::to_string(id));
    }
    const auto heights = katana::entity::heightsOf(entity->properties, vertices.size());
    SurveyPosition start;
    start.point = vertices.front();
    start.elevation = heights.front();
    start.entity = id;
    start.source = kind + std::to_string(id) + " start";
    SurveyPosition end;
    end.point = vertices.back();
    end.elevation = heights.back();
    end.entity = id;
    end.source = kind + std::to_string(id) + " end";
    return std::pair{std::move(start), std::move(end)};
}

Result<SurveyPosition> parseSurveyPosition(const Document& document, std::string_view text)
{
    const std::string_view body = katana::core::trimmed(text);
    if (body.find(',') == std::string_view::npos) {
        // No comma: an entity id, as LIST shows them.
        const auto id = katana::core::parseInteger(body);
        if (!id || *id <= 0) {
            return makeError(ErrorCode::ParseFailure,
                             "a position is E,N or E,N,Z (easting first) or a point's entity id",
                             std::string(text));
        }
        return positionOfPoint(document, static_cast<EntityId>(*id));
    }
    std::vector<double> numbers;
    std::size_t start = 0;
    while (true) {
        const std::size_t comma = body.find(',', start);
        const std::string_view part = body.substr(start, comma == std::string_view::npos
                                                             ? std::string_view::npos
                                                             : comma - start);
        const auto value = katana::core::parseFiniteDouble(katana::core::trimmed(part));
        if (!value) {
            return makeError(ErrorCode::ParseFailure,
                             "a position is E,N or E,N,Z with numbers ('.' is the decimal point)",
                             std::string(text));
        }
        numbers.push_back(*value);
        if (comma == std::string_view::npos) {
            break;
        }
        start = comma + 1;
    }
    if (numbers.size() < 2 || numbers.size() > 3) {
        return makeError(ErrorCode::ParseFailure, "a position is E,N or E,N,Z (easting first)",
                         std::string(text));
    }
    SurveyPosition position;
    position.point = Point2(numbers[0], numbers[1]);
    if (numbers.size() == 3) {
        position.elevation = numbers[2];
    }
    return position;
}

std::vector<SurveyPosition> selectedPointPositions(const Document& document)
{
    std::vector<SurveyPosition> positions;
    for (const EntityId id : document.selection().ids()) {
        if (auto position = positionOfPoint(document, id)) {
            positions.push_back(std::move(*position));
        }
    }
    return positions;
}

// ---- inverse -------------------------------------------------------------------------------

Result<InverseResult> computeInverse(const SurveyPosition& from, const SurveyPosition& to)
{
    const auto inverse = survey::inverse(toSurvey(from.point), toSurvey(to.point));
    if (!inverse) {
        return makeError(inverse.error().code, inverse.error().message,
                         from.label() + " to " + to.label());
    }
    InverseResult result;
    result.from = from;
    result.to = to;
    result.deltaEasting = to.point.x - from.point.x;
    result.deltaNorthing = to.point.y - from.point.y;
    result.horizontalDistance = inverse->distance;
    result.azimuth = inverse->azimuth;
    if (from.elevation && to.elevation) {
        const double dz = *to.elevation - *from.elevation;
        result.heightDifference = dz;
        result.slopeDistance = std::hypot(result.horizontalDistance, dz);
        // survey::inverse refuses coincident marks, so the distance is at least
        // the 0.1 mm coordinate tolerance and the ratio is finite.
        result.grade = dz / result.horizontalDistance;
    }
    return result;
}

std::string formatInverseReport(const InverseResult& result)
{
    std::string text = "Inverse  " + result.from.label() + "  ->  " + result.to.label() + "\n";
    text += "  dE " + signedFixed(result.deltaEasting, 3) + "   dN " +
            signedFixed(result.deltaNorthing, 3) + "\n";
    text += "  Horizontal distance " + fixed(result.horizontalDistance, 3) + "\n";
    text += "  Azimuth " + dms(result.azimuth) + "   back azimuth " +
            dms(survey::normalizeAzimuth(result.azimuth + katana::math::kPi)) + "\n";
    text += "  Bearing " + bearing(result.azimuth);
    if (result.heightDifference) {
        text += "\n  Height difference " + signedFixed(*result.heightDifference, 3) +
                "   slope distance " + fixed(*result.slopeDistance, 3) + "   grade " +
                signedFixed(*result.grade * 100.0, 3) + " %";
    } else {
        // Said, not left out: a missing line reads as "flat".
        const SurveyPosition& missing = result.from.elevation ? result.to : result.from;
        text += "\n  No height difference: " + missing.label() + " has no elevation";
    }
    return text;
}

// ---- forward -------------------------------------------------------------------------------

Result<ForwardResult> computeForward(const ForwardInput& input)
{
    if (!std::isfinite(input.distance) || !(input.distance > 0.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         "the horizontal distance must be a positive number",
                         std::to_string(input.distance));
    }
    if (input.heightDifference) {
        if (!std::isfinite(*input.heightDifference)) {
            return makeError(ErrorCode::InvalidArgument, "the height difference is not finite");
        }
        if (!input.from.elevation) {
            // Absent is not zero: a height difference from a mark with no
            // height would put the new point at a height nobody measured.
            return makeError(ErrorCode::InvalidArgument,
                             "a height difference was given but the start has no elevation",
                             input.from.label());
        }
    }
    const auto reached = survey::forward(toSurvey(input.from.point), input.azimuth, input.distance);
    if (!reached) {
        return reached.error();
    }
    ForwardResult result;
    result.input = input;
    result.input.azimuth = survey::normalizeAzimuth(input.azimuth);
    result.point = fromSurvey(*reached);
    if (input.heightDifference) {
        result.elevation = *input.from.elevation + *input.heightDifference;
    }
    return result;
}

std::string formatForwardReport(const ForwardResult& result)
{
    const ForwardInput& input = result.input;
    std::string text = "Forward from " + input.from.label() + "\n";
    text += "  Azimuth " + dms(input.azimuth) + "   bearing " + bearing(input.azimuth) + "\n";
    text += "  Horizontal distance " + fixed(input.distance, 3);
    if (input.heightDifference) {
        text += "   height difference " + signedFixed(*input.heightDifference, 3);
    }
    text += "\n  New point" + (input.name.empty() ? std::string{} : " " + input.name) + "  " +
            coordinateText(result.point);
    text += result.elevation ? " Z " + fixed(*result.elevation, 3) : std::string("  (no elevation)");
    return text;
}

Result<commands::CommandPtr> forwardPointCommand(const Document& document,
                                                 const ForwardResult& result)
{
    const commands::EntityAttributes attributes = document.currentAttributes();
    Entity entity;
    entity.geometry = katana::entity::PointGeometry{result.point};
    entity.layer = attributes.layer;
    entity.style = attributes.style;
    entity.color = attributes.color;
    if (!result.input.name.empty()) {
        entity.properties.insert_or_assign(pointNameProperty(),
                                           katana::entity::PropertyValue(result.input.name));
    }
    // The one writer of heights (entity.hpp): no property at all for a point
    // without one, rather than 0.0.
    katana::entity::setHeights(entity.properties, {result.elevation});
    auto transaction = std::make_unique<commands::Transaction>("SURVEY_FORWARD_POINT");
    transaction->add(commands::createEntities({std::move(entity)}));
    return commands::CommandPtr{std::move(transaction)};
}

// ---- area ----------------------------------------------------------------------------------

bool isMetreUnit(std::string_view linearUnit)
{
    const std::string unit = katana::core::lowered(katana::core::trimmed(linearUnit));
    return unit == "metre" || unit == "meter" || unit == "metres" || unit == "meters" ||
           unit == "m";
}

Result<AreaResult> computeArea(const Document& document, const std::vector<EntityId>& ids)
{
    if (ids.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "nothing to measure; select closed polylines or circles");
    }
    AreaResult result;
    result.linearUnit = document.metadata().linearUnit;
    result.metres = isMetreUnit(result.linearUnit);
    for (const EntityId id : ids) {
        const Entity* entity = findEntity(document, id);
        if (entity == nullptr) {
            return makeError(ErrorCode::NotFound, "no entity with that id", std::to_string(id));
        }
        if (const auto* circle = std::get_if<katana::geometry::Circle2>(&entity->geometry)) {
            result.items.push_back(
                {id, katana::entity::EntityType::Circle, circle->area(), circle->perimeter()});
            continue;
        }
        const auto* polyline = std::get_if<katana::geometry::Polyline2>(&entity->geometry);
        if (polyline == nullptr) {
            result.skipped.push_back(
                {id, "a " + std::string(katana::entity::toString(entity->type())) +
                         " has no area"});
            continue;
        }
        if (!polyline->closed) {
            result.skipped.push_back({id, "an open polyline has no area"});
            continue;
        }
        std::vector<Coordinate2> vertices;
        vertices.reserve(polyline->vertices.size());
        for (const Point2& vertex : polyline->vertices) {
            vertices.push_back(toSurvey(vertex));
        }
        const auto area = survey::polygonArea(vertices);
        const auto perimeter = survey::polygonPerimeter(vertices);
        if (!area || !perimeter) {
            result.skipped.push_back(
                {id, "not a polygon: " + (area ? perimeter.error() : area.error()).message});
            continue;
        }
        result.items.push_back({id, katana::entity::EntityType::Polyline, *area, *perimeter});
    }
    if (result.items.empty()) {
        std::string reasons;
        for (const AreaSkip& skip : result.skipped) {
            reasons += (reasons.empty() ? "" : "; ") + std::to_string(skip.id) + ": " + skip.reason;
        }
        return makeError(ErrorCode::InvalidGeometry,
                         "none of these is a closed polyline or a circle", reasons);
    }
    for (const AreaItem& item : result.items) {
        result.totalArea += item.area;
        result.totalPerimeter += item.perimeter;
    }
    return result;
}

std::string formatAreaReport(const AreaResult& result)
{
    // One hectare is 10 000 m^2 by definition (SI Brochure, 9th edition,
    // table 8: the hectare, accepted for use with the SI).
    constexpr double kSquareMetresPerHectare = 10'000.0;
    const std::string squareUnit =
        result.metres ? std::string("m\xC2\xB2") : "square " + result.linearUnit;
    const std::string lengthUnit = result.metres ? std::string("m") : result.linearUnit;
    const auto area = [&](double value) {
        std::string text = fixed(value, 3) + " " + squareUnit;
        if (result.metres) {
            text += " (" + fixed(value / kSquareMetresPerHectare, 4) + " ha)";
        }
        return text;
    };

    std::string text = "Area of " + std::to_string(result.items.size()) +
                       (result.items.size() == 1 ? " outline" : " outlines") +
                       " - drawing unit '" + result.linearUnit + "' (project settings)\n";
    for (const AreaItem& item : result.items) {
        text += "  " + std::to_string(item.id) +
                (item.type == katana::entity::EntityType::Circle ? "  circle    "
                                                                  : "  polyline  ") +
                "area " + area(item.area) + "   perimeter " + fixed(item.perimeter, 3) + " " +
                lengthUnit + "\n";
    }
    text += "  Total  area " + area(result.totalArea) + "   perimeter " +
            fixed(result.totalPerimeter, 3) + " " + lengthUnit;
    for (const AreaSkip& skip : result.skipped) {
        text += "\n  Skipped " + std::to_string(skip.id) + ": " + skip.reason;
    }
    if (!result.metres) {
        text += "\n  No hectares: they are defined only for a drawing in metres";
    }
    text += "\n  A self-intersecting outline is not detected; the areas of its lobes cancel";
    return text;
}

// ---- traverse ------------------------------------------------------------------------------

std::string_view toString(TraverseMethod method)
{
    switch (method) {
    case TraverseMethod::None:
        return "no adjustment";
    case TraverseMethod::Compass:
        return "Bowditch (compass) rule";
    case TraverseMethod::Transit:
        return "transit rule";
    case TraverseMethod::LeastSquares:
        return "least squares";
    }
    return "unknown method";
}

survey::TraverseStochasticModel defaultTraversePrecision()
{
    survey::TraverseStochasticModel model;
    model.distanceSigmaConstant = 0.002; // 2 mm
    model.distanceSigmaPpm = 2.0;        // + 2 ppm
    model.angleSigma = survey::arcSecondsToRadians(5.0);
    model.azimuthSigma = survey::arcSecondsToRadians(0.01);
    return model;
}

Result<TraverseToolResult> computeTraverseTool(const TraverseSpec& spec)
{
    if (spec.legs.empty()) {
        return makeError(ErrorCode::InvalidArgument, "a traverse needs at least one leg",
                         "traverse '" + spec.name + "'");
    }
    TraverseToolResult result;
    result.spec = spec;

    const bool loop = spec.kind == survey::TraverseKind::ClosedLoop;
    const auto startAzimuth =
        orientationAzimuth(spec.start, spec.startOrientation, loop ? "first-leg" : "backsight");
    if (!startAzimuth) {
        return startAzimuth.error();
    }
    result.startAzimuth = *startAzimuth;

    survey::Traverse traverse;
    traverse.name = spec.name;
    traverse.kind = spec.kind;
    traverse.start = toSurvey(spec.start);
    traverse.startAzimuth = *startAzimuth;
    traverse.setups = spec.legs;
    if (!loop) {
        traverse.endStationId = spec.endStation;
    }
    if (spec.kind == survey::TraverseKind::Link) {
        traverse.end = toSurvey(spec.end);
        if (spec.closingAngle) {
            const auto reference =
                orientationAzimuth(spec.end, spec.closingOrientation, "closing reference");
            if (!reference) {
                return reference.error();
            }
            result.closingReferenceAzimuth = *reference;
            traverse.closingAngle = survey::TraverseClosingAngle{*spec.closingAngle, *reference};
        }
    }

    survey::TraverseOptions options;
    options.balanceAngles = spec.balanceAngles;
    switch (spec.method) {
    case TraverseMethod::Compass:
        options.adjustment = survey::TraverseAdjustment::Compass;
        break;
    case TraverseMethod::Transit:
        options.adjustment = survey::TraverseAdjustment::Transit;
        break;
    case TraverseMethod::None:
    case TraverseMethod::LeastSquares:
        // Least squares is reported beside the classical misclosures, which are
        // what a surveyor checks the field work against before adjusting at all.
        options.adjustment = survey::TraverseAdjustment::None;
        break;
    }
    auto classical = survey::computeTraverse(traverse, options);
    if (!classical) {
        return classical.error();
    }
    result.classical = std::move(*classical);

    if (spec.method == TraverseMethod::LeastSquares) {
        auto adjusted = survey::adjustTraverse(traverse, spec.precision);
        if (!adjusted) {
            return adjusted.error();
        }
        result.leastSquares = std::move(*adjusted);
    }

    const std::string& first = spec.legs.front().stationId;
    for (const survey::TraverseStationResult& station : result.classical.stations) {
        TraverseStationRow row;
        row.id = station.id;
        row.unadjusted = fromSurvey(station.unadjusted);
        row.adjusted = fromSurvey(station.adjusted);
        row.control = station.id == first ||
                      (spec.kind == survey::TraverseKind::Link && station.id == spec.endStation);
        if (result.leastSquares) {
            const auto& stations = result.leastSquares->stations;
            const auto found = std::find_if(stations.begin(), stations.end(), [&](const auto& s) {
                return s.pointId == station.id;
            });
            if (found == stations.end()) {
                return makeError(ErrorCode::Internal,
                                 "the least-squares adjustment has no result for a station",
                                 station.id);
            }
            row.adjusted = fromSurvey(found->position);
            row.sigmaEasting = found->sigmaEasting;
            row.sigmaNorthing = found->sigmaNorthing;
        }
        result.stations.push_back(std::move(row));
    }
    return result;
}

std::string formatTraverseReport(const TraverseToolResult& result)
{
    const TraverseSpec& spec = result.spec;
    const survey::TraverseResult& classical = result.classical;
    const char* kind = spec.kind == survey::TraverseKind::ClosedLoop ? "closed loop"
                       : spec.kind == survey::TraverseKind::Link    ? "link traverse"
                                                                    : "open traverse";
    std::string text = "Traverse '" + spec.name + "' - " + kind + ", " +
                       std::to_string(spec.legs.size()) +
                       (spec.legs.size() == 1 ? " leg, " : " legs, ") +
                       std::string(toString(spec.method)) + "\n";
    text += "  Start " + spec.legs.front().stationId + "  " + coordinateText(spec.start) + ", " +
            (spec.kind == survey::TraverseKind::ClosedLoop ? "first-leg azimuth "
                                                           : "backsight azimuth ") +
            dms(result.startAzimuth) + "\n";
    if (spec.kind == survey::TraverseKind::Link) {
        text += "  Closing control " + spec.endStation + "  " + coordinateText(spec.end);
        text += result.closingReferenceAzimuth
                    ? ", reference azimuth " + dms(*result.closingReferenceAzimuth) + "\n"
                    : ", no closing angle (no angular check)\n";
    }

    if (classical.angularMisclosure) {
        text += "  Angular misclosure " + dms(*classical.angularMisclosure) + " over " +
                std::to_string(classical.angleCount) + " angles";
        text += spec.balanceAngles
                    ? "; " + dms(classical.angleCorrection) + " applied to each\n"
                    : "; not balanced\n";
    } else {
        text += "  No angular check\n";
    }
    if (classical.linearMisclosure) {
        const survey::LinearMisclosure& linear = *classical.linearMisclosure;
        text += "  Linear misclosure dN " + signedFixed(linear.latitude, 3) + "  dE " +
                signedFixed(linear.departure, 3) + "  length " + fixed(linear.length, 3) +
                "  over " + fixed(classical.totalLength, 3);
        text += linear.precisionDenominator
                    ? "  precision 1 : " + std::format("{:.0f}", *linear.precisionDenominator) +
                          "\n"
                    : "  (exact closure)\n";
    } else {
        text += "  Open traverse: no linear misclosure - nothing checks it\n";
    }

    text += "  Legs\n    from        to          azimuth            distance    dN          dE";
    const bool corrected =
        spec.method == TraverseMethod::Compass || spec.method == TraverseMethod::Transit;
    text += corrected ? "          corr N    corr E\n" : "\n";
    for (const survey::TraverseLegResult& leg : classical.legs) {
        text += std::format("    {:<11} {:<11} {:<18} {:>10}  {:>10}  {:>10}", leg.fromId,
                            leg.toId, dms(leg.azimuth), fixed(leg.distance, 3),
                            signedFixed(leg.latitude, 3), signedFixed(leg.departure, 3));
        if (corrected) {
            text += std::format("  {:>8}  {:>8}", signedFixed(leg.latitudeCorrection, 4),
                                signedFixed(leg.departureCorrection, 4));
        }
        text += "\n";
    }

    text += "  Stations            unadjusted E   unadjusted N     adjusted E     adjusted N";
    text += result.leastSquares ? "   sE (mm)   sN (mm)\n" : "\n";
    for (const TraverseStationRow& row : result.stations) {
        text += std::format("    {:<16} {:>14} {:>14} {:>14} {:>14}", row.id,
                            fixed(row.unadjusted.x, 3), fixed(row.unadjusted.y, 3),
                            fixed(row.adjusted.x, 3), fixed(row.adjusted.y, 3));
        if (row.sigmaEasting && row.sigmaNorthing) {
            text += std::format("  {:>8}  {:>8}", fixed(*row.sigmaEasting * 1000.0, 1),
                                fixed(*row.sigmaNorthing * 1000.0, 1));
        }
        text += row.control ? "  control\n" : "\n";
    }

    if (result.leastSquares) {
        const survey::AdjustmentStatistics& statistics = result.leastSquares->statistics;
        const survey::TraverseStochasticModel& p = spec.precision;
        text += "  Least squares weighted with angles " +
                fixed(survey::radiansToArcSeconds(p.angleSigma), 1) + "\", distances " +
                fixed(p.distanceSigmaConstant * 1000.0, 1) + " mm + " +
                fixed(p.distanceSigmaPpm, 1) + " ppm; " +
                std::to_string(statistics.degreesOfFreedom) + " degrees of freedom, " +
                std::to_string(result.leastSquares->iterations) + " iterations";
        if (statistics.varianceFactor) {
            text += "; variance factor " + fixed(*statistics.varianceFactor, 3);
        }
        if (statistics.globalTest) {
            text += statistics.globalTest->passed ? "; global test passed"
                                                  : "; global test FAILED - check the weights "
                                                    "and look for a gross error";
        }
        text += "\n";
    }
    if (text.ends_with('\n')) {
        text.pop_back();
    }
    return text;
}

Result<commands::CommandPtr> traverseCommand(const Document& document,
                                             const TraverseToolResult& result)
{
    if (result.stations.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the traverse has no stations");
    }
    const commands::EntityAttributes attributes = document.currentAttributes();
    const auto withAttributes = [&](Entity entity) {
        entity.layer = attributes.layer;
        entity.style = attributes.style;
        entity.color = attributes.color;
        return entity;
    };
    std::vector<Entity> entities;
    for (const TraverseStationRow& row : result.stations) {
        // Control is already known - usually already in the drawing - so only
        // the stations this traverse computed become new points.
        if (row.control) {
            continue;
        }
        Entity point;
        point.geometry = katana::entity::PointGeometry{row.adjusted};
        point.properties.insert_or_assign(pointNameProperty(),
                                          katana::entity::PropertyValue(row.id));
        entities.push_back(withAttributes(std::move(point)));
    }
    katana::geometry::Polyline2 line;
    for (const TraverseStationRow& row : result.stations) {
        line.vertices.push_back(row.adjusted);
    }
    // A loop comes back to its first station: said with the closed flag rather
    // than a repeated vertex, as everywhere in the drawing. A link traverse
    // closing on its own start is the same figure.
    line.closed = result.spec.kind == survey::TraverseKind::ClosedLoop;
    if (result.stations.size() > 1 && result.stations.back().id == result.stations.front().id) {
        line.vertices.pop_back();
        line.closed = true;
    }
    if (line.vertices.size() >= 2) {
        Entity polyline;
        polyline.geometry = std::move(line);
        polyline.properties.insert_or_assign("traverse",
                                             katana::entity::PropertyValue(result.spec.name));
        entities.push_back(withAttributes(std::move(polyline)));
    }
    auto transaction = std::make_unique<commands::Transaction>("SURVEY_TRAVERSE");
    transaction->add(commands::createEntities(std::move(entities)));
    return commands::CommandPtr{std::move(transaction)};
}

Result<std::vector<survey::TraverseSetup>> parseTraverseLegs(std::string_view text)
{
    std::vector<survey::TraverseSetup> legs;
    const auto lines = katana::core::splitLines(text);
    for (std::size_t i = 0; i < lines.size(); ++i) {
        const auto parts = fields(lines[i]);
        if (parts.empty()) {
            continue;
        }
        const std::string context = lineContext(i + 1, lines[i]);
        if (parts.size() != 3) {
            return makeError(ErrorCode::ParseFailure,
                             "a leg is 'station angle distance' - three fields, the angle "
                             "without blanks inside it (36d52m11.63s)",
                             context);
        }
        const auto angle = parseSurveyAngle(parts[1]);
        if (!angle) {
            return makeError(ErrorCode::ParseFailure, angle.error().message, context);
        }
        const auto distance = parseSurveyNumber(parts[2], "the distance");
        if (!distance) {
            return makeError(ErrorCode::ParseFailure, distance.error().message, context);
        }
        legs.push_back(survey::TraverseSetup{std::string(parts[0]), *angle, *distance});
    }
    if (legs.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "no legs: one line per station, 'station angle distance'");
    }
    return legs;
}

// ---- level book ----------------------------------------------------------------------------

Result<std::vector<LevelBookLine>> parseLevelBook(std::string_view text)
{
    std::vector<LevelBookLine> book;
    const auto lines = katana::core::splitLines(text);
    for (std::size_t i = 0; i < lines.size(); ++i) {
        const auto parts = fields(lines[i]);
        if (parts.empty()) {
            continue;
        }
        const std::string context = lineContext(i + 1, lines[i]);
        if (parts.size() != 4 && parts.size() != 5) {
            return makeError(ErrorCode::ParseFailure,
                             "a line is 'point BS IS FS [distance]', '-' for an empty column",
                             context);
        }
        LevelBookLine line;
        line.point = std::string(parts[0]);
        line.lineNumber = i + 1;
        const auto reading = [&](std::size_t index,
                                 std::string_view what) -> Result<std::optional<double>> {
            if (parts[index] == "-") {
                return std::optional<double>{};
            }
            const auto value = parseSurveyNumber(parts[index], what);
            if (!value) {
                return makeError(ErrorCode::ParseFailure, value.error().message, context);
            }
            return std::optional<double>{*value};
        };
        const auto backsight = reading(1, "the backsight");
        const auto intersight = reading(2, "the intersight");
        const auto foresight = reading(3, "the foresight");
        for (const auto* value : {&backsight, &intersight, &foresight}) {
            if (!*value) {
                return value->error();
            }
        }
        line.backsight = *backsight;
        line.intersight = *intersight;
        line.foresight = *foresight;
        if (parts.size() == 5) {
            const auto distance = reading(4, "the distance");
            if (!distance) {
                return distance.error();
            }
            if (*distance && !(**distance >= 0.0)) {
                return makeError(ErrorCode::ParseFailure, "a distance cannot be negative",
                                 context);
            }
            line.distance = *distance;
        }
        if (!line.backsight && !line.intersight && !line.foresight) {
            return makeError(ErrorCode::ParseFailure, "the line has no reading", context);
        }
        book.push_back(std::move(line));
    }
    return book;
}

Result<LevelBookResult> computeLevelBook(const LevelBookSpec& spec)
{
    const auto& lines = spec.lines;
    const auto refuse = [](const LevelBookLine& line, std::string message) {
        return makeError(ErrorCode::InvalidArgument, std::move(message),
                         "line " + std::to_string(line.lineNumber) + " (" + line.point + ")");
    };
    if (lines.size() < 2) {
        return makeError(ErrorCode::InvalidArgument,
                         "a level book needs a backsight on the benchmark and a foresight");
    }
    // The shape of a height-of-collimation book, checked line by line so that
    // a booking error is named where it is rather than surfacing as a strange
    // reduced level further down.
    const LevelBookLine& opening = lines.front();
    if (!opening.backsight || opening.intersight || opening.foresight) {
        return refuse(opening, "the book starts with a backsight, and only a backsight, on the "
                               "benchmark");
    }
    if (opening.distance) {
        return refuse(opening, "a distance belongs on a foresight line, the end of its setup");
    }
    for (std::size_t i = 1; i < lines.size(); ++i) {
        const LevelBookLine& line = lines[i];
        if (!line.backsight && !line.intersight && !line.foresight) {
            return refuse(line, "the line has no reading");
        }
        if (line.intersight && (line.backsight || line.foresight)) {
            return refuse(line, "an intersight line carries no other reading");
        }
        if (line.backsight && !line.foresight) {
            return refuse(line, "a backsight after the first must be on a change point, booked "
                                "with that point's foresight");
        }
        if (line.distance && !line.foresight) {
            return refuse(line, "a distance belongs on a foresight line, the end of its setup");
        }
    }
    if (!lines.back().foresight || lines.back().backsight) {
        return refuse(lines.back(), "the book ends on a foresight; a backsight here starts a "
                                    "setup that is never finished");
    }

    // The setups the survey library levels: each runs from a backsight to the
    // next foresight. Intersights hang off the setup they were read from.
    survey::LevelRun run;
    run.name = spec.name;
    run.startElevation = spec.startLevel;
    run.closingElevation = spec.closingLevel;
    std::vector<std::size_t> setupOfLine(lines.size(), 0); // 0-based setup index
    std::size_t withDistance = 0;
    double backsight = *opening.backsight;
    for (std::size_t i = 1; i < lines.size(); ++i) {
        const LevelBookLine& line = lines[i];
        setupOfLine[i] = run.setups.size();
        if (!line.foresight) {
            continue; // an intersight
        }
        survey::LevelSetup setup;
        setup.foresightPointId = line.point;
        setup.backsight = backsight;
        setup.foresight = *line.foresight;
        setup.distance = line.distance.value_or(0.0);
        withDistance += line.distance ? 1 : 0;
        run.setups.push_back(std::move(setup));
        if (line.backsight) {
            backsight = *line.backsight;
        }
    }
    // A length on some setups and not others would weight the adjustment by
    // lengths that are partly zero, and the allowable misclosure would be
    // computed over too short a line. Both are refused or left unstated
    // rather than computed from a zero nobody measured.
    const bool allDistances = withDistance == run.setups.size();
    if (spec.adjustment == survey::LevelAdjustment::ByDistance && !allDistances) {
        return makeError(ErrorCode::InvalidArgument,
                         "adjustment by distance needs the length levelled on every foresight "
                         "line",
                         std::to_string(run.setups.size() - withDistance) + " of " +
                             std::to_string(run.setups.size()) + " setups have none");
    }
    auto levelled = survey::computeLevelRun(run, spec.adjustment);
    if (!levelled) {
        return levelled.error();
    }

    LevelBookResult result;
    result.spec = spec;
    result.run = std::move(*levelled);
    const auto& points = result.run.points;
    for (std::size_t i = 0; i < lines.size(); ++i) {
        const LevelBookLine& line = lines[i];
        LevelBookRow row;
        row.line = line;
        if (i == 0) {
            row.heightOfCollimation = points.front().heightOfInstrument;
            row.reducedLevel = spec.startLevel;
            row.adjustedLevel = spec.startLevel;
        } else {
            const std::size_t setup = setupOfLine[i];
            const survey::LevelPointResult& point = points[setup];
            row.setup = setup + 1;
            if (line.intersight) {
                // THE reduction the library does not do: the height of
                // collimation it computed for this setup, minus the staff
                // reading, corrected like every other reading of the setup.
                row.reducedLevel = point.heightOfInstrument - *line.intersight;
                row.correction = point.correction;
                row.adjustedLevel = row.reducedLevel + row.correction;
            } else {
                row.reducedLevel = point.elevation;
                row.correction = point.correction;
                row.adjustedLevel = point.adjustedElevation;
                if (line.backsight) {
                    row.heightOfCollimation = points[setup + 1].heightOfInstrument;
                }
            }
        }
        result.rows.push_back(std::move(row));
    }
    result.sumBacksights = result.run.sumBacksights;
    result.sumForesights = result.run.sumForesights;
    result.riseMinusFall = result.sumBacksights - result.sumForesights;
    result.lastMinusFirst = points.back().elevation - spec.startLevel;
    result.misclosure = result.run.misclosure;
    if (allDistances) {
        result.totalDistance = result.run.totalDistance;
        auto allowable = survey::allowableLevelMisclosure(spec.allowanceK, result.run.totalDistance);
        if (!allowable) {
            return allowable.error();
        }
        result.allowable = *allowable;
    }
    return result;
}

std::string formatLevelBookReport(const LevelBookResult& result)
{
    const LevelBookSpec& spec = result.spec;
    const char* adjustment = spec.adjustment == survey::LevelAdjustment::BySetups
                                 ? "misclosure shared equally between setups"
                             : spec.adjustment == survey::LevelAdjustment::ByDistance
                                 ? "misclosure shared by distance levelled"
                                 : "no adjustment";
    std::string text = "Level run '" + spec.name + "' - opening level " +
                       fixed(spec.startLevel, 3) +
                       (spec.closingLevel ? ", closing level " + fixed(*spec.closingLevel, 3)
                                          : std::string(", no closing benchmark")) +
                       ", " + adjustment + "\n";
    const auto cell = [](const std::optional<double>& value) {
        return value ? fixed(*value, 3) : std::string{};
    };
    text += std::format("  {:<12} {:>8} {:>8} {:>8} {:>10} {:>10} {:>8} {:>10}\n", "Point", "BS",
                        "IS", "FS", "HPC", "RL", "Corr", "Adj RL");
    for (const LevelBookRow& row : result.rows) {
        const bool corrected = spec.adjustment != survey::LevelAdjustment::None && row.setup > 0;
        text += std::format("  {:<12} {:>8} {:>8} {:>8} {:>10} {:>10} {:>8} {:>10}\n",
                            row.line.point, cell(row.line.backsight), cell(row.line.intersight),
                            cell(row.line.foresight), cell(row.heightOfCollimation),
                            fixed(row.reducedLevel, 3),
                            corrected ? signedFixed(row.correction, 4) : std::string{},
                            corrected ? fixed(row.adjustedLevel, 3) : std::string{});
    }
    text += "  Check: sum BS " + fixed(result.sumBacksights, 3) + " - sum FS " +
            fixed(result.sumForesights, 3) + " = " + signedFixed(result.riseMinusFall, 3) +
            "; last RL - first RL = " + signedFixed(result.lastMinusFirst, 3) + "\n";
    if (result.misclosure) {
        text += "  Misclosure " + signedFixed(*result.misclosure, 4) + " (computed - known)";
        if (result.allowable && result.totalDistance) {
            const bool within = std::abs(*result.misclosure) <= *result.allowable;
            text += "; allowable " + fixed(*result.allowable, 4) + " (" +
                    fixed(spec.allowanceK * 1000.0, 1) + " mm x sqrt(" +
                    fixed(*result.totalDistance / 1000.0, 3) + " km)) - " +
                    (within ? "within it" : "EXCEEDS it");
        } else {
            text += "; no allowable misclosure: not every setup has a length levelled";
        }
    } else {
        text += "  No closing benchmark: the run is not checked";
    }
    return text;
}

// ---- angle calculator ----------------------------------------------------------------------

Result<AngleConversion> convertAngle(std::string_view text, AngleInputUnit unit)
{
    Result<double> radians = makeError(ErrorCode::Internal, "unhandled unit");
    switch (unit) {
    case AngleInputUnit::Degrees:
        radians = parseSurveyAngle(text);
        break;
    case AngleInputUnit::Gons:
        if (auto gons = parseSurveyNumber(text, "an angle in gons")) {
            radians = survey::gonToRadians(*gons);
        } else {
            radians = gons.error();
        }
        break;
    case AngleInputUnit::Radians:
        radians = parseSurveyNumber(text, "an angle in radians");
        break;
    case AngleInputUnit::Bearing:
        radians = survey::parseBearing(text);
        break;
    }
    if (!radians) {
        return radians.error();
    }
    AngleConversion conversion;
    conversion.unit = unit;
    conversion.radians = *radians;
    conversion.degrees = survey::radiansToDegrees(*radians);
    conversion.gons = survey::radiansToGon(*radians);
    conversion.azimuth = survey::normalizeAzimuth(*radians);
    conversion.backAzimuth = survey::normalizeAzimuth(conversion.azimuth + katana::math::kPi);
    return conversion;
}

std::string formatAngleConversion(const AngleConversion& conversion)
{
    std::string text = "Angle " + dms(conversion.radians) + "  =  " +
                       std::format("{:.9f}", conversion.degrees) + "\xC2\xB0  =  " +
                       std::format("{:.8f}", conversion.gons) + " gon  =  " +
                       std::format("{:.10f}", conversion.radians) + " rad\n";
    text += "  As a direction: azimuth " + dms(conversion.azimuth) + "   bearing " +
            bearing(conversion.azimuth) + "   reverse " + dms(conversion.backAzimuth);
    return text;
}

// ---- coordinate converter ------------------------------------------------------------------

namespace {

Result<geodesy::CoordinateReferenceSystem> crsFromText(std::string_view text)
{
    const std::string_view body = katana::core::trimmed(text);
    if (body.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "name a coordinate system - an EPSG code such as 4326 or 28356");
    }
    if (const auto code = katana::core::parseInteger(body)) {
        if (*code <= 0 || *code > 2'147'483'647) {
            return makeError(ErrorCode::InvalidCRS, "not an EPSG code", std::string(body));
        }
        return geodesy::CoordinateReferenceSystem::fromEpsg(static_cast<int>(*code));
    }
    return geodesy::CoordinateReferenceSystem::fromUserInput(body);
}

std::string crsTitle(const geodesy::CoordinateReferenceSystem& crs)
{
    std::string title = crs.name();
    if (!crs.authority().empty()) {
        title = crs.authority() + ":" + crs.code() + " " + title;
    }
    return title;
}

Status requireHorizontal(const geodesy::CoordinateReferenceSystem& crs)
{
    if (crs.isGeographic() || crs.isProjected()) {
        return {};
    }
    return makeError(ErrorCode::Unsupported,
                     "the converter works between geographic and projected systems; this is " +
                         std::string(geodesy::toString(crs.kind())),
                     crsTitle(crs));
}

} // namespace

Result<CoordinateConversion> convertCoordinates(std::string_view source, std::string_view target,
                                                std::string_view points)
{
    auto sourceCrs = crsFromText(source);
    if (!sourceCrs) {
        return sourceCrs.error();
    }
    auto targetCrs = crsFromText(target);
    if (!targetCrs) {
        return targetCrs.error();
    }
    for (const auto* crs : {&*sourceCrs, &*targetCrs}) {
        if (auto status = requireHorizontal(*crs); !status) {
            return status.error();
        }
    }
    auto transformer = geodesy::CoordinateTransformer::create(*sourceCrs, *targetCrs);
    if (!transformer) {
        return transformer.error();
    }

    CoordinateConversion conversion;
    conversion.sourceName = crsTitle(*sourceCrs);
    conversion.targetName = crsTitle(*targetCrs);
    conversion.sourceGeographic = sourceCrs->isGeographic();
    conversion.targetGeographic = targetCrs->isGeographic();
    conversion.operation = transformer->operation().name;
    conversion.accuracyMetres = transformer->operation().accuracyMetres;

    std::optional<geodesy::GridFactorCalculator> sourceFactors;
    std::optional<geodesy::GridFactorCalculator> targetFactors;
    for (auto [crs, calculator] : {std::pair{&*sourceCrs, &sourceFactors},
                                   std::pair{&*targetCrs, &targetFactors}}) {
        if (crs->isProjected()) {
            auto created = geodesy::GridFactorCalculator::create(*crs);
            if (!created) {
                return created.error();
            }
            calculator->emplace(std::move(*created));
        }
    }

    const auto lines = katana::core::splitLines(points);
    for (std::size_t i = 0; i < lines.size(); ++i) {
        const auto parts = fields(lines[i]);
        if (parts.empty()) {
            continue;
        }
        const std::string context = lineContext(i + 1, lines[i]);
        if (parts.size() < 2) {
            return makeError(ErrorCode::ParseFailure,
                             conversion.sourceGeographic
                                 ? "a point is '[label] latitude longitude'"
                                 : "a point is '[label] easting northing'",
                             context);
        }
        CoordinateConversionRow row;
        // Everything before the two coordinates is the label, so a point name
        // with a blank in it survives.
        for (std::size_t k = 0; k + 2 < parts.size(); ++k) {
            row.label += (row.label.empty() ? "" : " ") + std::string(parts[k]);
        }
        if (row.label.empty()) {
            row.label = "P" + std::to_string(conversion.rows.size() + 1);
        }
        const std::string_view firstText = parts[parts.size() - 2];
        const std::string_view secondText = parts[parts.size() - 1];
        if (conversion.sourceGeographic) {
            const auto latitude = parseSurveyAngle(firstText);
            const auto longitude = parseSurveyAngle(secondText);
            if (!latitude || !longitude) {
                return makeError(ErrorCode::ParseFailure,
                                 (latitude ? longitude.error() : latitude.error()).message,
                                 context);
            }
            row.inFirst = survey::radiansToDegrees(*latitude);
            row.inSecond = survey::radiansToDegrees(*longitude);
        } else {
            const auto easting = parseSurveyNumber(firstText, "the easting");
            const auto northing = parseSurveyNumber(secondText, "the northing");
            if (!easting || !northing) {
                return makeError(ErrorCode::ParseFailure,
                                 (easting ? northing.error() : easting.error()).message, context);
            }
            row.inFirst = *easting;
            row.inSecond = *northing;
        }

        // The generic Coordinate is traditional GIS order - x longitude or
        // easting, y latitude or northing (geodesy/coordinate.hpp) - so the
        // latitude-first rows are swapped exactly here, and back below.
        geodesy::Coordinate in;
        in.x = conversion.sourceGeographic ? row.inSecond : row.inFirst;
        in.y = conversion.sourceGeographic ? row.inFirst : row.inSecond;
        const auto out = transformer->forward(in);
        if (!out) {
            return makeError(out.error().code, out.error().message,
                             context + " " + out.error().context);
        }
        row.outFirst = conversion.targetGeographic ? out->y : out->x;
        row.outSecond = conversion.targetGeographic ? out->x : out->y;

        if (sourceFactors) {
            auto factors =
                sourceFactors->at(geodesy::ProjectedCoordinate{row.inFirst, row.inSecond, 0.0});
            if (!factors) {
                return makeError(factors.error().code, factors.error().message, context);
            }
            row.sourceFactors = *factors;
        }
        if (targetFactors) {
            auto factors =
                targetFactors->at(geodesy::ProjectedCoordinate{row.outFirst, row.outSecond, 0.0});
            if (!factors) {
                return makeError(factors.error().code, factors.error().message, context);
            }
            row.targetFactors = *factors;
        }
        conversion.rows.push_back(std::move(row));
    }
    if (conversion.rows.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "no points to convert: one per line, '[label] first second'");
    }
    return conversion;
}

std::string formatCoordinateConversion(const CoordinateConversion& conversion)
{
    const auto axes = [](bool geographic) {
        return geographic ? std::string("latitude, longitude") : std::string("easting, northing");
    };
    const auto value = [](bool geographic, double number) {
        // 1e-9 degree is 0.1 mm on the ground; 1 mm is the resolution every
        // other survey tool here prints a grid coordinate to.
        return geographic ? std::format("{:.9f}", number) : fixed(number, 3);
    };
    const auto factors = [](const geodesy::GridFactors& f) {
        return "k " + std::format("{:.8f}", f.pointScaleFactor()) + " conv " +
               dms(survey::degreesToRadians(f.convergenceDegrees));
    };
    std::string text = "Convert " + conversion.sourceName + " (" +
                       axes(conversion.sourceGeographic) + ")  ->  " + conversion.targetName +
                       " (" + axes(conversion.targetGeographic) + ")\n";
    text += "  Operation: " + conversion.operation;
    text += conversion.accuracyMetres
                ? " (accuracy " + std::format("{:g}", *conversion.accuracyMetres) + " m)\n"
                : " (accuracy not stated)\n";
    for (const CoordinateConversionRow& row : conversion.rows) {
        text += "  " + row.label + "  " + value(conversion.sourceGeographic, row.inFirst) + " " +
                value(conversion.sourceGeographic, row.inSecond) + "  ->  " +
                value(conversion.targetGeographic, row.outFirst) + " " +
                value(conversion.targetGeographic, row.outSecond);
        if (row.sourceFactors) {
            text += "   source " + factors(*row.sourceFactors);
        }
        if (row.targetFactors) {
            text += "   target " + factors(*row.targetFactors);
        }
        text += "\n";
    }
    text += "  Heights are not converted, and nothing in the drawing is moved";
    return text;
}

Result<std::string> conversionLinesForSelection(const Document& document, std::string_view source)
{
    auto crs = crsFromText(source);
    if (!crs) {
        return crs.error();
    }
    if (crs->isGeographic()) {
        return makeError(ErrorCode::InvalidArgument,
                         "the drawing's points are easting and northing; choose the projected "
                         "system they are in as the source",
                         crsTitle(*crs));
    }
    std::string lines;
    for (const SurveyPosition& position : selectedPointPositions(document)) {
        std::string label =
            position.name.empty() ? "id" + std::to_string(*position.entity) : position.name;
        // '#' starts a comment and ',' separates fields in the lines this
        // makes; a name carrying either would come back cut short.
        std::replace(label.begin(), label.end(), '#', '_');
        std::replace(label.begin(), label.end(), ',', '_');
        lines += label + " " + katana::core::formatExactReal(position.point.x) + " " +
                 katana::core::formatExactReal(position.point.y) + "\n";
    }
    if (lines.empty()) {
        return makeError(ErrorCode::InvalidState, "no point entities are selected");
    }
    return lines;
}

} // namespace katana::cad
