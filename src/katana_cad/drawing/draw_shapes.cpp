#include "katana/cad/drawing/draw_shapes.hpp"

#include <cmath>
#include <memory>
#include <utility>

#include "katana/cad/drawing/construction.hpp"
#include "katana/commands/change_set.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/geometry/editing.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad {

namespace cmd = katana::commands;
using katana::core::Result;
using katana::entity::Entity;
using katana::geometry::CurvePolyline2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;

Vec2 endHeading(const CurvePolyline2& path)
{
    if (path.vertices.size() < 2) {
        return Vec2(1.0, 0.0);
    }
    const auto& from = path.vertices[path.vertices.size() - 2];
    const Vec2 chord = (path.vertices.back().position - from.position).normalized();
    // A segment's end tangent is its chord turned by half its sweep.
    const double sweep = 4.0 * std::atan(from.bulge);
    return chord.rotated(0.5 * sweep);
}

double tangentBulge(const CurvePolyline2& path, const Point2& to)
{
    if (path.vertices.empty()) {
        return 0.0;
    }
    const Vec2 chord = to - path.vertices.back().position;
    if (chord.length() <= katana::math::tolerance::kGeometric) {
        return 0.0;
    }
    const Vec2 tangent = endHeading(path);
    const double half = std::atan2(tangent.cross(chord), tangent.dot(chord));
    return katana::geometry::bulgeFromSweep(2.0 * half);
}

Segment2 constructionSegment(const Point2& through, const Vec2& direction, bool ray)
{
    const Point2 far = through + direction * kConstructionReach;
    return ray ? Segment2{through, far} : Segment2{through - direction * kConstructionReach, far};
}

cmd::CommandPtr createConstruction(const katana::entity::Model& model,
                                   cmd::EntityAttributes attributes,
                                   const std::vector<Segment2>& lines, bool ray)
{
    const char* name = ray ? "CREATE_RAY" : "CREATE_XLINE";
    attributes.layer = std::string(kConstructionLayer);
    auto transaction = std::make_unique<cmd::Transaction>(name);
    if (model.layers.find(attributes.layer) == nullptr) {
        katana::entity::Layer layer;
        layer.name = attributes.layer;
        layer.color = katana::entity::Color{0x80, 0x80, 0x80, 0xFF};
        transaction->add(cmd::createLayer(layer));
    }
    std::vector<Entity> entities;
    entities.reserve(lines.size());
    for (const Segment2& line : lines) {
        entities.push_back(drawnEntity(line, attributes));
    }
    transaction->add(createDrawn(name, std::move(entities)));
    return transaction;
}

std::vector<Polyline2> doubleLineSides(const Polyline2& path, double width)
{
    std::vector<Polyline2> out;
    if (path.vertices.size() < 2 || !(width > 0.0)) {
        return out;
    }
    for (const double sign : {1.0, -1.0}) {
        if (auto side = katana::geometry::offset(path, sign * 0.5 * width)) {
            out.push_back(std::move(*side));
        }
    }
    return out;
}

Entity drawnEntity(katana::entity::Geometry geometry, const cmd::EntityAttributes& attributes)
{
    Entity entity;
    entity.geometry = std::move(geometry);
    entity.layer = attributes.layer;
    entity.style = attributes.style;
    entity.color = attributes.color;
    return entity;
}

cmd::CommandPtr createDrawn(std::string name, std::vector<Entity> entities)
{
    return std::make_unique<cmd::ChangeSetCommand>(
        std::move(name),
        [entities = std::move(entities)](const cmd::CommandContext&) -> Result<cmd::ChangeSet> {
            cmd::ChangeSet changes;
            changes.add = entities;
            return changes;
        });
}

} // namespace katana::cad
