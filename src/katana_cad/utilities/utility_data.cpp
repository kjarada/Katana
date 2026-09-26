#include "katana/cad/utilities/utility_data.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <iterator>
#include <limits>
#include <map>
#include <memory>
#include <set>
#include <type_traits>
#include <utility>
#include <variant>

#include "katana/cad/survey_coding.hpp"
#include "katana/cad/survey_import.hpp"
#include "katana/commands/change_set.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/chording.hpp"
#include "katana/geometry/profile.hpp"
#include "katana/math/numerics.hpp"
#include "katana/survey/subsurface/utility_csv.hpp"

namespace katana::cad::utilities {

namespace sub = katana::survey::subsurface;
namespace cmd = katana::commands;
using katana::core::Error;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::EntityType;
using katana::entity::Layer;
using katana::entity::Linetype;
using katana::entity::Model;
using katana::entity::PropertyMap;
using katana::entity::PropertyValue;

namespace {

// The line attributes every point of a line carries, and must agree on -
// with the settings the line was graded with.
constexpr std::array kLineKeys{keys::kType,          keys::kOwner,       keys::kMaterial,
                               keys::kDiameter,      keys::kDiameterInside,
                               keys::kConfiguration, keys::kDescription, keys::kStatus,
                               keys::kSpacing,       keys::kMinimumCover};

// Every key UTILITY DRAW writes: what REGRADE replaces on a point. Another
// "utility." property - a person's own note - is theirs, and stays.
constexpr std::array kDrawnKeys{
    keys::kLine,          keys::kType,         keys::kQualityLevel,
    keys::kOwner,         keys::kMaterial,     keys::kDiameter,
    keys::kDiameterInside, keys::kConfiguration, keys::kDescription,
    keys::kStatus,        keys::kLimitedBy,    keys::kLength,
    keys::kFrom,          keys::kTo,           keys::kVertex,
    keys::kOrder,         keys::kMethod,       keys::kClaimed,
    keys::kOverClaim,     keys::kLevel,        keys::kDepth,
    keys::kHorizontalUncertainty, keys::kVerticalUncertainty, keys::kPath,
    keys::kServiceLevel,  keys::kLevelReference, keys::kLevelQualified,
    keys::kSurfaceLevel,  keys::kCover,        keys::kCoverNote,
    keys::kCoverBelowMinimum, keys::kVerifies,   keys::kSpacing,
    keys::kMinimumCover};

bool isDrawnKey(std::string_view key)
{
    return key.starts_with(keys::kFieldPrefix) || key.starts_with(keys::kRecordedPrefix) ||
           std::ranges::find(kDrawnKeys, key) != kDrawnKeys.end();
}

// A cell kept as the schedule wrote it that is the line's, not a vertex's:
// the schedule reader keeps type, status and size per service, and ql and
// level_ref per vertex (subsurface::UtilityAttributes::recorded).
bool isLineRecorded(std::string_view column)
{
    return column == "type" || column == "status" || column == "size";
}

// The kept cells of `point` that are the line's (`line`) or its own.
std::map<std::string, std::string> recordedOf(const Entity& point, bool line)
{
    std::map<std::string, std::string> recorded;
    for (const auto& [key, value] : point.properties) {
        if (!key.starts_with(keys::kRecordedPrefix)) {
            continue;
        }
        const std::string_view column =
            std::string_view(key).substr(keys::kRecordedPrefix.size());
        if (isLineRecorded(column) == line) {
            recorded.emplace(std::string(column), katana::entity::toString(value));
        }
    }
    return recorded;
}

// A kept field the schedule format carries per service, as it did when the
// schedule was read. One it does not know is a point's own.
bool isLineField(std::string_view name)
{
    return std::ranges::any_of(sub::utilityCsvColumns(), [name](const sub::UtilityCsvColumn& column) {
        return column.name == name && column.carried == sub::CarriedOn::Line;
    });
}

const PropertyValue* valueOf(const Entity& entity, std::string_view key)
{
    const auto found = entity.properties.find(key);
    return found == entity.properties.end() ? nullptr : &found->second;
}

// Reads one point of one line, and says which when it cannot.
struct PointReader {
    const std::string& line;
    const Entity& point;
    // What to call it instead, when what is read is not a drawn point: the
    // line entity a service was read from ("polyline (#12)").
    std::string label{};

    [[nodiscard]] std::string name() const
    {
        if (!label.empty()) {
            return label;
        }
        const PropertyValue* vertex = valueOf(point, keys::kVertex);
        return "point " + (vertex ? katana::entity::toString(*vertex) + " " : std::string()) + "(#" +
               std::to_string(point.id) + ")";
    }

    [[nodiscard]] Error refuse(const std::string& what) const
    {
        return makeError(ErrorCode::InvalidArgument, "line " + line + ": " + name() + " " + what);
    }

    // Text as the property panel shows it, whatever the value's type.
    [[nodiscard]] std::optional<std::string> text(std::string_view key) const
    {
        const PropertyValue* value = valueOf(point, key);
        return value ? std::optional<std::string>(katana::entity::toString(*value)) : std::nullopt;
    }

    // A real, a whole number, or text that reads as a number.
    [[nodiscard]] Result<std::optional<double>> number(std::string_view key) const
    {
        const PropertyValue* value = valueOf(point, key);
        if (value == nullptr) {
            return std::optional<double>{};
        }
        std::optional<double> number;
        if (const auto* real = std::get_if<double>(value)) {
            number = *real;
        } else if (const auto* whole = std::get_if<std::int64_t>(value)) {
            number = static_cast<double>(*whole);
        } else if (const auto* words = std::get_if<std::string>(value)) {
            number = katana::core::parseFiniteDouble(katana::core::trimmed(*words));
        }
        if (!number || !std::isfinite(*number)) {
            return refuse("has " + std::string(key) + " \"" + katana::entity::toString(*value) +
                          "\", which is not a number");
        }
        return number;
    }

