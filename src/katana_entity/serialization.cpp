#include "katana/entity/serialization.hpp"

#include <nlohmann/json.hpp>

#include "katana/entity/entity_geometry.hpp"

namespace katana::entity {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using Json = nlohmann::json;

// nlohmann reports malformed documents by throwing. Those exceptions are caught
// at each public function below and never leave this translation unit.

namespace {

Json pointToJson(const Point2& p)
{
    return Json::array({p.x, p.y});
}

Point2 pointFromJson(const Json& j)
{
    if (!j.is_array() || j.size() != 2) {
        throw Json::other_error::create(501, "a point must be an array of two numbers", &j);
    }
    return Point2(j.at(0).get<double>(), j.at(1).get<double>());
}

Json toJsonValue(const Geometry& geometry)
{
    struct Visitor {
        Json operator()(const PointGeometry& g) const
        {
            return {{"type", "Point"}, {"position", pointToJson(g.position)}};
        }
        Json operator()(const Segment2& g) const
        {
            return {{"type", "Line"}, {"start", pointToJson(g.start)}, {"end", pointToJson(g.end)}};
        }
        Json operator()(const Arc2& g) const
        {
            return {{"type", "Arc"},
                    {"center", pointToJson(g.center)},
                    {"radius", g.radius},
                    {"startAngle", g.startAngle},
                    {"sweep", g.sweep}};
        }
        Json operator()(const Polyline2& g) const
        {
            Json vertices = Json::array();
            for (const Point2& vertex : g.vertices) {
                vertices.push_back(pointToJson(vertex));
            }
            return {{"type", "Polyline"}, {"closed", g.closed}, {"vertices", std::move(vertices)}};
        }
        Json operator()(const Circle2& g) const
        {
            return {{"type", "Circle"}, {"center", pointToJson(g.center)}, {"radius", g.radius}};
        }
        Json operator()(const TextGeometry& g) const
        {
            return {{"type", "Text"},
                    {"position", pointToJson(g.position)},
                    {"text", g.text},
                    {"height", g.height},
                    {"rotation", g.rotation}};
        }
        Json operator()(const DimensionGeometry& g) const
        {
            return {{"type", "Dimension"},
                    {"start", pointToJson(g.start)},
                    {"end", pointToJson(g.end)},
                    {"offset", g.offset},
                    {"textOverride", g.textOverride}};
        }
    };
    return std::visit(Visitor{}, geometry);
}

Result<Geometry> geometryFromJsonValue(const Json& j)
{
    const auto type = entityTypeFromString(j.at("type").get<std::string>());
    if (!type) {
        return type.error();
    }
    Geometry geometry;
    switch (*type) {
    case EntityType::Point:
        geometry = PointGeometry{pointFromJson(j.at("position"))};
        break;
    case EntityType::Line:
        geometry = Segment2{pointFromJson(j.at("start")), pointFromJson(j.at("end"))};
        break;
    case EntityType::Arc:
        geometry = Arc2{pointFromJson(j.at("center")), j.at("radius").get<double>(),
                        j.at("startAngle").get<double>(), j.at("sweep").get<double>()};
        break;
    case EntityType::Polyline: {
        Polyline2 polyline;
        polyline.closed = j.at("closed").get<bool>();
        for (const Json& vertex : j.at("vertices")) {
            polyline.vertices.push_back(pointFromJson(vertex));
        }
        geometry = std::move(polyline);
        break;
    }
    case EntityType::Circle:
        geometry = Circle2{pointFromJson(j.at("center")), j.at("radius").get<double>()};
        break;
    case EntityType::Text:
        geometry = TextGeometry{pointFromJson(j.at("position")), j.at("text").get<std::string>(),
                                j.at("height").get<double>(), j.at("rotation").get<double>()};
        break;
    case EntityType::Dimension:
        geometry = DimensionGeometry{pointFromJson(j.at("start")), pointFromJson(j.at("end")),
                                     j.at("offset").get<double>(),
                                     j.value("textOverride", std::string{})};
        break;
    }
    if (auto status = validate(geometry); !status) {
        return status.error();
    }
    return geometry;
}

Json toJsonValue(const PropertyMap& properties)
{
    Json object = Json::object();
    for (const auto& [name, value] : properties) {
        std::visit([&](const auto& v) { object[name] = v; }, value);
    }
    return object;
}

Result<PropertyMap> propertiesFromJsonValue(const Json& j)
{
    if (!j.is_object()) {
        return makeError(ErrorCode::ParseFailure, "properties must be a JSON object");
    }
    PropertyMap properties;
    for (const auto& [name, value] : j.items()) {
        if (value.is_boolean()) {
            properties.emplace(name, value.get<bool>());
        } else if (value.is_number_integer()) {
            properties.emplace(name, value.get<std::int64_t>());
        } else if (value.is_number_float()) {
            properties.emplace(name, value.get<double>());
        } else if (value.is_string()) {
            properties.emplace(name, value.get<std::string>());
        } else {
            return makeError(ErrorCode::ParseFailure,
                             "property values must be boolean, number or string", name);
        }
    }
    return properties;
}

Json parse(std::string_view text)
{
    return Json::parse(text.begin(), text.end());
}

template <typename T, typename Function> Result<T> guarded(Function&& function)
{
    try {
        return function();
    } catch (const Json::exception& error) {
        return makeError(ErrorCode::ParseFailure, "malformed JSON", error.what());
    }
}

// dump() throws type_error.316 when any string in the document is not valid
// UTF-8. Writing is not parsing, so the failure is reported as Internal: the
// strings were validated on the way into the model, and reaching this means a
// guard was bypassed, not that the user typed something odd.
Result<std::string> dumped(const Json& json)
{
    try {
        return json.dump();
    } catch (const Json::exception& error) {
        return makeError(ErrorCode::Internal, "value could not be written as JSON",
                         error.what());
    }
}

} // namespace

Result<std::string> geometryToJson(const Geometry& geometry)
{
    return dumped(toJsonValue(geometry));
}

Result<Geometry> geometryFromJson(std::string_view json)
{
    return guarded<Geometry>([&] { return geometryFromJsonValue(parse(json)); });
}

Result<std::string> propertiesToJson(const PropertyMap& properties)
{
    return dumped(toJsonValue(properties));
}

Result<PropertyMap> propertiesFromJson(std::string_view json)
{
    return guarded<PropertyMap>([&] { return propertiesFromJsonValue(parse(json)); });
}

Result<std::string> entityToJson(const Entity& entity)
{
    Json j = {{"id", entity.id},
              {"layer", entity.layer},
              {"style", entity.style},
              {"visible", entity.visible},
              {"geometry", toJsonValue(entity.geometry)},
              {"properties", toJsonValue(entity.properties)},
              {"metadata", toJsonValue(entity.metadata)}};
    j["color"] = entity.color ? Json(entity.color->toHex()) : Json(nullptr);
    return dumped(j);
}

Result<Entity> entityFromJson(std::string_view json)
{
    return guarded<Entity>([&]() -> Result<Entity> {
        const Json j = parse(json);
        Entity entity;
        entity.id = j.value("id", kInvalidEntityId);
        entity.layer = j.value("layer", std::string("0"));
        entity.style = j.value("style", std::string{});
        entity.visible = j.value("visible", true);
        if (j.contains("color") && !j.at("color").is_null()) {
            const auto color = Color::fromHex(j.at("color").get<std::string>());
            if (!color) {
                return color.error();
            }
            entity.color = *color;
        }
        auto geometry = geometryFromJsonValue(j.at("geometry"));
        if (!geometry) {
            return geometry.error();
        }
        entity.geometry = std::move(*geometry);
        for (const auto& [key, target] :
             {std::pair{"properties", &entity.properties}, std::pair{"metadata", &entity.metadata}}) {
            if (j.contains(key)) {
                auto parsed = propertiesFromJsonValue(j.at(key));
                if (!parsed) {
                    return parsed.error();
                }
                *target = std::move(*parsed);
            }
        }
        return entity;
    });
}

} // namespace katana::entity
