// The scope and filter of the ROADOBJECT verbs
// (include/katana/cad/road/road_object_scope.hpp, docs/road_objects.md).

#include "katana/cad/road/road_object_scope.hpp"

#include <algorithm>
#include <cstdint>
#include <utility>

#include "katana/cad/selection.hpp"
#include "katana/core/text.hpp"

namespace katana::cad::road {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::EntityId;
using katana::entity::PropertyMap;
using katana::entity::PropertyValue;
using katana::geometry::Box2;
using katana::geometry::Point2;

std::string RoadObjectMatch::record() const
{
    std::string out = scopeRecord(resolved, ids.size());
    if (unmade > 0) {
        out += " unmade=" + std::to_string(unmade);
    }
    return out;
}

Status checkFilterFits(const ModifyFilter& filter)
{
    if (!filter.types.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "TYPE= is an entity's type and a road object is not an entity; its kind "
                         "is PROP=kind:barrier",
                         "TYPE");
    }
    if (filter.style) {
        return makeError(ErrorCode::InvalidArgument,
                         "STYLE= is the style an entity wears and a road object wears none",
                         "STYLE");
    }
    if (filter.text) {
        return makeError(ErrorCode::InvalidArgument,
                         "TEXT= is the text of a label or a dimension and a road object has none",
                         "TEXT");
    }
    return {};
}

std::optional<Box2> planExtent(const PlacementContext& context, const RoadObject& object)
{
    Box2 box;
    for (const bim::BimObject& part : buildRoadObject(context, object).objects) {
        for (const katana::geometry::Point3& vertex : part.mesh.vertices) {
            box.expand(Point2(vertex.x, vertex.y));
        }
    }
    if (box.empty()) {
        return std::nullopt;
    }
    return box;
}

PropertyMap filterProperties(const RoadObject& object)
{
    PropertyMap map;
    const auto text = [&map](const char* key, std::string value) {
        map.insert_or_assign(key, PropertyValue(std::move(value)));
    };
    const auto number = [&map](const char* key, double value) {
        map.insert_or_assign(key, PropertyValue(value));
    };
    const auto whole = [&map](const char* key, std::uint64_t value) {
        map.insert_or_assign(key, PropertyValue(static_cast<std::int64_t>(value)));
    };
    text("id", object.id);
    text("kind", std::string(kindWord(object.kind)));
    if (!object.name.empty()) {
        text("name", object.name);
    }
    text("layer", layerOf(object));
    text("colour", colourOf(object).toHex());
    text("material", materialOf(object));
    if (!object.facility.empty()) {
        text("facility", object.facility);
    }
    const RoadObjectPlacement& placement = object.placement;
    text("placement", std::string(placementWord(placement.kind)));
    if (placement.kind == PlacementKind::Alignment) {
        text("alignment", placement.alignment);
        number("station", placement.station);
        if (placement.to) {
            number("to", *placement.to);
        }
        number("offset", placement.offset);
    } else {
        whole("points", placement.vertices.size());
        if (placement.source != katana::entity::kInvalidEntityId) {
            whole("source", placement.source);
        }
    }
    if (placement.spacing) {
        number("spacing", *placement.spacing);
    }
    text("heights", std::string(heightWord(heightSourceOf(placement))));
    if (!placement.surface.empty()) {
        text("surface", placement.surface);
    }
    number("crossfall", placement.crossfall);
    number("dz", placement.dz);
    // The kind's own keys with the value the object is made with. A key the
    // object takes in common keeps its meaning (insert, not assign).
    for (const auto& [key, value] : effectiveParameters(object)) {
        if (const double* real = std::get_if<double>(&value)) {
            map.emplace(key, PropertyValue(*real));
        } else {
            map.emplace(key, PropertyValue(std::get<std::string>(value)));
        }
    }
    return map;
}

Result<RoadObjectMatch> matchRoadObjects(const Document& document, const ScopeWords& words,
                                         const ScopeViewProvider& views)
{
    // A record that cannot be read is empty to roadObjects(): without this a
    // scope would say it took nothing of objects that are there and kept.
    if (const Status status = document.roadObjectsStatus(); !status) {
        return status.error();
    }
    if (const Status status = checkFilterFits(words.filter); !status) {
        return status.error();
    }
    auto resolved = resolveScope(words, views);
    if (!resolved) {
        return resolved.error();
    }
    const katana::entity::Model& model = document.model();
    if (const Status status = checkScopeLayers(model, resolved->scope); !status) {
        return status.error();
    }

    RoadObjectMatch match;
    match.resolved = std::move(*resolved);
    const ModifyScope& scope = match.resolved.scope;
    ModifyFilter filter = match.resolved.filter;
    // The keys are lower case words; a person types PROP=Kind as readily as
    // PROP=kind, and a key is exact in the shared condition.
    if (filter.property) {
        filter.property = katana::core::lowered(*filter.property);
    }
    const bool readsProperties = filter.property || filter.propertyValue;
    const LayerOverrides& hidden = scope.view != nullptr ? *scope.view : kNoLayerOverrides;
    // The selection, ascending (SelectionSet keeps it ordered).
    const std::vector<EntityId> selected =
        scope.kind == ScopeKind::Selection ? document.selection().ids()
                                           : std::vector<EntityId>{};
    const PlacementContext context = placementContext(document);

    for (const RoadObject& object : document.roadObjects().objects) {
        const std::string layer = layerOf(object);
        switch (scope.kind) {
        case ScopeKind::Selection:
            if (object.placement.source == katana::entity::kInvalidEntityId ||
                !std::ranges::binary_search(selected, object.placement.source)) {
                continue;
            }
            break;
        case ScopeKind::Drawing:
            break;
        case ScopeKind::Layers:
            if (!layerInScope(scope, layer)) {
                continue;
            }
            break;
        case ScopeKind::View:
            if (!isLayerDrawn(model.layers.resolve(layer), layer, hidden)) {
                continue;
            }
            break;
        }
        if (!layerPassesFilter(filter, layer)) {
            continue;
        }
        // An object's own colour is none until colour= gives one: ByLayer here,
        // as for an entity; a colour is the one it is drawn in.
        if (filter.colour) {
            const bool matches = *filter.colour
                                     ? colourOf(object) == **filter.colour
                                     : !object.colour.has_value();
            if (!matches) {
                continue;
            }
        }
        if (readsProperties && !propertiesPassFilter(filter, filterProperties(object))) {
            continue;
        }
        if (filter.drawnOnly &&
            !isLayerDrawn(model.layers.resolve(layer), layer, kNoLayerOverrides)) {
            continue;
        }
        // The plan extent is worked out last: it makes the object.
        const bool needsExtent = scope.kind == ScopeKind::View || filter.drawnOnly;
        if (needsExtent) {
            const std::optional<Box2> extent = planExtent(context, object);
            if (!extent) {
                ++match.unmade;
                continue;
            }
            if (scope.kind == ScopeKind::View && scope.area && !extent->intersects(*scope.area)) {
                continue;
            }
        }
        match.ids.push_back(object.id);
    }
    return match;
}

} // namespace katana::cad::road