    [[nodiscard]] Result<bool> flag(std::string_view key) const
    {
        const PropertyValue* value = valueOf(point, key);
        if (value == nullptr) {
            return false;
        }
        if (const auto* on = std::get_if<bool>(value)) {
            return *on;
        }
        const std::string word = katana::core::lowered(katana::entity::toString(*value));
        if (word == "true" || word == "yes" || word == "1") {
            return true;
        }
        if (word == "false" || word == "no" || word == "0") {
            return false;
        }
        return refuse("has " + std::string(key) + " \"" + word + "\", which is not true or false");
    }
};

// The line's attributes as one point carries them.
PropertyMap lineAttributesOf(const Entity& point)
{
    PropertyMap attributes;
    for (const std::string_view key : kLineKeys) {
        if (const PropertyValue* value = valueOf(point, key)) {
            attributes.emplace(std::string(key), *value);
        }
    }
    for (const auto& [key, value] : point.properties) {
        if ((key.starts_with(keys::kFieldPrefix) &&
             isLineField(std::string_view(key).substr(keys::kFieldPrefix.size()))) ||
            (key.starts_with(keys::kRecordedPrefix) &&
             isLineRecorded(std::string_view(key).substr(keys::kRecordedPrefix.size())))) {
            attributes.emplace(key, value);
        }
    }
    return attributes;
}

// A point that is a vertex of a schedule - it has a point id or a place
// along a line - read with no utility.line, or an empty one.
bool isStrayVertex(const Entity& entity)
{
    return entity.type() == EntityType::Point &&
           (valueOf(entity, keys::kVertex) != nullptr || valueOf(entity, keys::kOrder) != nullptr);
}

// Refuses a stray vertex by its point id and entity id, and names the lines
// it is most likely a point of.
Error strayRefusal(const Entity& point, const std::vector<std::string>& lines)
{
    const PropertyValue* vertex = valueOf(point, keys::kVertex);
    std::string text = "point " +
                       (vertex ? katana::entity::toString(*vertex) + " " : std::string()) + "(#" +
                       std::to_string(point.id) + ") is a vertex of a schedule and has no " +
                       std::string(keys::kLine) + ", the line it is a point of";
    if (!lines.empty()) {
        std::string names;
        for (const std::string& line : lines) {
            names += (names.empty() ? "" : " or ") + line;
        }
        text += "; by its attributes it is a point of line " + names;
    }
    return makeError(ErrorCode::InvalidArgument,
                     text + "; give it its line again, or delete its " +
                         std::string(keys::kVertex) + " and " + std::string(keys::kOrder));
}

// Every point must say what the first says of its line.
katana::core::Status agree(const PointReader& first, const PropertyMap& expected,
                           const PointReader& other, const PropertyMap& actual)
{
    const auto quoted = [](const PropertyValue& value) {
        return "\"" + katana::entity::toString(value) + "\"";
    };
    for (const auto& [key, value] : expected) {
        const auto found = actual.find(key);
        if (found == actual.end()) {
            return other.refuse("has no " + key + " where " + first.name() + " has " +
                                quoted(value) + "; the points of one line must agree");
        }
        if (found->second != value) {
            return other.refuse("has " + key + " " + quoted(found->second) + " where " +
                                first.name() + " has " + quoted(value) +
                                "; the points of one line must agree");
        }
    }
    for (const auto& [key, value] : actual) {
        if (!expected.contains(key)) {
            return other.refuse("has " + key + " " + quoted(value) + " where " + first.name() +
                                " has none; the points of one line must agree");
        }
    }
    return {};
}

Result<sub::UtilityAttributes> readAttributes(const PointReader& reader)
{
    sub::UtilityAttributes attributes;
    const auto type = reader.text(keys::kType);
    if (!type) {
        return reader.refuse("has no " + std::string(keys::kType));
    }
    const auto parsedType = sub::parseUtilityType(*type);
    if (!parsedType) {
        return reader.refuse("has " + std::string(keys::kType) + " \"" + *type +
                             "\", which is not a utility type");
    }
    attributes.type = *parsedType;
    if (const auto status = reader.text(keys::kStatus)) {
        const auto parsed = sub::parseUtilityStatus(*status);
        if (!parsed) {
            return reader.refuse("has " + std::string(keys::kStatus) + " \"" + *status +
                                 "\", which is not a utility status");
        }
        attributes.status = *parsed;
    }
    attributes.owner = reader.text(keys::kOwner).value_or("");
    attributes.material = reader.text(keys::kMaterial).value_or("");
    attributes.configuration = reader.text(keys::kConfiguration).value_or("");
    attributes.description = reader.text(keys::kDescription).value_or("");
    auto diameter = reader.number(keys::kDiameter);
    if (!diameter) {
        return diameter.error();
    }
    if (*diameter) {
        if (!(**diameter > 0.0)) {
            return reader.refuse("has a " + std::string(keys::kDiameter) +
                                 " that is not a positive number of metres");
        }
        attributes.diameter = **diameter;
        auto inside = reader.flag(keys::kDiameterInside);
        if (!inside) {
            return inside.error();
        }
        attributes.diameterIsInside = *inside;
    }
    for (const auto& [key, value] : reader.point.properties) {
        const std::string_view name = std::string_view(key).substr(
            key.starts_with(keys::kFieldPrefix) ? keys::kFieldPrefix.size() : 0);
        if (key.starts_with(keys::kFieldPrefix) && isLineField(name)) {
            attributes.fields.emplace(std::string(name), katana::entity::toString(value));
        }
    }
    attributes.recorded = recordedOf(reader.point, true);
    return attributes;
}

Result<sub::UtilityVertex> readVertex(const PointReader& reader)
{
    sub::UtilityVertex vertex;
    const auto id = reader.text(keys::kVertex);
    if (!id || id->empty()) {
        return reader.refuse("has no " + std::string(keys::kVertex) + ", its point id");
    }
    vertex.id = *id;
    const auto& position =
        std::get<katana::entity::PointGeometry>(reader.point.geometry).position;
    vertex.position = {position.y, position.x};

    const auto method = reader.text(keys::kMethod);
    if (!method) {
        return reader.refuse("has no " + std::string(keys::kMethod) +
                             ", the method it was located by");
    }
    // toString's "unknown method" is what DRAW writes for a method nobody
    // recorded; the schedule reader's word for it is "unknown".
    const auto parsedMethod = *method == sub::toString(sub::LocationMethod::Unknown)
                                  ? std::optional(sub::LocationMethod::Unknown)
                                  : sub::parseLocationMethod(*method);
    if (!parsedMethod) {
        return reader.refuse("has " + std::string(keys::kMethod) + " \"" + *method +
                             "\", which is not a location method");
    }
    vertex.evidence.method = *parsedMethod;

    for (const auto& [key, out] :
         {std::pair{keys::kLevel, &vertex.level}, std::pair{keys::kDepth, &vertex.depth},
          std::pair{keys::kSurfaceLevel, &vertex.surfaceLevel},
          std::pair{keys::kHorizontalUncertainty, &vertex.evidence.horizontalUncertainty},
          std::pair{keys::kVerticalUncertainty, &vertex.evidence.verticalUncertainty}}) {
        auto number = reader.number(key);
        if (!number) {
            return number.error();
        }
        *out = *number;
    }
    if (vertex.depth && *vertex.depth < 0.0) {
        return reader.refuse("has a " + std::string(keys::kDepth) +
                             " above the surface; a depth is below it");
    }
    if (const auto reference = reader.text(keys::kLevelReference)) {
        const auto parsed = sub::parseLevelReference(*reference);
        if (!parsed) {
            return reader.refuse("has " + std::string(keys::kLevelReference) + " \"" + *reference +
                                 "\", which is not a level reference");
        }
        vertex.levelReference = *parsed;
    }
    if (const auto claimed = reader.text(keys::kClaimed)) {
        const auto parsed = sub::parseQualityLevel(*claimed);
        if (!parsed) {
            return reader.refuse("has " + std::string(keys::kClaimed) + " \"" + *claimed +
                                 "\", which is not a quality level");
        }
        vertex.claimed = *parsed;
    }
    vertex.verifies = reader.text(keys::kVerifies).value_or("");
    for (const auto& [key, value] : reader.point.properties) {
        if (!key.starts_with(keys::kFieldPrefix)) {
            continue;
        }
        const std::string_view name = std::string_view(key).substr(keys::kFieldPrefix.size());
        if (!isLineField(name)) {
            vertex.fields.emplace(std::string(name), katana::entity::toString(value));
        }
    }
    vertex.recorded = recordedOf(reader.point, false);
    vertex.evidence.hasLevel = sub::hasVerticalMeasurement(vertex);
    return vertex;
}

Result<DrawnService> readService(const std::string& id, const std::vector<const Entity*>& points)
{
    // Their places along the line; a tie is broken by id only to name both.
    std::vector<std::pair<double, const Entity*>> places;
    for (const Entity* point : points) {
        const PointReader reader{id, *point};
        auto order = reader.number(keys::kOrder);
        if (!order) {
            return order.error();
        }
        if (!*order) {
            return reader.refuse("has no " + std::string(keys::kOrder) +
                                 ", its place along the line");
        }
        places.emplace_back(**order, point);
    }
    std::ranges::sort(places, [](const auto& a, const auto& b) {
        return a.first != b.first ? a.first < b.first : a.second->id < b.second->id;
    });

    DrawnService service;
    service.line.id = id;
    const PointReader first{id, *places.front().second};
    const PropertyMap expected = lineAttributesOf(*places.front().second);
    auto attributes = readAttributes(first);
    if (!attributes) {
        return attributes.error();
    }
    service.line.attributes = std::move(attributes).value();
    for (const auto& [key, out] : {std::pair{keys::kSpacing, &service.spacing},
                                   std::pair{keys::kMinimumCover, &service.minimumCover}}) {
        auto metres = first.number(key);
        if (!metres) {
            return metres.error();
        }
        if (*metres && !(**metres >= 0.0)) {
            return first.refuse("has a " + std::string(key) +
                                " that is not a number of metres, not negative");
        }
        *out = *metres;
    }

    std::vector<std::optional<std::string>> paths;
    std::set<std::string, std::less<>> vertexIds;
    for (std::size_t i = 0; i < places.size(); ++i) {
        const PointReader reader{id, *places[i].second};
        if (i > 0 && places[i].first == places[i - 1].first) {
            return makeError(ErrorCode::InvalidArgument,
                             "line " + id + " has two points at " + std::string(keys::kOrder) +
                                 " " + katana::core::formatExactReal(places[i].first) + ": #" +
                                 std::to_string(places[i - 1].second->id) + " and #" +
                                 std::to_string(places[i].second->id));
        }
        if (i > 0) {
            if (auto status = agree(first, expected, reader, lineAttributesOf(reader.point));
                !status) {
                return status.error();
            }
        }
        auto vertex = readVertex(reader);
        if (!vertex) {
            return vertex.error();
        }
        if (!vertexIds.insert(vertex->id).second) {
            return reader.refuse("has the point id another point of the line has");
        }
        paths.push_back(reader.text(keys::kPath));
        service.line.vertices.push_back(std::move(vertex).value());
        service.points.push_back(reader.point.id);
    }
    // A path is to the next point, so the last point's says nothing. Any
    // other point giving one makes the list whole, the rest detected - as a
    // schedule reads its path column.
    const bool anyPath = std::any_of(paths.begin(), paths.end() - 1,
                                     [](const auto& path) { return path.has_value(); });
    if (anyPath) {
        for (std::size_t i = 0; i + 1 < paths.size(); ++i) {
            sub::PathEvidence evidence = sub::PathEvidence::Detected;
            if (paths[i]) {
                const auto parsed = sub::parsePathEvidence(*paths[i]);
                if (!parsed) {
                    return PointReader{id, *places[i].second}.refuse(
                        "has " + std::string(keys::kPath) + " \"" + *paths[i] +
                        "\", which is not detected, exposed or assumed");
                }
                evidence = *parsed;
            }
            service.line.pathEvidence.push_back(evidence);
        }
    }
    return service;
}

// "<prefix>/<word>/<leaf>" -> prefix, when the layer has that shape.
std::optional<std::string> prefixUnder(std::string_view layer, bool (*leafIs)(std::string_view))
{
    if (katana::entity::layerDepth(layer) < 3 || !leafIs(katana::entity::layerLeaf(layer))) {
        return std::nullopt;
    }
    return std::string(katana::entity::layerParent(katana::entity::layerParent(layer)));
}

bool isPointsLeaf(std::string_view leaf)
{
    return leaf == "points";
}

bool isQualityLeaf(std::string_view leaf)
{
    return std::ranges::any_of(
        std::array{sub::QualityLevel::A, sub::QualityLevel::B, sub::QualityLevel::C,
                   sub::QualityLevel::D},
        [leaf](sub::QualityLevel level) { return leaf == sub::toString(level); });
}

std::string prefixOf(const Model& model, const DrawnService& service, std::string_view fallback)
{
    for (const EntityId id : service.points) {
        if (const auto prefix = prefixUnder(model.entities.find(id)->layer, isPointsLeaf)) {
            return *prefix;
        }
    }
    for (const EntityId id : service.runs) {
        if (const auto prefix = prefixUnder(model.entities.find(id)->layer, isQualityLeaf)) {
            return *prefix;
        }
    }
    return std::string(fallback);
}

} // namespace

std::vector<sub::UtilityLine> UtilityData::lines() const
{
    std::vector<sub::UtilityLine> out;
    out.reserve(services.size());
    for (const DrawnService& service : services) {
        out.push_back(service.line);
    }
    return out;
}

Result<UtilityData> readUtilityData(const Model& model, std::span<const EntityId> matched)
{
    // Every point and run of every line in the drawing, by line id: a line is
    // read whole, whatever part of it the scope took.
    struct Members {
        std::vector<const Entity*> points;
        std::vector<EntityId> runs;
    };
    std::map<std::string, Members, std::less<>> byLine;
    // Points that are a vertex of a schedule - they carry a point id or a
    // place along a line - but say no line: a utility.line deleted or
    // emptied by hand. Never ignored, or a line would be read short of them.
    std::vector<const Entity*> strays;
    model.entities.forEach([&byLine, &strays](const Entity& entity) {
        const PropertyValue* line = valueOf(entity, keys::kLine);
        if (line == nullptr || katana::entity::toString(*line).empty()) {
            if (isStrayVertex(entity)) {
                strays.push_back(&entity);
            }
            return;
        }
        Members& members = byLine[katana::entity::toString(*line)];
        if (entity.type() == EntityType::Point) {
            members.points.push_back(&entity);
        } else if (entity.type() == EntityType::Polyline ||
                   entity.type() == EntityType::CurvePolyline) {
            // A run filleted or given an arc is stored as a curve polyline
            // (docs/drawing.md) and is still the line's run: left out, a
            // regrade would draw the run again beside it.
            members.runs.push_back(entity.id);
        }
    });
    // The lines a stray point is most likely one of: those whose points
    // carry the line attributes it carries.
    const auto linesOf = [&byLine](const Entity& stray) {
        const PropertyMap attributes = lineAttributesOf(stray);
        std::vector<std::string> lines;
        for (const auto& [name, members] : byLine) {
            if (std::ranges::any_of(members.points, [&attributes](const Entity* point) {
                    return lineAttributesOf(*point) == attributes;
                })) {
                lines.push_back(name);
            }
        }
        return lines;
    };

    UtilityData data;
    std::map<std::string, std::size_t, std::less<>> pointsTaken;
    std::set<std::string, std::less<>> wanted;
    for (const EntityId id : matched) {
        const Entity* entity = model.entities.find(id);
        if (entity == nullptr) {
            continue;
        }
        if (std::ranges::find(strays, entity) != strays.end()) {
            return strayRefusal(*entity, linesOf(*entity));
        }
        const PropertyValue* line = valueOf(*entity, keys::kLine);
        if (line == nullptr || katana::entity::toString(*line).empty()) {
            ++data.ignored;
            continue;
        }
        const std::string name = katana::entity::toString(*line);
        wanted.insert(name);
        if (entity->type() == EntityType::Point) {
            ++pointsTaken[name];
        }
    }
    // A stray outside the scope still refuses a line it may be a point of:
    // read without it, that line would be graded, reported or written short.
    for (const Entity* stray : strays) {
        const std::vector<std::string> lines = linesOf(*stray);
        if (std::ranges::any_of(lines, [&wanted](const std::string& name) {
                return wanted.contains(name);
            })) {
            return strayRefusal(*stray, lines);
        }
    }

    std::vector<std::pair<EntityId, std::string>> drawnOrder;
    for (const std::string& name : wanted) {
        const Members& members = byLine[name];
        if (members.points.empty()) {
            return makeError(ErrorCode::InvalidArgument,
                             "line " + name +
                                 " has no points left to read it from: its points are the "
                                 "schedule, its runs only draw it");
        }
        EntityId first = std::numeric_limits<EntityId>::max();
        for (const Entity* point : members.points) {
            first = std::min(first, point->id);
        }
        drawnOrder.emplace_back(first, name);
    }
    std::ranges::sort(drawnOrder);
    for (const auto& [first, name] : drawnOrder) {
        const Members& members = byLine[name];
        auto service = readService(name, members.points);
        if (!service) {
            return service.error();
        }
        service->runs = members.runs;
        std::ranges::sort(service->runs);
        service->completed = pointsTaken[name] < service->points.size();
        data.completed += service->completed ? 1 : 0;
        data.services.push_back(std::move(service).value());
    }
    return data;
}

std::string utilityDataKeys(const UtilityData& data)
{
    return " lines=" + std::to_string(data.services.size()) +
           " completed=" + std::to_string(data.completed) +
           " ignored=" + std::to_string(data.ignored);
}

// ---- services from surveyed or imported geometry -----------------------------------------

std::string_view geometryHeightsWord(GeometryHeights heights)
{
    switch (heights) {
    case GeometryHeights::Surface:
        return "surface";
    case GeometryHeights::Service:
        return "service";
    case GeometryHeights::Unused:
        return "none";
    }
    return "surface";
}

std::optional<GeometryHeights> parseGeometryHeights(std::string_view text)
{
    for (const GeometryHeights heights :
         {GeometryHeights::Surface, GeometryHeights::Service, GeometryHeights::Unused}) {
        if (katana::core::equalsIgnoringCase(katana::core::trimmed(text),
                                             geometryHeightsWord(heights))) {
            return heights;
        }
    }
    return std::nullopt;
}

namespace {

// What a line entity may say of the service it is.
constexpr std::array kServiceKeys{keys::kType,        keys::kOwner,          keys::kMaterial,
                                  keys::kDiameter,    keys::kDiameterInside, keys::kConfiguration,
                                  keys::kDescription, keys::kStatus};
// What it may say of every vertex of it.
constexpr std::array kLineVertexKeys{keys::kMethod,
                                     keys::kHorizontalUncertainty,
                                     keys::kVerticalUncertainty,
                                     keys::kLevelReference,
                                     keys::kClaimed,
                                     keys::kPath,
                                     keys::kDepth};
// What a point on a vertex may say of that vertex: its whole row but the
// line's attributes, which are the line's to say.
constexpr std::array kPointVertexKeys{keys::kMethod,
                                      keys::kLevel,
                                      keys::kDepth,
                                      keys::kSurfaceLevel,
                                      keys::kLevelReference,
                                      keys::kHorizontalUncertainty,
                                      keys::kVerticalUncertainty,
                                      keys::kClaimed,
                                      keys::kVerifies,
                                      keys::kPath};

// The key a schedule column is drawn as (utility_drawing.hpp, keys), for a
// field FIELDS reads as that column. Empty for "line" and "point", which are
// the ids, not properties.
struct FieldColumn {
    std::string column; // the schedule's name for it
    std::string key;
    std::string property; // the entity's property it is read from
};

Result<std::vector<FieldColumn>>
fieldColumns(const std::vector<std::pair<std::string, std::string>>& fields)
{
    static const std::map<std::string_view, std::string_view> kKeyOf{
        {"line", ""},
        {"point", ""},
        {"method", keys::kMethod},
        {"level", keys::kLevel},
        {"level_ref", keys::kLevelReference},
        {"depth", keys::kDepth},
        {"surface", keys::kSurfaceLevel},
        {"h_unc", keys::kHorizontalUncertainty},
        {"v_unc", keys::kVerticalUncertainty},
        {"ql", keys::kClaimed},
        {"path", keys::kPath},
        {"verifies", keys::kVerifies},
        {"type", keys::kType},
        {"owner", keys::kOwner},
        {"material", keys::kMaterial},
        {"diameter_mm", keys::kDiameter},
        {"status", keys::kStatus},
        {"config", keys::kConfiguration},
        {"description", keys::kDescription}};
    std::vector<FieldColumn> out;
    for (const auto& [name, property] : fields) {
        const sub::UtilityCsvColumn* column = sub::utilityCsvColumnNamed(name);
        if (column == nullptr) {
            return makeError(ErrorCode::InvalidArgument,
                             "FIELDS: " + name + " is not a column of a utility schedule");
        }
        std::string key;
        if (column->carried != sub::CarriedOn::Interpreted) {
            key = std::string(keys::kFieldPrefix) + std::string(column->name);
        } else if (const auto found = kKeyOf.find(column->name); found != kKeyOf.end()) {
            key = std::string(found->second);
        } else {
            return makeError(ErrorCode::InvalidArgument,
                             "FIELDS: " + std::string(column->name) +
                                 (column->name == "size"
                                      ? " is read from a schedule only; give the diameter as "
                                        "diameter_mm"
                                      : " is where the geometry is, not a field"));
        }
        if (property.empty()) {
            return makeError(ErrorCode::InvalidArgument,
                             "FIELDS: " + std::string(column->name) + " names no property");
        }
        if (std::ranges::any_of(
                out, [column](const FieldColumn& taken) { return taken.column == column->name; })) {
            return makeError(ErrorCode::InvalidArgument,
                             "FIELDS: " + std::string(column->name) + " given twice");
        }
        out.push_back({std::string(column->name), std::move(key), property});
    }
    return out;
}

// `entity` as the reader reads it: its fields under the keys of their
// columns, where it has no utility.* property of its own there, and its ids
// in `line` and `point` when a field names them.
struct EntityView {
    Entity entity;
    std::string line;
    std::string point;
};

EntityView viewOf(const Entity& entity, const std::vector<FieldColumn>& fields)
{
    EntityView view{entity, {}, {}};
    for (const FieldColumn& field : fields) {
        const PropertyValue* value = valueOf(entity, field.property);
        if (value == nullptr) {
            continue;
        }
        const std::string text(katana::core::trimmed(katana::entity::toString(*value)));
        if (field.column == "line" || field.column == "point") {
            (field.column == "line" ? view.line : view.point) = text;
            continue;
        }
        if (text.empty() || view.entity.properties.contains(field.key)) {
            continue;
        }
        PropertyValue read = *value;
        // A schedule's diameter is millimetres; the drawing's is metres.
        if (field.column == "diameter_mm") {
            if (const auto millimetres = katana::core::parseFiniteDouble(text)) {
                read = *millimetres / 1000.0;
            }
        }
        view.entity.properties.insert_or_assign(field.key, std::move(read));
    }
    return view;
}

bool hasLine(const Entity& entity)
{
    const PropertyValue* line = valueOf(entity, keys::kLine);
    return line != nullptr && !katana::entity::toString(*line).empty();
}

// The entity's name: its code, as survey coding finds one, a coded survey's
// string name or a .12da archive string's. Empty when it has none.
std::string nameOf(const Entity& entity)
{
    for (const std::string& candidate : codePropertyCandidates()) {
        if (const std::string* code = surveyCodeOf(entity, candidate)) {
            const std::string_view name = katana::core::trimmed(*code);
            if (!name.empty()) {
                return std::string(name);
            }
        }
    }
    return {};
}

// "polyline (#12)", "line (#7)": the entity a service is read from, as a
// refusal names it.
std::string sourceLabel(const Entity& entity)
{
    return katana::core::lowered(katana::entity::toString(entity.type())) + " (#" +
           std::to_string(entity.id) + ")";
}

// The vertices of a line entity that can be a service: a line, or an open
// polyline. Empty for anything else.
std::vector<katana::geometry::Point2> runVertices(const Entity& entity)
{
    if (const auto* segment = std::get_if<katana::geometry::Segment2>(&entity.geometry)) {
        return {segment->start, segment->end};
    }
    if (const auto* polyline = std::get_if<katana::geometry::Polyline2>(&entity.geometry);
        polyline != nullptr && !polyline->closed) {
        return polyline->vertices;
    }
    return {};
}

void copyKeys(const Entity& from, std::span<const std::string_view> keysToCopy, PropertyMap& into)
{
    for (const std::string_view key : keysToCopy) {
        if (const PropertyValue* value = valueOf(from, key)) {
            into.insert_or_assign(std::string(key), *value);
        }
    }
}

// A delivery schema's fields on `from`: the line's (`line`) or a vertex's.
void copyFields(const Entity& from, bool line, PropertyMap& into)
{
    for (const auto& [key, value] : from.properties) {
        if (key.starts_with(keys::kFieldPrefix) &&
            isLineField(std::string_view(key).substr(keys::kFieldPrefix.size())) == line) {
            into.insert_or_assign(key, value);
        }
    }
}

// What the verb's options say, as the properties a drawn point would carry.
PropertyMap serviceDefaults(const GeometryServiceOptions& options)
{
    PropertyMap properties;
    const auto text = [&properties](std::string_view key, std::string value) {
        if (!value.empty()) {
            properties.insert_or_assign(std::string(key), std::move(value));
        }
    };
    if (options.type) {
        text(keys::kType, std::string(utilityTypeWord(*options.type)));
    }
    text(keys::kOwner, options.owner);
    text(keys::kMaterial, options.material);
    if (options.diameter) {
        properties.insert_or_assign(std::string(keys::kDiameter), *options.diameter);
    }
    if (options.status) {
        text(keys::kStatus, sub::toString(*options.status));
    }
    return properties;
}

PropertyMap vertexDefaults(const GeometryServiceOptions& options)
{
    PropertyMap properties;
    if (options.method) {
        properties.insert_or_assign(std::string(keys::kMethod),
                                    std::string(sub::toString(*options.method)));
    }
    if (options.horizontalUncertainty) {
        properties.insert_or_assign(std::string(keys::kHorizontalUncertainty),
                                    *options.horizontalUncertainty);
    }
    if (options.verticalUncertainty) {
        properties.insert_or_assign(std::string(keys::kVerticalUncertainty),
                                    *options.verticalUncertainty);
    }
    if (options.levelReference) {
        properties.insert_or_assign(std::string(keys::kLevelReference),
                                    std::string(sub::toString(*options.levelReference)));
    }
    if (options.path) {
        properties.insert_or_assign(std::string(keys::kPath),
                                    std::string(sub::toString(*options.path)));
    }
    return properties;
}

// What an entity's keys::kHeights says, refused by `reader` when it says
// something else.
Result<std::optional<GeometryHeights>> heightsSaidBy(const PointReader& reader)
{
    const auto word = reader.text(keys::kHeights);
    if (!word) {
        return std::optional<GeometryHeights>{};
    }
    const auto heights = parseGeometryHeights(*word);
    if (!heights) {
        return reader.refuse("has " + std::string(keys::kHeights) + " \"" + *word +
                             "\", which is not surface, service or none");
    }
    return std::optional(*heights);
}

// What the line entity says for the service and each of its vertices, read
// as a drawn point's would be so that a value that does not read is refused
// by the entity it is on, before a point of the service names it.
katana::core::Status checkLineSays(const std::string& line, const Entity& source,
                                   const PropertyMap& service, const PropertyMap& vertex)
{
    Entity probe;
    probe.id = source.id;
    probe.geometry = katana::entity::PointGeometry{};
    probe.properties = service;
    for (const auto& [key, value] : vertex) {
        probe.properties.insert_or_assign(key, value);
    }
    // What only a vertex has, so that readVertex reads the rest.
    probe.properties.insert_or_assign(std::string(keys::kVertex), std::string("-"));
    probe.properties.try_emplace(std::string(keys::kMethod),
                                 std::string(sub::toString(sub::LocationMethod::Records)));
    const PointReader reader{line, probe, sourceLabel(source)};
    if (auto attributes = readAttributes(reader); !attributes) {
        return attributes.error();
    }
    if (auto read = readVertex(reader); !read) {
        return read.error();
    }
    if (const auto path = reader.text(keys::kPath); path && !sub::parsePathEvidence(*path)) {
        return reader.refuse("has " + std::string(keys::kPath) + " \"" + *path +
                             "\", which is not detected, exposed or assumed");
    }
    return {};
}

} // namespace

Result<GeometryServices> readGeometryServices(const Model& model, std::span<const EntityId> matched,
                                              const GeometryServiceOptions& options)
{
    // Every line drawn, and every entity a line was drawn from.
    std::set<std::string, std::less<>> linesDrawn;
    std::set<std::string, std::less<>> sourcesDrawn;
    model.entities.forEach([&linesDrawn, &sourcesDrawn](const Entity& entity) {
        if (!hasLine(entity)) {
            return;
        }
        linesDrawn.insert(katana::entity::toString(*valueOf(entity, keys::kLine)));
        if (const PropertyValue* source = valueOf(entity, keys::kSource)) {
            sourcesDrawn.insert(katana::entity::toString(*source));
        }
    });

    const auto fields = fieldColumns(options.fields);
    if (!fields) {
        return fields.error();
    }
    GeometryServices out;
    // Each entity taken, as read through the fields: an id's view stays where
    // it is, since the lists below point into it.
    std::map<EntityId, EntityView> views;
    std::vector<const EntityView*> runs;   // to be drawn
    std::vector<const Entity*> before;     // drawn from before: their points are not loose
    std::vector<const EntityView*> points; // that may be on a vertex
    for (const EntityId id : matched) {
        const Entity* entity = model.entities.find(id);
        if (entity == nullptr || views.contains(id)) {
            continue;
        }
        if (hasLine(*entity)) {
            ++out.drawn;
            continue;
        }
        if (isStrayVertex(*entity)) {
            return strayRefusal(*entity, {});
        }
        if (entity->type() == EntityType::Point) {
            points.push_back(&views.emplace(id, viewOf(*entity, *fields)).first->second);
        } else if (runVertices(*entity).size() < 2) {
            ++out.ignored;
        } else if (sourcesDrawn.contains("#" + std::to_string(id))) {
            ++out.drawn;
            before.push_back(entity);
        } else {
            runs.push_back(&views.emplace(id, viewOf(*entity, *fields)).first->second);
        }
    }
    std::ranges::sort(runs, {}, [](const EntityView* view) { return view->entity.id; });

    // The points on a place, for each vertex: sorted by x, a window each.
    const auto at = [](const EntityView* point) {
        return std::get<katana::entity::PointGeometry>(point->entity.geometry).position;
    };
    std::ranges::sort(points, [&at](const EntityView* a, const EntityView* b) {
        return at(a).x != at(b).x ? at(a).x < at(b).x : a->entity.id < b->entity.id;
    });
    constexpr double kSameMark = katana::math::tolerance::kCoordinate;
    const auto pointsOn = [&points, &at](katana::geometry::Point2 place) {
        std::vector<const EntityView*> on;
        auto first = std::ranges::lower_bound(points, place.x - kSameMark, {},
                                              [&at](const EntityView* p) { return at(p).x; });
        for (; first != points.end() && at(*first).x <= place.x + kSameMark; ++first) {
            if (katana::math::distance(at(*first), place) < kSameMark) {
                on.push_back(*first);
            }
        }
        return on;
    };
    // A run's name: the field that names it, else its code.
    const auto nameOfRun = [](const EntityView* run) {
        return run->line.empty() ? nameOf(run->entity) : run->line;
    };

    // Each run's id: its name, told apart from another of the same name.
    std::map<std::string, std::size_t, std::less<>> named;
    for (const EntityView* view : runs) {
        if (const std::string name = nameOfRun(view); !name.empty()) {
            ++named[name];
        }
    }
    std::set<std::string, std::less<>> ids;
    std::set<EntityId> used;
    for (const EntityView* view : runs) {
        const Entity* run = &view->entity;
        const std::string name = nameOfRun(view);
        const std::string own = "#" + std::to_string(run->id);
        const std::string id = name.empty()                                   ? own
                               : named[name] > 1 || linesDrawn.contains(name) ? name + own
                                                                              : name;
        if (linesDrawn.contains(id) || !ids.insert(id).second) {
            return makeError(ErrorCode::InvalidArgument,
                             "the service read from " + sourceLabel(*run) + " would be line " + id +
                                 ", which is " +
                                 (linesDrawn.contains(id) ? "drawn already" : "another's too") +
                                 "; name it apart (its code)");
        }

        PropertyMap service = serviceDefaults(options);
        copyKeys(*run, kServiceKeys, service);
        copyFields(*run, true, service);
        service.try_emplace(std::string(keys::kType),
                            std::string(utilityTypeWord(sub::UtilityType::Unknown)));
        PropertyMap lineVertex = vertexDefaults(options);
        copyKeys(*run, kLineVertexKeys, lineVertex);
        if (auto status = checkLineSays(id, *run, service, lineVertex); !status) {
            return status.error();
        }
        auto lineHeights = heightsSaidBy(PointReader{id, *run, sourceLabel(*run)});
        if (!lineHeights) {
            return lineHeights.error();
        }

        const std::vector<katana::geometry::Point2> vertices = runVertices(*run);
        const std::vector<std::optional<double>> heights =
            katana::entity::heightsOf(run->properties, vertices.size());
        std::vector<Entity> row(vertices.size());
        for (std::size_t i = 0; i < vertices.size(); ++i) {
            const std::vector<const EntityView*> on = pointsOn(vertices[i]);
            if (on.size() > 1) {
                return makeError(ErrorCode::InvalidArgument,
                                 "line " + id + ": vertex " + std::to_string(i + 1) + " of " +
                                     sourceLabel(*run) + " has two points on it, #" +
                                     std::to_string(on[0]->entity.id) + " and #" +
                                     std::to_string(on[1]->entity.id) +
                                     "; take the one that is not the survey's out of the scope");
            }
            const EntityView* onVertex = on.empty() ? nullptr : on.front();
            const Entity* point = onVertex != nullptr ? &onVertex->entity : nullptr;
            Entity& vertex = row[i];
            vertex.id = point != nullptr ? point->id : run->id;
            vertex.geometry = katana::entity::PointGeometry{vertices[i]};
            vertex.properties = service;
            for (const auto& [key, value] : lineVertex) {
                vertex.properties.insert_or_assign(key, value);
            }

            // The height, and what it is the height of.
            std::optional<double> height = heights[i];
            std::optional<GeometryHeights> meaning = *lineHeights;
            if (point != nullptr) {
                if (!height) {
                    height = katana::entity::heightsOf(point->properties, 1).front();
                }
                auto pointHeights = heightsSaidBy(PointReader{id, *point});
                if (!pointHeights) {
                    return pointHeights.error();
                }
                meaning = pointHeights->has_value() ? *pointHeights : meaning;
            }
            if (height) {
                switch (meaning.value_or(options.heights)) {
                case GeometryHeights::Surface:
                    vertex.properties.insert_or_assign(std::string(keys::kSurfaceLevel), *height);
                    break;
                case GeometryHeights::Service:
                    vertex.properties.insert_or_assign(std::string(keys::kLevel), *height);
                    break;
                case GeometryHeights::Unused:
                    break;
                }
            }

            std::string vertexId = id + "-" + std::to_string(i + 1);
            if (point != nullptr) {
                copyKeys(*point, kPointVertexKeys, vertex.properties);
                copyFields(*point, false, vertex.properties);
                if (!onVertex->point.empty()) {
                    vertexId = onVertex->point;
                } else if (const PropertyValue* number =
                               valueOf(*point, SurveyImportOptions{}.pointNumberProperty)) {
                    if (const std::string text = katana::entity::toString(*number);
                        !katana::core::trimmed(text).empty()) {
                        vertexId = std::string(katana::core::trimmed(text));
                    }
                }
                used.insert(point->id);
            }
            vertex.properties.insert_or_assign(std::string(keys::kLine), id);
            vertex.properties.insert_or_assign(std::string(keys::kVertex), vertexId);
            vertex.properties.insert_or_assign(std::string(keys::kOrder),
                                               static_cast<std::int64_t>(i + 1));
            if (!vertex.properties.contains(keys::kMethod)) {
                return makeError(ErrorCode::InvalidArgument,
                                 "line " + id + ": vertex " + vertexId +
                                     (point != nullptr ? " (#" + std::to_string(point->id) + ")"
                                                       : std::string()) +
                                     " says no method it was located by; give METHOD, or " +
                                     std::string(keys::kMethod) + " on its point or on " +
                                     sourceLabel(*run));
            }
        }
        std::vector<const Entity*> members;
        for (const Entity& vertex : row) {
            members.push_back(&vertex);
        }
        auto read = readService(id, members);
        if (!read) {
            return read.error();
        }
        out.lines.push_back(std::move(read->line));
        out.sources.push_back(run->id);
    }

    // A point of a service drawn before is that service's, not loose.
    std::set<EntityId> drawnPoints;
    for (const Entity* run : before) {
        for (const katana::geometry::Point2 vertex : runVertices(*run)) {
            for (const EntityView* point : pointsOn(vertex)) {
                if (!used.contains(point->entity.id)) {
                    drawnPoints.insert(point->entity.id);
                }
            }
        }
    }
    out.points = used.size();
    out.drawn += drawnPoints.size();
    out.loose = points.size() - used.size() - drawnPoints.size();
    return out;
}

std::string geometryServiceKeys(const GeometryServices& services)
{
    return " lines=" + std::to_string(services.lines.size()) +
           " points=" + std::to_string(services.points) +
           " loose=" + std::to_string(services.loose) + " drawn=" + std::to_string(services.drawn) +
           " ignored=" + std::to_string(services.ignored);
}

Result<UtilityDrawing> drawGeometryServices(const GeometryServices& services,
                                            const UtilityDrawOptions& options)
{
    auto drawing = drawUtilities(services.lines, options);
    if (!drawing) {
        return drawing.error();
    }
    std::map<std::string, std::string, std::less<>> sourceOf;
    for (std::size_t i = 0; i < services.lines.size(); ++i) {
        sourceOf.emplace(services.lines[i].id, "#" + std::to_string(services.sources[i]));
    }
    for (Entity& entity : drawing->entities) {
        const PropertyValue* line = valueOf(entity, keys::kLine);
        if (entity.type() != EntityType::Point || line == nullptr) {
            continue;
        }
        if (const auto found = sourceOf.find(katana::entity::toString(*line));
            found != sourceOf.end()) {
            entity.properties.insert_or_assign(std::string(keys::kSource), found->second);
        }
    }
    return drawing;
}

Result<UtilityRegrade> planUtilityRegrade(const Model& model, const UtilityData& data,
                                          const UtilityRegradeOptions& options)
{
    UtilityRegrade result;
    UtilityDrawing& total = result.drawing;
    cmd::ChangeSet changes;
    // Every layer and linetype a drawing of the services would make, by name.
    std::map<std::string, Layer> definitions;
    std::map<std::string, Linetype> linetypes;
    std::set<std::string> holding;

    for (const DrawnService& service : data.services) {
        // What it was drawn with, unless the regrade says otherwise.
        UtilityDrawOptions own;
        own.layerPrefix = prefixOf(model, service, options.layerPrefix);
        if (const auto spacing = options.spacing ? options.spacing : service.spacing) {
            own.grading.maximumDetectedSpacing = *spacing;
        }
        own.minimumCover = options.minimumCover ? options.minimumCover : service.minimumCover;
        auto drawn = drawUtilities({service.line}, own);
        if (!drawn) {
            return drawn.error();
        }
        for (Layer& layer : drawn->layers) {
            definitions.emplace(layer.name, std::move(layer));
        }
        for (Linetype& linetype : drawn->linetypes) {
            linetypes.emplace(linetype.name, std::move(linetype));
        }

        std::vector<const Entity*> runs;
        std::vector<const Entity*> points;
        for (const Entity& entity : drawn->entities) {
            (entity.type() == EntityType::Point ? points : runs).push_back(&entity);
        }
        // The points keep what is theirs and take what the grading says now.
        bool changed = false;
        for (std::size_t i = 0; i < points.size(); ++i) {
            const Entity& old = *model.entities.find(service.points[i]);
            Entity updated = old;
            std::erase_if(updated.properties,
                          [](const auto& entry) { return isDrawnKey(entry.first); });
            for (const auto& [key, value] : points[i]->properties) {
                updated.properties.insert_or_assign(key, value);
            }
            if (prefixUnder(old.layer, isPointsLeaf) == own.layerPrefix) {
                updated.layer = points[i]->layer;
            }
            holding.insert(updated.layer);
            if (updated != old) {
                changes.modify.push_back(std::move(updated));
                changed = true;
            }
        }
        // The runs are drawn again only when they differ, so a regrade of a
        // line nobody edited leaves its entities, and their ids, alone.
        bool sameRuns = runs.size() == service.runs.size();
        for (std::size_t i = 0; sameRuns && i < runs.size(); ++i) {
            const Entity& old = *model.entities.find(service.runs[i]);
            sameRuns = old.geometry == runs[i]->geometry && old.layer == runs[i]->layer &&
                       old.properties == runs[i]->properties;
        }
        for (const Entity* run : runs) {
            holding.insert(run->layer);
        }
        if (!sameRuns) {
            changes.remove.insert(changes.remove.end(), service.runs.begin(), service.runs.end());
            for (const Entity* run : runs) {
                changes.add.push_back(*run);
            }
            changed = true;
        }
        result.changed += changed ? 1 : 0;

        total.lines.push_back(drawn->lines.front());
        total.vertices += drawn->vertices;
        total.segments += drawn->segments;
        total.bounds.expand(drawn->bounds);
        std::ranges::move(drawn->entities, std::back_inserter(total.entities));
    }
    total.drawnLayers = holding.size();
    for (auto& entry : definitions) {
        total.layers.push_back(entry.second);
    }
    for (auto& entry : linetypes) {
        total.linetypes.push_back(entry.second);
    }
    if (changes.empty()) {
        return result;
    }

    // The layers what is added or moved lands on that the model lacks, with
    // their ancestors, parents first - a set of paths sorts a parent before
    // its children - and the linetypes those new layers name.
    std::set<std::string> make;
    const auto need = [&](const std::string& layer) {
        if (!model.layers.contains(layer) && definitions.contains(layer)) {
            make.insert(layer);
            for (const std::string& ancestor : katana::entity::layerAncestors(layer)) {
                if (!model.layers.contains(ancestor) && definitions.contains(ancestor)) {
                    make.insert(ancestor);
                }
            }
        }
    };
    for (const Entity& entity : changes.add) {
        need(entity.layer);
    }
    for (const Entity& entity : changes.modify) {
        need(entity.layer);
    }
    auto transaction = std::make_unique<cmd::Transaction>(std::string(kUtilityRegradeStep));
    std::set<std::string> named;
    for (const std::string& layer : make) {
        named.insert(definitions.at(layer).linetype);
    }
    for (const auto& [name, linetype] : linetypes) {
        if (named.contains(name) && !model.linetypes.contains(name)) {
            transaction->add(cmd::createLinetype(linetype));
        }
    }
    for (const std::string& layer : make) {
        transaction->add(cmd::createLayer(definitions.at(layer)));
    }
    transaction->add(std::make_unique<cmd::ChangeSetCommand>(
        "UTILITY_REGRADE_ENTITIES",
        [changes = std::move(changes)](const cmd::CommandContext&) {
            return Result<cmd::ChangeSet>(changes);
        }));
    result.command = std::move(transaction);
    return result;
}

// ---- design sources -------------------------------------------------------------------------

Result<sub::DesignAlignment> designFromEntity(const Entity& entity, std::optional<double> level)
{
    std::vector<katana::geometry::Point2> points;
    std::size_t own = 0; // vertices the entity itself has
    if (const auto* segment = std::get_if<katana::geometry::Segment2>(&entity.geometry)) {
        points = {segment->start, segment->end};
        own = 2;
    } else if (const auto* polyline = std::get_if<katana::geometry::Polyline2>(&entity.geometry)) {
        points = polyline->vertices;
        own = points.size();
        if (polyline->closed && points.size() >= 2) {
            points.push_back(points.front());
        }
    } else if (const auto* curve =
                   std::get_if<katana::geometry::CurvePolyline2>(&entity.geometry)) {
        // A centre line with arcs - drawn so, or read from DXF with bulges -
        // as chords within a millimetre, each with the height its vertices
        // give it, interpolated along a segment.
        sub::DesignAlignment design;
        design.id = "#" + std::to_string(entity.id);
        for (const auto& point : curve->tessellateWithHeights(katana::geometry::kCurveChordTolerance)) {
            design.vertices.push_back(
                {{point.position.y, point.position.x}, level ? level : point.height});
        }
        if (design.vertices.size() < 2) {
            return makeError(ErrorCode::InvalidArgument,
                             "entity #" + std::to_string(entity.id) +
                                 " has fewer than two vertices");
        }
        return design;
    } else {
        return makeError(ErrorCode::InvalidArgument,
                         "entity #" + std::to_string(entity.id) + " is " +
                             katana::core::lowered(katana::entity::toString(entity.type())) +
                             "; the design centre line is a line, a polyline or a curve polyline");
    }
    if (points.size() < 2) {
        return makeError(ErrorCode::InvalidArgument,
                         "entity #" + std::to_string(entity.id) + " has fewer than two vertices");
    }
    std::vector<std::optional<double>> heights =
        level ? std::vector<std::optional<double>>(own, level)
              : katana::entity::heightsOf(entity.properties, own);
    if (heights.size() < points.size()) {
        heights.push_back(heights.front()); // the closing vertex is the first
    }
    sub::DesignAlignment design;
    design.id = "#" + std::to_string(entity.id);
    for (std::size_t i = 0; i < points.size(); ++i) {
        design.vertices.push_back({{points[i].y, points[i].x}, heights[i]});
    }
    return design;
}

namespace {

// An alignment solved, and the stations its design is sampled at.
struct SampledAlignment {
    katana::geometry::SolvedAlignment solved;
    std::optional<katana::geometry::SolvedProfile> profile;
    std::vector<double> stations;
};

Result<SampledAlignment> sampleAlignment(const katana::entity::Alignment& alignment,
                                         double tolerance)
{
    auto solved = katana::geometry::solveAlignment(alignment.horizontal);
    if (!solved) {
        return solved.error();
    }
    std::optional<katana::geometry::SolvedProfile> profile;
    if (alignment.vertical) {
        auto solvedProfile = katana::geometry::solveProfile(*alignment.vertical);
        if (!solvedProfile) {
            return solvedProfile.error();
        }
        profile = std::move(solvedProfile).value();
    }

    // Where the plan geometry needs a vertex: each element's ends, and along
    // a curve as often as the one sagitta rule asks.
    std::vector<double> stations;
    const auto divide = [&stations](double start, double length, std::size_t pieces) {
        pieces = std::max<std::size_t>(pieces, 1);
        for (std::size_t k = 0; k <= pieces; ++k) {
            stations.push_back(start + length * static_cast<double>(k) /
                                           static_cast<double>(pieces));
        }
    };
    for (const katana::geometry::AlignmentElement& element : solved->elements()) {
        const std::size_t pieces = std::visit(
            [tolerance](const auto& shape) -> std::size_t {
                using Shape = std::decay_t<decltype(shape)>;
                if constexpr (std::is_same_v<Shape, katana::geometry::Arc2>) {
                    return katana::geometry::sagittaChordCount(shape.radius, shape.sweep,
                                                               tolerance);
                } else if constexpr (std::is_same_v<Shape, katana::geometry::Spiral2>) {
                    return shape.chordCountFor(tolerance) - 1;
                } else {
                    return 1;
                }
            },
            element.shape);
        divide(element.startStation, element.length, pieces);
    }
    // Where the levels need one: each key station of the profile, and inside
    // a vertical curve closely enough that the straight line between two
    // samples stays within `tolerance` of the parabola. A chord of length l on
    // a parabola whose grade changes by A over L departs from it by at most
    // A l^2 / (8 L), at its middle, so l = sqrt(8 L tolerance / A). Capped at
    // 8192 pieces, as geometry::sagittaChordCount caps a circle's.
    if (profile) {
        for (const double key : profile->keyStations()) {
            stations.push_back(key);
        }
        for (const katana::geometry::ProfileElement& element : profile->elements()) {
            const double change = std::abs(element.endGrade - element.startGrade);
            if (element.kind != katana::geometry::ProfileElementKind::Curve || !(change > 0.0) ||
                !(element.length > 0.0)) {
                continue;
            }
            const double chord = std::sqrt(8.0 * element.length * tolerance / change);
            const double pieces = std::min(std::ceil(element.length / chord), 8192.0);
            divide(element.startStation, element.length, static_cast<std::size_t>(pieces));
        }
    }
    // The ends are the alignment's own, and every station is held inside
    // them rather than dropped: the last element's end is the elements'
    // lengths summed from the start, which can come out one rounding past
    // endStation() - and dropping it would drop the whole last tangent. A
    // profile station off either end folds onto that end, and the unique
    // below makes it one with it.
    const double first = solved->startStation();
    const double last = solved->endStation();
    for (double& station : stations) {
        station = std::clamp(station, first, last);
    }
    stations.push_back(first);
    stations.push_back(last);
    std::ranges::sort(stations);
    // Stations a rounding apart are one: a key station of the profile falls
    // on an element's end as often as not.
    stations.erase(std::unique(stations.begin(), stations.end(),
                               [](double a, double b) { return std::abs(a - b) < 1e-9; }),
                   stations.end());
    // A station within that of an end is the end itself.
    stations.front() = first;
    stations.back() = last;
    return SampledAlignment{std::move(solved).value(), std::move(profile), std::move(stations)};
}

} // namespace

Result<std::vector<double>> designStations(const katana::entity::Alignment& alignment,
                                           double tolerance)
{
    auto sampled = sampleAlignment(alignment, tolerance);
    if (!sampled) {
        return sampled.error();
    }
    return std::move(sampled->stations);
}

Result<sub::DesignAlignment> designFromAlignment(const katana::entity::Alignment& alignment,
                                                 double tolerance)
{
    auto sampled = sampleAlignment(alignment, tolerance);
    if (!sampled) {
        return sampled.error();
    }
    sub::DesignAlignment design;
    design.id = alignment.name;
    for (const double station : sampled->stations) {
        const auto point = sampled->solved.pointAtStation(station);
        if (!point) {
            continue;
        }
        design.vertices.push_back(
            {{point->y, point->x},
             sampled->profile ? sampled->profile->elevationAt(station) : std::nullopt});
    }
    return design;
}

} // namespace katana::cad::utilities
