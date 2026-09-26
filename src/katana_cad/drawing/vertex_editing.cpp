#include "katana/cad/drawing/vertex_editing.hpp"

#include <limits>
#include <memory>
#include <utility>

#include "katana/cad/survey_import.hpp"
#include "katana/commands/change_set.hpp"
#include "katana/entity/entity_geometry.hpp"

namespace katana::cad {

namespace cmd = katana::commands;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::CurvePolyline2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

bool isPolylineEntity(const Entity& entity)
{
    return std::holds_alternative<Polyline2>(entity.geometry) ||
           std::holds_alternative<CurvePolyline2>(entity.geometry);
}

std::optional<CurvePolyline2> readPolyline(const Entity& entity)
{
    if (const auto* straight = std::get_if<Polyline2>(&entity.geometry)) {
        return CurvePolyline2::fromPolyline(
            *straight, katana::entity::heightsOf(entity.properties, straight->vertices.size()));
    }
    if (const auto* curved = std::get_if<CurvePolyline2>(&entity.geometry)) {
        return *curved;
    }
    return std::nullopt;
}

Result<Entity> writePolyline(Entity entity, const CurvePolyline2& polyline)
{
    if (polyline.hasArcs()) {
        entity.geometry = polyline;
        // The geometry holds the heights now; a stale property list would be
        // read by anything that asks the properties (the rule has one home).
        katana::entity::setHeights(entity.properties, {});
    } else {
        entity.geometry = Polyline2{polyline.positions(), polyline.closed};
        katana::entity::setHeights(entity.properties, polyline.heights());
    }
    if (auto status = katana::entity::validate(entity.geometry); !status) {
        return status.error();
    }
    return entity;
}

namespace {

Result<Entity> applyEdit(const cmd::CommandContext& context, EntityId id, const PolylineEdit& edit)
{
    const Entity* found = context.model.entities.find(id);
    if (found == nullptr) {
        return makeError(ErrorCode::NotFound, "there is no entity " + std::to_string(id));
    }
    const auto polyline = readPolyline(*found);
    if (!polyline) {
        return makeError(ErrorCode::InvalidArgument,
                         "entity " + std::to_string(id) + " is a " +
                             std::string(katana::entity::toString(found->type())) +
                             ", not a polyline; its vertices cannot be edited");
    }
    auto edited = edit(*polyline);
    if (!edited) {
        return edited.error();
    }
    return writePolyline(*found, *edited);
}

} // namespace

cmd::CommandPtr editPolyline(EntityId id, std::string name, PolylineEdit edit)
{
    return std::make_unique<cmd::ChangeSetCommand>(
        std::move(name),
        [id, edit = std::move(edit)](const cmd::CommandContext& context) -> Result<cmd::ChangeSet> {
            auto entity = applyEdit(context, id, edit);
            if (!entity) {
                return entity.error();
            }
            cmd::ChangeSet changes;
            changes.modify.push_back(std::move(*entity));
            return changes;
        });
}

cmd::CommandPtr editPolylines(std::vector<EntityId> ids, std::string name, PolylineEdit edit)
{
    return std::make_unique<cmd::ChangeSetCommand>(
        std::move(name),
        [ids = std::move(ids), edit = std::move(edit)](
            const cmd::CommandContext& context) -> Result<cmd::ChangeSet> {
            cmd::ChangeSet changes;
            for (const EntityId id : ids) {
                auto entity = applyEdit(context, id, edit);
                if (!entity) {
                    return entity.error();
                }
                changes.modify.push_back(std::move(*entity));
            }
            return changes;
        });
}

cmd::CommandPtr editEachPolyline(std::vector<EntityId> ids, std::string name, PolylineEditOf edit)
{
    return std::make_unique<cmd::ChangeSetCommand>(
        std::move(name),
        [ids = std::move(ids), edit = std::move(edit)](
            const cmd::CommandContext& context) -> Result<cmd::ChangeSet> {
            cmd::ChangeSet changes;
            for (const EntityId id : ids) {
                auto entity = applyEdit(context, id, [&](const CurvePolyline2& polyline) {
                    return edit(context.model, id, polyline);
                });
                if (!entity) {
                    return entity.error();
                }
                changes.modify.push_back(std::move(*entity));
            }
            return changes;
        });
}

std::vector<bool> verticesOnSurveyPoints(const Document& document, const CurvePolyline2& polyline,
                                         double tolerance)
{
    return verticesOnSurveyPoints(document.model(), polyline, tolerance);
}

std::vector<bool> verticesOnSurveyPoints(const katana::entity::Model& model,
                                         const CurvePolyline2& polyline, double tolerance)
{
    std::vector<bool> keep(polyline.vertices.size(), false);
    const std::string property = SurveyImportOptions{}.pointNumberProperty;
    std::vector<Point2> marks;
    model.entities.forEach([&](const Entity& entity) {
        const auto* point = std::get_if<katana::entity::PointGeometry>(&entity.geometry);
        if (point != nullptr && entity.properties.contains(property)) {
            marks.push_back(point->position);
        }
    });
    for (std::size_t i = 0; i < polyline.vertices.size(); ++i) {
        for (const Point2& mark : marks) {
            if (mark.distanceTo(polyline.vertices[i].position) <= tolerance) {
                keep[i] = true;
                break;
            }
        }
    }
    return keep;
}

std::optional<double> heightAtPoint(const Document& document, const Point2& at, double tolerance)
{
    std::optional<double> best;
    double bestDistance = std::numeric_limits<double>::infinity();
    const auto offer = [&](const Point2& p, const std::optional<double>& height) {
        const double d = p.distanceTo(at);
        if (height && d <= tolerance && d < bestDistance) {
            bestDistance = d;
            best = height;
        }
    };
    std::vector<katana::geometry::SpatialId> ids;
    document.spatialIndex().query(
        katana::geometry::Box2(at, at).inflated(std::max(tolerance, 1.0e-9)), ids);
    for (const auto id : ids) {
        const Entity* entity = document.model().entities.find(static_cast<EntityId>(id));
        if (entity == nullptr) {
            continue;
        }
        if (const auto* point = std::get_if<katana::entity::PointGeometry>(&entity->geometry)) {
            offer(point->position, katana::entity::heightsOf(entity->properties, 1)[0]);
        } else if (const auto* line = std::get_if<katana::geometry::Segment2>(&entity->geometry)) {
            const auto heights = katana::entity::heightsOf(entity->properties, 2);
            offer(line->start, heights[0]);
            offer(line->end, heights[1]);
        } else if (const auto polyline = readPolyline(*entity)) {
            for (const auto& vertex : polyline->vertices) {
                offer(vertex.position, vertex.height);
            }
        }
    }
    return best;
}

} // namespace katana::cad
