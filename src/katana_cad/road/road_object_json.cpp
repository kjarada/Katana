// The road objects as JSON: what the drawing keeps under its "road_objects"
// metadata key and ROADOBJECT LIST JSON prints
// (include/katana/cad/road/road_objects.hpp, docs/road_objects.md, "The JSON").

#include <set>
#include <string>
#include <utility>

#include <nlohmann/json.hpp>

#include "katana/cad/road/road_objects.hpp"
#include "katana/core/text.hpp"

namespace katana::cad::road {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using Json = nlohmann::json;

namespace {

constexpr std::string_view kFormat = "katana-road-objects";

// Thrown inside the reader and caught at its edge, like the JSON library's
// own exceptions, so the helpers below stay plain value functions.
struct BadValue {
    std::string what;
};

Json vertexJson(const PlacedVertex& vertex)
{
    // Absent is not zero: a vertex with no height says null.
    return Json::array({vertex.position.x, vertex.position.y,
                        vertex.z ? Json(*vertex.z) : Json(nullptr)});
}

PlacedVertex vertexFrom(const Json& json)
{
    if (!json.is_array() || (json.size() != 2 && json.size() != 3)) {
        throw BadValue{"a vertex is [x, y] or [x, y, z]"};
    }
    PlacedVertex vertex;
    vertex.position = katana::geometry::Point2(json.at(0).get<double>(), json.at(1).get<double>());
    if (json.size() == 3 && !json.at(2).is_null()) {
        vertex.z = json.at(2).get<double>();
    }
    return vertex;
}

Json placementJson(const RoadObjectPlacement& placement)
{
    Json json{{"kind", placementWord(placement.kind)}};
    if (placement.kind == PlacementKind::Alignment) {
        json["alignment"] = placement.alignment;
        json["station"] = placement.station;
        if (placement.to) {
            json["to"] = *placement.to;
        }
        json["offset"] = placement.offset;
    } else {
        Json vertices = Json::array();
        for (const PlacedVertex& vertex : placement.vertices) {
            vertices.push_back(vertexJson(vertex));
        }
        json["vertices"] = std::move(vertices);
        if (placement.source != katana::entity::kInvalidEntityId) {
            json["source"] = placement.source;
        }
    }
    if (placement.spacing) {
        json["spacing"] = *placement.spacing;
    }
    if (placement.heights) {
        json["heights"] = heightWord(*placement.heights);
    }
    if (!placement.surface.empty()) {
        json["surface"] = placement.surface;
    }
    if (placement.crossfall != 0.0) {
        json["crossfall"] = placement.crossfall;
    }
    if (placement.dz != 0.0) {
        json["dz"] = placement.dz;
    }
    return json;
}

RoadObjectPlacement placementFrom(const Json& json)
{
    RoadObjectPlacement placement;
    const std::string kind = json.at("kind").get<std::string>();
    if (kind == "alignment") {
        placement.kind = PlacementKind::Alignment;
        placement.alignment = json.at("alignment").get<std::string>();
        placement.station = json.at("station").get<double>();
        if (json.contains("to")) {
            placement.to = json.at("to").get<double>();
        }
        placement.offset = json.value("offset", 0.0);
    } else if (kind == "line" || kind == "points") {
        placement.kind = kind == "line" ? PlacementKind::Line : PlacementKind::Points;
        for (const Json& vertex : json.at("vertices")) {
            placement.vertices.push_back(vertexFrom(vertex));
        }
        if (placement.vertices.empty()) {
            throw BadValue{"a placement along a line or at points has its points"};
        }
        placement.source = json.value("source", katana::entity::kInvalidEntityId);
    } else {
        throw BadValue{"unknown placement \"" + kind + "\""};
    }
    if (json.contains("spacing")) {
        placement.spacing = json.at("spacing").get<double>();
        if (!(*placement.spacing > 0.0)) {
            throw BadValue{"a spacing is more than 0"};
        }
    }
    if (json.contains("heights")) {
        auto source = heightFromWord(json.at("heights").get<std::string>());
        if (!source) {
            throw BadValue{source.error().message};
        }
        placement.heights = *source;
    }
    placement.surface = json.value("surface", std::string{});
    placement.crossfall = json.value("crossfall", 0.0);
    placement.dz = json.value("dz", 0.0);
    return placement;
}

Json objectJson(const RoadObject& object)
{
    Json json{{"id", object.id}, {"kind", kindWord(object.kind)}};
    // What was not given is left out, so a file says what a person chose.
    if (!object.name.empty()) {
        json["name"] = object.name;
    }
    if (!object.layer.empty()) {
        json["layer"] = object.layer;
    }
    if (object.colour) {
        json["colour"] = object.colour->toHex();
    }
    if (!object.material.empty()) {
        json["material"] = object.material;
    }
    if (!object.facility.empty()) {
        json["facility"] = object.facility;
    }
    json["placement"] = placementJson(object.placement);
    Json parameters = Json::object();
    for (const auto& [key, value] : object.parameters) {
        if (const auto* number = std::get_if<double>(&value)) {
            parameters[key] = *number;
        } else {
            parameters[key] = std::get<std::string>(value);
        }
    }
    json["parameters"] = std::move(parameters);
    return json;
}

RoadObject objectFrom(const Json& json)
{
    RoadObject object;
    object.id = json.at("id").get<std::string>();
    if (object.id.empty()) {
        throw BadValue{"an object has an id"};
    }
    auto kind = kindFromWord(json.at("kind").get<std::string>());
    if (!kind) {
        throw BadValue{kind.error().message + " (" + object.id + ")"};
    }
    object.kind = *kind;
    object.name = json.value("name", std::string{});
    object.layer = json.value("layer", std::string{});
    if (json.contains("colour")) {
        auto colour = katana::entity::Color::fromHex(json.at("colour").get<std::string>());
        if (!colour) {
            throw BadValue{"object " + object.id + ": " + colour.error().message};
        }
        object.colour = *colour;
    }
    object.material = json.value("material", std::string{});
    object.facility = json.value("facility", std::string{});
    object.placement = placementFrom(json.at("placement"));
    if (json.contains("parameters")) {
        for (const auto& [key, value] : json.at("parameters").items()) {
            // Read through the one parser, so a value edited out of range in
            // the file is refused as a typed one would be.
            const std::string text = value.is_number() ? katana::core::formatExactReal(
                                                             value.get<double>())
                                                       : value.get<std::string>();
            auto parsed = parseParameter(object.kind, key, text);
            if (!parsed) {
                throw BadValue{"object " + object.id + ": " + parsed.error().message};
            }
            object.parameters.insert_or_assign(key, std::move(*parsed));
        }
    }
    return object;
}

} // namespace

Result<std::string> roadObjectsToJson(const RoadObjectSet& set)
{
    Json root{{"format", kFormat}, {"version", kRoadObjectsVersion}, {"next", set.nextNumber}};
    Json objects = Json::array();
    for (const RoadObject& object : set.objects) {
        objects.push_back(objectJson(object));
    }
    root["objects"] = std::move(objects);
    try {
        return root.dump(2);
    } catch (const Json::exception& error) {
        // Text that is not UTF-8 - typed on a console in another code page.
        return makeError(ErrorCode::InvalidArgument, "a road object's text is not UTF-8",
                         error.what());
    }
}

Result<RoadObjectSet> roadObjectsFromJson(std::string_view text)
{
    try {
        const Json root = Json::parse(text);
        if (root.value("format", std::string{}) != kFormat) {
            return makeError(ErrorCode::ParseFailure, "not a set of road objects");
        }
        const int version = root.at("version").get<int>();
        if (version > kRoadObjectsVersion) {
            return makeError(ErrorCode::Unsupported,
                             "the road objects were written by a newer version of Katana",
                             "version " + std::to_string(version));
        }
        RoadObjectSet set;
        set.nextNumber = root.value("next", std::uint64_t{1});
        std::set<std::string> ids;
        for (const Json& object : root.value("objects", Json::array())) {
            RoadObject read = objectFrom(object);
            if (!ids.insert(katana::core::lowered(read.id)).second) {
                throw BadValue{"two objects are called " + read.id};
            }
            set.objects.push_back(std::move(read));
        }
        if (set.nextNumber < 1) {
            throw BadValue{"the next number is 1 or more"};
        }
        return set;
    } catch (const Json::exception& error) {
        return makeError(ErrorCode::ParseFailure, "the road objects are not in the expected format",
                         error.what());
    } catch (const BadValue& error) {
        return makeError(ErrorCode::ParseFailure, "the road objects are not in the expected format",
                         error.what);
    }
}

} // namespace katana::cad::road
