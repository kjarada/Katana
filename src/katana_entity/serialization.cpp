#include "katana/entity/serialization.hpp"

#include <nlohmann/json.hpp>

#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/tables.hpp"

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

// An AnchorRef as {"entity": id, "point": "end", "index": 3}; the index only
// when it says something.
Json anchorToJson(const AnchorRef& ref)
{
    Json j = {{"entity", ref.entity}, {"point", std::string(toString(ref.point))}};
    if (ref.point == AnchorPoint::Vertex || ref.point == AnchorPoint::SegmentMid ||
        (ref.point == AnchorPoint::Along && ref.index != 0)) {
        j["index"] = ref.index;
    }
    if (ref.point == AnchorPoint::Along) {
        j["parameter"] = ref.parameter;
    }
    return j;
}

AnchorRef anchorFromJson(const Json& j)
{
    AnchorRef ref;
    ref.entity = j.at("entity").get<std::uint64_t>();
    const auto point = anchorPointFromString(j.at("point").get<std::string>());
    if (!point) {
        throw Json::other_error::create(501, point.error().describe(), &j);
    }
    ref.point = *point;
    ref.index = j.value("index", std::uint32_t{0});
    ref.parameter = j.value("parameter", 0.0);
    return ref;
}

// The enumerations are written by name and read back through the same
// fromString the command line uses, so the two cannot disagree.
template <typename Enum, typename Parse> Enum enumFromJson(const Json& j, Parse parse)
{
    const auto value = parse(j.get<std::string>());
    if (!value) {
        throw Json::other_error::create(501, value.error().describe(), &j);
    }
    return *value;
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
        // The members added on 2026-09-25 are written only when they differ
        // from their defaults, so a text or a dimension of the old kind is the
        // same JSON as before.
        Json operator()(const TextGeometry& g) const
        {
            Json j = {{"type", "Text"},
                      {"position", pointToJson(g.position)},
                      {"text", g.text},
                      {"height", g.height},
                      {"rotation", g.rotation}};
            if (!g.style.empty()) {
                j["style"] = g.style;
            }
            if (g.paperHeight != 0.0) {
                j["paperHeight"] = g.paperHeight;
            }
            if (g.justify != TextJustify::BottomLeft) {
                j["justify"] = std::string(toString(g.justify));
            }
            return j;
        }
        Json operator()(const DimensionGeometry& g) const
        {
            Json j = {{"type", "Dimension"},
                      {"start", pointToJson(g.start)},
                      {"end", pointToJson(g.end)},
                      {"offset", g.offset},
                      {"textOverride", g.textOverride}};
            if (g.kind != DimensionKind::Aligned) {
                j["kind"] = std::string(toString(g.kind));
            }
            if (g.angle != 0.0) {
                j["angle"] = g.angle;
            }
            if (g.usesVertex() || g.vertex != Point2{}) {
                j["vertex"] = pointToJson(g.vertex);
            }
            for (const auto& [key, ref] : {std::pair{"startRef", &g.startRef},
                                           std::pair{"endRef", &g.endRef},
                                           std::pair{"vertexRef", &g.vertexRef}}) {
                if (ref->associated()) {
                    j[key] = anchorToJson(*ref);
                }
            }
            return j;
        }
        Json operator()(const LabelGeometry& g) const
        {
            Json j = {{"type", "Label"}, {"style", g.style}, {"anchor", pointToJson(g.anchor)}};
            if (g.target != 0) {
                j["target"] = g.target;
            }
            if (!g.alignment.empty()) {
                j["alignment"] = g.alignment;
            }
            if (g.part != -1) {
                j["part"] = g.part;
            }
            if (g.position) {
                j["position"] = pointToJson(*g.position);
            }
            if (!g.textOverride.empty()) {
                j["textOverride"] = g.textOverride;
            }
            if (!g.rule.empty()) {
                j["rule"] = g.rule;
            }
            return j;
        }
        Json operator()(const LeaderGeometry& g) const
        {
            Json vertices = Json::array();
            for (const Point2& vertex : g.vertices) {
                vertices.push_back(pointToJson(vertex));
            }
            Json j = {{"type", "Leader"},
                      {"vertices", std::move(vertices)},
                      {"text", g.text},
                      {"arrow", std::string(toString(g.arrow))},
                      {"callout", std::string(toString(g.callout))},
                      {"style", g.style},
                      {"paperHeight", g.paperHeight},
                      {"arrowSize", g.arrowSize},
                      {"landing", g.landing}};
            if (g.tipRef.associated()) {
                j["tipRef"] = anchorToJson(g.tipRef);
            }
            if (g.fields) {
                j["fields"] = true;
            }
            if (!g.labelStyle.empty()) {
                j["labelStyle"] = g.labelStyle;
            }
            return j;
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
    case EntityType::Text: {
        TextGeometry text;
        text.position = pointFromJson(j.at("position"));
        text.text = j.at("text").get<std::string>();
        text.height = j.at("height").get<double>();
        text.rotation = j.at("rotation").get<double>();
        text.style = j.value("style", std::string{});
        text.paperHeight = j.value("paperHeight", 0.0);
        if (j.contains("justify")) {
            text.justify = enumFromJson<TextJustify>(j.at("justify"), textJustifyFromString);
        }
        geometry = std::move(text);
        break;
    }
    case EntityType::Dimension: {
        DimensionGeometry dimension;
        dimension.start = pointFromJson(j.at("start"));
        dimension.end = pointFromJson(j.at("end"));
        dimension.offset = j.at("offset").get<double>();
        dimension.textOverride = j.value("textOverride", std::string{});
        if (j.contains("kind")) {
            dimension.kind = enumFromJson<DimensionKind>(j.at("kind"), dimensionKindFromString);
        }
        dimension.angle = j.value("angle", 0.0);
        if (j.contains("vertex")) {
            dimension.vertex = pointFromJson(j.at("vertex"));
        }
        for (const auto& [key, ref] : {std::pair{"startRef", &dimension.startRef},
                                       std::pair{"endRef", &dimension.endRef},
                                       std::pair{"vertexRef", &dimension.vertexRef}}) {
            if (j.contains(key)) {
                *ref = anchorFromJson(j.at(key));
            }
        }
        geometry = std::move(dimension);
        break;
    }
    case EntityType::Label: {
        LabelGeometry label;
        label.style = j.at("style").get<std::string>();
        label.anchor = pointFromJson(j.at("anchor"));
        label.target = j.value("target", std::uint64_t{0});
        label.alignment = j.value("alignment", std::string{});
        label.part = j.value("part", std::int32_t{-1});
        if (j.contains("position")) {
            label.position = pointFromJson(j.at("position"));
        }
        label.textOverride = j.value("textOverride", std::string{});
        label.rule = j.value("rule", std::string{});
        geometry = std::move(label);
        break;
    }
    case EntityType::Leader: {
        LeaderGeometry leader;
        for (const Json& vertex : j.at("vertices")) {
            leader.vertices.push_back(pointFromJson(vertex));
        }
        leader.text = j.value("text", std::string{});
        leader.arrow = enumFromJson<ArrowHead>(j.at("arrow"), arrowHeadFromString);
        leader.callout = enumFromJson<CalloutShape>(j.at("callout"), calloutShapeFromString);
        leader.style = j.value("style", std::string{});
        leader.paperHeight = j.value("paperHeight", 0.0);
        leader.arrowSize = j.value("arrowSize", 2.5);
        leader.landing = j.value("landing", 2.5);
        if (j.contains("tipRef")) {
            leader.tipRef = anchorFromJson(j.at("tipRef"));
        }
        leader.fields = j.value("fields", false);
        leader.labelStyle = j.value("labelStyle", std::string{});
        geometry = std::move(leader);
        break;
    }
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

// ---- label style definitions ------------------------------------------------------

namespace {

// The version labelStyleDefinitionToJson writes.
constexpr int kLabelStyleDefinitionVersion = 1;

std::optional<Color> colourFromJson(const Json& j)
{
    const auto colour = Color::fromHex(j.get<std::string>());
    if (!colour) {
        throw Json::other_error::create(501, colour.error().describe(), &j);
    }
    return *colour;
}

} // namespace

Result<std::string> labelStyleDefinitionToJson(const LabelStyle& style)
{
    const LabelStyle defaults;
    Json j = {{"version", kLabelStyleDefinitionVersion}};
    // Always written: the template is what a style is for, and a reader of the
    // raw row should see it.
    j["text"] = style.text;
    if (style.textStyle != defaults.textStyle) {
        j["textStyle"] = style.textStyle;
    }
    // Doubles are compared bit for bit through ==, which is right here:
    // they are either the default literal or a value someone set.
    const auto real = [&](const char* key, double value, double fallback) {
        if (value != fallback) {
            j[key] = value;
        }
    };
    real("paperHeight", style.paperHeight, defaults.paperHeight);
    real("offset", style.offset, defaults.offset);
    real("markerSize", style.markerSize, defaults.markerSize);
    real("minimumLength", style.minimumLength, defaults.minimumLength);
    real("interval", style.interval, defaults.interval);
    real("tickInterval", style.tickInterval, defaults.tickInterval);
    real("tickLength", style.tickLength, defaults.tickLength);
    if (style.placement != defaults.placement) {
        j["placement"] = std::string(toString(style.placement));
    }
    if (style.orientation != defaults.orientation) {
        j["orientation"] = std::string(toString(style.orientation));
    }
    if (style.marker != defaults.marker) {
        j["marker"] = std::string(toString(style.marker));
    }
    if (style.leader != defaults.leader) {
        j["leader"] = style.leader;
    }
    if (style.displace != defaults.displace) {
        j["displace"] = style.displace;
    }
    if (style.priority != defaults.priority) {
        j["priority"] = style.priority;
    }
    if (style.color) {
        j["color"] = style.color->toHex();
    }
    return dumped(j);
}

katana::core::Status labelStyleDefinitionFromJson(std::string_view json, LabelStyle& style)
{
    auto parsed = guarded<LabelStyle>([&]() -> Result<LabelStyle> {
        const Json j = parse(json);
        const int version = j.at("version").get<int>();
        if (version > kLabelStyleDefinitionVersion) {
            return makeError(ErrorCode::Unsupported,
                             "the label style was written by a newer version of Katana",
                             "version=" + std::to_string(version));
        }
        LabelStyle read;
        read.name = style.name;
        read.kind = style.kind;
        read.text = j.at("text").get<std::string>();
        read.textStyle = j.value("textStyle", read.textStyle);
        read.paperHeight = j.value("paperHeight", read.paperHeight);
        read.offset = j.value("offset", read.offset);
        read.markerSize = j.value("markerSize", read.markerSize);
        read.minimumLength = j.value("minimumLength", read.minimumLength);
        read.interval = j.value("interval", read.interval);
        read.tickInterval = j.value("tickInterval", read.tickInterval);
        read.tickLength = j.value("tickLength", read.tickLength);
        if (j.contains("placement")) {
            read.placement =
                enumFromJson<LabelPlacement>(j.at("placement"), labelPlacementFromString);
        }
        if (j.contains("orientation")) {
            read.orientation =
                enumFromJson<LabelOrientation>(j.at("orientation"), labelOrientationFromString);
        }
        if (j.contains("marker")) {
            read.marker = enumFromJson<LabelMarker>(j.at("marker"), labelMarkerFromString);
        }
        read.leader = j.value("leader", read.leader);
        read.displace = j.value("displace", read.displace);
        read.priority = j.value("priority", read.priority);
        if (j.contains("color")) {
            read.color = colourFromJson(j.at("color"));
        }
        return read;
    });
    if (!parsed) {
        return parsed.error();
    }
    style = std::move(*parsed);
    return {};
}

} // namespace katana::entity
