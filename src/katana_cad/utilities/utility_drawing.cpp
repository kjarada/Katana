#include "katana/cad/utilities/utility_drawing.hpp"

#include <algorithm>
#include <format>
#include <map>
#include <memory>
#include <set>
#include <utility>

#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/layer_path.hpp"

namespace katana::cad::utilities {

namespace sub = katana::survey::subsurface;
namespace cmd = katana::commands;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::Color;
using katana::entity::Entity;
using katana::entity::Layer;
using katana::entity::Linetype;
using katana::entity::LinetypeElement;
using katana::entity::PropertyMap;
using katana::geometry::Point2;

namespace {

Color hex(std::uint8_t r, std::uint8_t g, std::uint8_t b)
{
    return Color{r, g, b, 255};
}

// A plan point from a survey coordinate: x is the easting.
Point2 planPoint(const sub::UtilityVertex& vertex)
{
    return Point2(vertex.position.easting, vertex.position.northing);
}

void setText(PropertyMap& properties, std::string_view key, const std::string& value)
{
    if (!value.empty()) {
        properties.insert_or_assign(std::string(key), value);
    }
}

void setReal(PropertyMap& properties, std::string_view key, std::optional<double> value)
{
    if (value) {
        properties.insert_or_assign(std::string(key), *value);
    }
}

void setFields(PropertyMap& properties, const std::map<std::string, std::string>& fields)
{
    for (const auto& [name, value] : fields) {
        setText(properties, std::string(keys::kFieldPrefix) + name, value);
    }
}

// What a polyline of a service carries whatever its level.
PropertyMap lineProperties(const sub::UtilityLine& line)
{
    const sub::UtilityAttributes& attributes = line.attributes;
    PropertyMap properties;
    properties.insert_or_assign(std::string(keys::kLine), line.id);
    properties.insert_or_assign(std::string(keys::kType),
                                std::string(utilityTypeWord(attributes.type)));
    setText(properties, keys::kOwner, attributes.owner);
    setText(properties, keys::kMaterial, attributes.material);
    if (attributes.diameter > 0.0) {
        properties.insert_or_assign(std::string(keys::kDiameter), attributes.diameter);
        properties.insert_or_assign(std::string(keys::kDiameterInside),
                                    attributes.diameterIsInside);
    }
    setText(properties, keys::kConfiguration, attributes.configuration);
    setText(properties, keys::kDescription, attributes.description);
    properties.insert_or_assign(std::string(keys::kStatus),
                                std::string(sub::toString(attributes.status)));
    setFields(properties, attributes.fields);
    return properties;
}

// Each distinct reason once, in the order the segments give them: a run of
// three segments all limited by one weak vertex says so once.
std::string limitedBy(const std::vector<sub::GradedSegment>& segments, std::size_t first,
                      std::size_t last)
{
    std::vector<std::string> reasons;
    for (std::size_t i = first; i <= last; ++i) {
        const std::string& reason = segments[i].limitedBy;
        if (!reason.empty() && std::ranges::find(reasons, reason) == reasons.end()) {
            reasons.push_back(reason);
        }
    }
    std::string text;
    for (const std::string& reason : reasons) {
        text += (text.empty() ? "" : "; ") + reason;
    }
    return text;
}

Entity vertexPoint(const sub::UtilityLine& line, std::size_t index, const sub::GradedVertex& graded,
                   const sub::CoverResult& cover, const UtilityDrawOptions& options,
                   const std::string& layer)
{
    const sub::UtilityVertex& vertex = line.vertices[index];
    Entity point;
    point.geometry = katana::entity::PointGeometry{planPoint(vertex)};
    point.layer = layer;
    PropertyMap& properties = point.properties;
    properties.insert_or_assign(std::string(keys::kLine), line.id);
    properties.insert_or_assign(std::string(keys::kType),
                                std::string(utilityTypeWord(line.attributes.type)));
    setText(properties, keys::kVertex, vertex.id);
    properties.insert_or_assign(std::string(keys::kMethod),
                                std::string(sub::toString(vertex.evidence.method)));
    properties.insert_or_assign(std::string(keys::kQualityLevel),
                                std::string(sub::toString(graded.classification.level)));
    if (vertex.claimed) {
        properties.insert_or_assign(std::string(keys::kClaimed),
                                    std::string(sub::toString(*vertex.claimed)));
    }
    setText(properties, keys::kOverClaim, graded.overClaim);
    if (const std::optional<double> level = sub::serviceLevel(vertex)) {
        properties.insert_or_assign(std::string(keys::kServiceLevel), *level);
        properties.insert_or_assign(std::string(keys::kLevelReference),
                                    std::string(sub::toString(vertex.levelReference)));
    }
    if (sub::hasVerticalMeasurement(vertex)) {
        properties.insert_or_assign(std::string(keys::kLevelQualified),
                                    graded.classification.levelQualified);
    }
    setReal(properties, keys::kSurfaceLevel, vertex.surfaceLevel);
    setReal(properties, keys::kCover, cover.cover);
    setText(properties, keys::kCoverNote, cover.note);
    if (cover.cover && options.minimumCover) {
        properties.insert_or_assign(std::string(keys::kCoverBelowMinimum), cover.belowMinimum);
    }
    setText(properties, keys::kVerifies, vertex.verifies);
    setFields(properties, vertex.fields);
    return point;
}

// The layer `name` for a service of `type`, and every ancestor it lacks, into
// `layers` by name - a map, so parents sort before their children. The
// ancestors take the defaults but for the kind's own group,
// "<prefix>/<type>", which takes its colour, so the layer tree reads as the
// plan does.
void needLayer(std::map<std::string, Layer>& layers, std::string_view prefix, sub::UtilityType type,
               const std::string& name, std::string_view linetype)
{
    const std::string group = katana::entity::joinLayerPath(prefix, utilityTypeWord(type));
    for (const std::string& ancestor : katana::entity::layerAncestors(name)) {
        if (!layers.contains(ancestor)) {
            Layer parent;
            parent.name = ancestor;
            if (ancestor == group) {
                parent.color = utilityTypeColour(type);
            }
            layers.emplace(ancestor, std::move(parent));
        }
    }
    Layer layer;
    layer.name = name;
    layer.color = utilityTypeColour(type);
    layer.linetype = std::string(linetype);
    layers.insert_or_assign(name, std::move(layer));
}

double lengthAt(const DrawnUtilityLine& line, sub::QualityLevel level)
{
    return line.lengthAt[static_cast<std::size_t>(level)];
}

// A value in a reply record: as it is when it is one plain word, else in
// double quotes with '"' and '\' escaped, so a record still splits on blanks.
std::string recordValue(std::string_view value)
{
    const bool plain =
        !value.empty() && value.find_first_of(" \t\"=\\\n\r") == std::string_view::npos;
    if (plain) {
        return std::string(value);
    }
    std::string out = "\"";
    for (const char c : value) {
        if (c == '"' || c == '\\') {
            out += '\\';
        }
        out += c == '\n' || c == '\r' ? ' ' : c;
    }
    return out + "\"";
}

} // namespace

std::string_view utilityTypeWord(sub::UtilityType type)
{
    switch (type) {
    case sub::UtilityType::Unknown:
        return "unknown";
    case sub::UtilityType::Electricity:
        return "electricity";
    case sub::UtilityType::Telecommunications:
        return "telecommunications";
    case sub::UtilityType::Gas:
        return "gas";
    case sub::UtilityType::Water:
        return "water";
    case sub::UtilityType::RecycledWater:
        return "recycled-water";
    case sub::UtilityType::FireService:
        return "fire-service";
    case sub::UtilityType::Sewer:
        return "sewer";
    case sub::UtilityType::Stormwater:
        return "stormwater";
    case sub::UtilityType::Fuel:
        return "fuel";
    case sub::UtilityType::IntelligentTransport:
        return "its";
    case sub::UtilityType::Other:
        return "other";
    }
    return "unknown";
}

// Each is at least 3:1 against the plan view's ground (#1E2329) and, as it
// prints, against white paper - the contrast asked of lines and symbols - which
// UtilityDrawing.EveryTypeColourReadsOnTheScreenAndOnPaper holds them to.
Color utilityTypeColour(sub::UtilityType type)
{
    switch (type) {
    case sub::UtilityType::Water:
        return hex(0x2F, 0x80, 0xED); // blue
    case sub::UtilityType::Electricity:
        return hex(0xE0, 0x70, 0x00); // orange
    case sub::UtilityType::Telecommunications:
        return hex(0xFF, 0xFF, 0xFF); // white: prints black
    case sub::UtilityType::Gas:
        return hex(0xB8, 0x86, 0x0B); // yellow, drawn dark gold
    case sub::UtilityType::RecycledWater:
        return hex(0x9B, 0x59, 0xD0); // lilac
    case sub::UtilityType::FireService:
        return hex(0xE5, 0x39, 0x35); // red
    case sub::UtilityType::Sewer:
        return hex(0xA0, 0x89, 0x6B); // cream, drawn darker
    case sub::UtilityType::Stormwater:
        return hex(0x43, 0xA0, 0x47); // green
    case sub::UtilityType::Fuel:
        return hex(0xB0, 0x60, 0x30); // brown
    case sub::UtilityType::IntelligentTransport:
        return hex(0x00, 0x9E, 0xB0); // cyan
    case sub::UtilityType::Other:
        return hex(0x94, 0x94, 0x94); // grey
    case sub::UtilityType::Unknown:
        return hex(0xD6, 0x3A, 0xD6); // magenta: nobody established what it is
    }
    return hex(0xD6, 0x3A, 0xD6);
}

std::string_view qualityLevelLinetypeName(sub::QualityLevel level)
{
    switch (level) {
    case sub::QualityLevel::A:
        return katana::entity::kContinuousLinetype;
    case sub::QualityLevel::B:
        return "utility-ql-b";
    case sub::QualityLevel::C:
        return "utility-ql-c";
    case sub::QualityLevel::D:
        return "utility-ql-d";
    }
    return katana::entity::kContinuousLinetype;
}

Linetype qualityLevelLinetype(sub::QualityLevel level)
{
    Linetype linetype;
    linetype.name = std::string(qualityLevelLinetypeName(level));
    switch (level) {
    case sub::QualityLevel::A:
        break;
    case sub::QualityLevel::B:
        linetype.description = "AS 5488 QL-B  __ __ __";
        linetype.pattern = {LinetypeElement{1.5}, LinetypeElement{-0.75}};
        break;
    case sub::QualityLevel::C:
        linetype.description = "AS 5488 QL-C  __ . __ .";
        linetype.pattern = {LinetypeElement{1.5}, LinetypeElement{-0.5}, LinetypeElement{0.0},
                            LinetypeElement{-0.5}};
        break;
    case sub::QualityLevel::D:
        linetype.description = "AS 5488 QL-D  . . . .";
        linetype.pattern = {LinetypeElement{0.0}, LinetypeElement{-0.6}};
        break;
    }
    return linetype;
}

std::string qualityLevelLayerName(std::string_view prefix, sub::UtilityType type,
                                  sub::QualityLevel level)
{
    return std::format("{}/{}/{}", prefix, utilityTypeWord(type), sub::toString(level));
}

std::string pointsLayerName(std::string_view prefix, sub::UtilityType type)
{
    return std::format("{}/{}/points", prefix, utilityTypeWord(type));
}

Result<UtilityDrawing> drawUtilities(const std::vector<sub::UtilityLine>& lines,
                                     const UtilityDrawOptions& options)
{
    if (auto status = katana::entity::validateLayerPath(options.layerPrefix); !status) {
        return makeError(ErrorCode::InvalidArgument,
                         "the layer prefix is not a layer path: " + status.error().message,
                         options.layerPrefix);
    }
    if (lines.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the schedule has no lines to draw");
    }

    UtilityDrawing drawing;
    std::map<std::string, Layer> layers;
    std::set<std::string> drawnOn;
    std::set<sub::QualityLevel> levelsUsed;
    for (const sub::UtilityLine& line : lines) {
        // A line that cannot be graded refuses the whole draw, by its id: a
        // drawing missing one service would look complete.
        const auto refuse = [&line](const katana::core::Error& error) {
            return makeError(error.code,
                             "line " + line.id + " cannot be graded: " + error.message +
                                 "; nothing was drawn",
                             error.context);
        };
        const auto graded = sub::gradeLine(line, options.grading);
        if (!graded) {
            return refuse(graded.error());
        }
        const auto cover = sub::depthOfCover(line, options.minimumCover, options.grading);
        if (!cover) {
            return refuse(cover.error());
        }

        const sub::UtilityType type = line.attributes.type;
        const PropertyMap shared = lineProperties(line);
        DrawnUtilityLine drawn;
        drawn.id = line.id;
        drawn.type = type;
        drawn.lengthAt = graded->lengthAt;
        drawn.length = graded->length();

        // One polyline per maximal run of segments at one level.
        const std::vector<sub::GradedSegment>& segments = graded->segments;
        for (std::size_t first = 0; first < segments.size();) {
            std::size_t last = first;
            double length = segments[first].length;
            while (last + 1 < segments.size() &&
                   segments[last + 1].level == segments[first].level) {
                ++last;
                length += segments[last].length;
            }
            const sub::QualityLevel level = segments[first].level;
            katana::geometry::Polyline2 polyline;
            for (std::size_t v = segments[first].from; v <= segments[last].from + 1; ++v) {
                polyline.vertices.push_back(planPoint(line.vertices[v]));
            }
            if (polyline.length() > 0.0) {
                const std::string layer = qualityLevelLayerName(options.layerPrefix, type, level);
                needLayer(layers, options.layerPrefix, type, layer,
                          qualityLevelLinetypeName(level));
                drawnOn.insert(layer);
                levelsUsed.insert(level);
                Entity run;
                run.geometry = std::move(polyline);
                run.layer = layer;
                run.properties = shared;
                PropertyMap& properties = run.properties;
                properties.insert_or_assign(std::string(keys::kQualityLevel),
                                            std::string(sub::toString(level)));
                setText(properties, keys::kLimitedBy, limitedBy(segments, first, last));
                properties.insert_or_assign(std::string(keys::kLength), length);
                setText(properties, keys::kFrom, line.vertices[segments[first].from].id);
                setText(properties, keys::kTo, line.vertices[segments[last].from + 1].id);
                drawing.entities.push_back(std::move(run));
                ++drawn.polylines;
            }
            first = last + 1;
        }

        const std::string pointsLayer = pointsLayerName(options.layerPrefix, type);
        needLayer(layers, options.layerPrefix, type, pointsLayer,
                  katana::entity::kContinuousLinetype);
        drawnOn.insert(pointsLayer);
        for (std::size_t i = 0; i < line.vertices.size(); ++i) {
            drawing.entities.push_back(vertexPoint(line, i, graded->vertices[i], (*cover)[i],
                                                   options, pointsLayer));
            drawing.bounds.expand(planPoint(line.vertices[i]));
        }
        drawing.vertices += line.vertices.size();
        drawing.segments += segments.size();
        drawing.lines.push_back(std::move(drawn));
    }

    for (auto& entry : layers) {
        drawing.layers.push_back(std::move(entry.second));
    }
    // Best evidence first, as the report lists the levels.
    for (const sub::QualityLevel level : {sub::QualityLevel::B, sub::QualityLevel::C,
                                          sub::QualityLevel::D}) {
        if (levelsUsed.contains(level)) {
            drawing.linetypes.push_back(qualityLevelLinetype(level));
        }
    }
    drawing.drawnLayers = drawnOn.size();
    return drawing;
}

cmd::CommandPtr utilityDrawCommand(const katana::entity::Model& model,
                                   const UtilityDrawing& drawing)
{
    auto transaction = std::make_unique<cmd::Transaction>(std::string(kUtilityDrawStep));
    // A linetype only for a layer that will be made: a layer that is reused
    // keeps its own, and a linetype nothing names is clutter.
    std::set<std::string> named;
    for (const Layer& layer : drawing.layers) {
        if (!model.layers.contains(layer.name)) {
            named.insert(layer.linetype);
        }
    }
    for (const Linetype& linetype : drawing.linetypes) {
        if (named.contains(linetype.name) && !model.linetypes.contains(linetype.name)) {
            transaction->add(cmd::createLinetype(linetype));
        }
    }
    // Parents first, each its own step, so that undo removes each: a layer
    // created with its ancestors missing would leave them behind on undo.
    for (const Layer& layer : drawing.layers) {
        if (!model.layers.contains(layer.name)) {
            transaction->add(cmd::createLayer(layer));
        }
    }
    transaction->add(cmd::createEntities(drawing.entities));
    return transaction;
}

std::string formatUtilityDrawing(const UtilityDrawing& drawing)
{
    std::string reply = std::format(
        "utilities drawn lines={} vertices={} segments={} entities={} layers={} "
        "bounds={:.3f},{:.3f},{:.3f},{:.3f}",
        drawing.lines.size(), drawing.vertices, drawing.segments, drawing.entities.size(),
        drawing.drawnLayers, drawing.bounds.min.x, drawing.bounds.min.y, drawing.bounds.max.x,
        drawing.bounds.max.y);
    for (const DrawnUtilityLine& line : drawing.lines) {
        reply += std::format("\nline id={} type={} length={:.3f} ql_a={:.3f} ql_b={:.3f} "
                             "ql_c={:.3f} ql_d={:.3f}",
                             recordValue(line.id), utilityTypeWord(line.type), line.length,
                             lengthAt(line, sub::QualityLevel::A),
                             lengthAt(line, sub::QualityLevel::B),
                             lengthAt(line, sub::QualityLevel::C),
                             lengthAt(line, sub::QualityLevel::D));
    }
    return reply;
}

} // namespace katana::cad::utilities
