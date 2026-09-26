// Draping the drawing on the ground (drape.hpp).

#include "katana/cad/geo/drape.hpp"

#include <cmath>
#include <memory>
#include <string_view>
#include <utility>
#include <variant>

#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/model.hpp"

namespace katana::cad::geo {

namespace cmd = katana::commands;
using katana::geometry::Point2;

std::vector<Point2> heightVertices(const katana::entity::Entity& entity)
{
    if (const auto* point = std::get_if<katana::entity::PointGeometry>(&entity.geometry)) {
        return {point->position};
    }
    if (const auto* line = std::get_if<katana::geometry::Segment2>(&entity.geometry)) {
        return {line->start, line->end};
    }
    if (const auto* polyline = std::get_if<katana::geometry::Polyline2>(&entity.geometry)) {
        return polyline->vertices;
    }
    return {};
}

DrapeHeights drapeHeights(const std::vector<katana::entity::Entity>& entities,
                          const HeightAt& ground, const std::stop_token& stop)
{
    DrapeHeights out;
    for (const katana::entity::Entity& entity : entities) {
        if (stop.stop_requested()) {
            out.stopped = true;
            return out;
        }
        const std::vector<Point2> vertices = heightVertices(entity);
        if (vertices.empty()) {
            ++out.skipped[katana::core::lowered(std::string(katana::entity::toString(entity.type())))];
            continue;
        }
        DrapedEntity draped;
        draped.id = entity.id;
        draped.heights.reserve(vertices.size());
        bool anyOn = false;
        for (const Point2& vertex : vertices) {
            std::optional<double> height = ground(vertex);
            if (height && !std::isfinite(*height)) {
                height.reset();
            }
            anyOn = anyOn || height.has_value();
            out.verticesOff += height ? 0U : 1U;
            draped.heights.push_back(height);
        }
        out.vertices += vertices.size();
        out.entitiesOff += anyOn ? 0U : 1U;
        out.entities.push_back(std::move(draped));
    }
    return out;
}

DrapeCommand drapeCommand(const katana::entity::Model& model,
                          const std::vector<DrapedEntity>& heights, const std::string& name)
{
    DrapeCommand out;
    auto transaction = std::make_unique<cmd::Transaction>(name.empty() ? std::string("DRAPE") : name);
    for (const DrapedEntity& draped : heights) {
        const katana::entity::Entity* entity = model.entities.find(draped.id);
        if (entity == nullptr) {
            ++out.missing;
            continue;
        }
        // The properties as setHeights would leave them, compared key by key
        // with what the entity has: only a difference becomes an edit, so a
        // second drape on the same ground changes nothing.
        katana::entity::PropertyMap after = entity->properties;
        katana::entity::setHeights(after, draped.heights);
        bool changed = false;
        for (const std::string_view key :
             {katana::entity::kElevationProperty, katana::entity::kElevationsProperty}) {
            const auto before = entity->properties.find(key);
            const auto now = after.find(key);
            if (now != after.end() &&
                (before == entity->properties.end() || before->second != now->second)) {
                transaction->add(cmd::setEntityProperty({entity->id}, std::string(key), now->second));
                changed = true;
            } else if (now == after.end() && before != entity->properties.end()) {
                transaction->add(cmd::removeEntityProperty({entity->id}, std::string(key)));
                changed = true;
            }
        }
        out.changed += changed ? 1U : 0U;
        out.unchanged += changed ? 0U : 1U;
    }
    if (transaction->size() != 0) {
        out.command = std::move(transaction);
    }
    return out;
}

} // namespace katana::cad::geo
