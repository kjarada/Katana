#pragma once

// The concrete CAD commands (PLAN.MD Phases 06 and 09). Each factory returns a
// ready-to-execute Command; nothing touches the model until a CommandStack
// executes it. Command names are the stable identifiers the structured command
// API will expose.
//
//   CREATE_POINT CREATE_LINE CREATE_CIRCLE CREATE_ARC CREATE_POLYLINE
//   CREATE_TEXT CREATE_DIMENSION CREATE_ENTITIES
//   DELETE MOVE ROTATE SCALE MIRROR COPY ARRAY
//   TRIM EXTEND OFFSET FILLET CHAMFER
//   SET_LAYER SET_COLOR SET_STYLE SET_VISIBLE SET_PROPERTY REMOVE_PROPERTY
//   SET_GEOMETRY
//   CREATE_LAYER UPDATE_LAYER DELETE_LAYER

#include <optional>
#include <string>
#include <vector>

#include "katana/commands/command.hpp"
#include "katana/math/mat3.hpp"

namespace katana::commands {

using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::geometry::Vec2;

// Presentation attributes given to newly created entities.
struct EntityAttributes {
    std::string layer = "0";
    std::string style{};
    std::optional<katana::entity::Color> color{};
};

// ---- creation ----------------------------------------------------------------
[[nodiscard]] CommandPtr createPoint(const Point2& position, EntityAttributes attributes = {});
[[nodiscard]] CommandPtr createLine(const Point2& start, const Point2& end,
                                    EntityAttributes attributes = {});
[[nodiscard]] CommandPtr createCircle(const Point2& center, double radius,
                                      EntityAttributes attributes = {});
[[nodiscard]] CommandPtr createArc(const katana::geometry::Arc2& arc,
                                   EntityAttributes attributes = {});
[[nodiscard]] CommandPtr createPolyline(katana::geometry::Polyline2 polyline,
                                        EntityAttributes attributes = {});
[[nodiscard]] CommandPtr createText(katana::entity::TextGeometry text,
                                    EntityAttributes attributes = {});
[[nodiscard]] CommandPtr createDimension(katana::entity::DimensionGeometry dimension,
                                         EntityAttributes attributes = {});
// Adds fully specified entities (imports, paste). Their ids are ignored.
[[nodiscard]] CommandPtr createEntities(std::vector<katana::entity::Entity> entities);

// ---- removal / transformation --------------------------------------------------
[[nodiscard]] CommandPtr deleteEntities(std::vector<EntityId> ids);
[[nodiscard]] CommandPtr moveEntities(std::vector<EntityId> ids, const Vec2& delta);
[[nodiscard]] CommandPtr rotateEntities(std::vector<EntityId> ids, const Point2& center,
                                        double radians);
// Uniform scale about `center`; factor must be positive and finite.
[[nodiscard]] CommandPtr scaleEntities(std::vector<EntityId> ids, const Point2& center,
                                       double factor);
// Mirror about the line through a and b. keepOriginal adds mirrored copies instead.
[[nodiscard]] CommandPtr mirrorEntities(std::vector<EntityId> ids, const Point2& a,
                                        const Point2& b, bool keepOriginal = false);
[[nodiscard]] CommandPtr copyEntities(std::vector<EntityId> ids, const Vec2& delta);
// Rectangular array: rows x columns copies spaced by `spacing` (the originals
// occupy cell 0,0 and are kept).
[[nodiscard]] CommandPtr arrayEntities(std::vector<EntityId> ids, int rows, int columns,
                                       const Vec2& spacing);
// Any similarity transform.
[[nodiscard]] CommandPtr transformEntities(std::string name, std::vector<EntityId> ids,
                                           const katana::math::Mat3& transform);

// ---- curve editing (targets must be Line, Arc or Circle entities) ---------------
// Cutters / boundaries may also be polylines; each of their segments is used.
[[nodiscard]] CommandPtr trimEntity(EntityId target, std::vector<EntityId> cutters,
                                    const Point2& pick);
[[nodiscard]] CommandPtr extendEntity(EntityId target, std::vector<EntityId> boundaries,
                                      const Point2& pick);
// Creates an offset copy on the side of `side`. Also accepts polylines.
[[nodiscard]] CommandPtr offsetEntity(EntityId source, double distance, const Point2& side);
[[nodiscard]] CommandPtr filletEntities(EntityId first, EntityId second, double radius);
[[nodiscard]] CommandPtr chamferEntities(EntityId first, EntityId second, double distanceFirst,
                                         double distanceSecond);

// ---- attributes -----------------------------------------------------------------
[[nodiscard]] CommandPtr setEntityLayer(std::vector<EntityId> ids, std::string layer);
[[nodiscard]] CommandPtr setEntityColor(std::vector<EntityId> ids,
                                        std::optional<katana::entity::Color> color);
[[nodiscard]] CommandPtr setEntityStyle(std::vector<EntityId> ids, std::string style);
[[nodiscard]] CommandPtr setEntityVisible(std::vector<EntityId> ids, bool visible);
[[nodiscard]] CommandPtr setEntityProperty(std::vector<EntityId> ids, std::string key,
                                           katana::entity::PropertyValue value);
[[nodiscard]] CommandPtr removeEntityProperty(std::vector<EntityId> ids, std::string key);
// Replaces the geometry of one entity (property editor, grip editing).
[[nodiscard]] CommandPtr setEntityGeometry(EntityId id, katana::entity::Geometry geometry);

// ---- layers ---------------------------------------------------------------------
[[nodiscard]] CommandPtr createLayer(katana::entity::Layer layer);
// Changes the attributes of the layer named layer.name.
[[nodiscard]] CommandPtr updateLayer(katana::entity::Layer layer);
// Fails while entities still use the layer, and while it has nested layers
// beneath it (use deleteLayerTree for that).
[[nodiscard]] CommandPtr deleteLayer(std::string name);
// Deletes a layer and everything nested under it. Fails when ANY layer in the
// subtree still holds entities - reported with the layer named, so the user is
// told which one rather than left to find it.
[[nodiscard]] CommandPtr deleteLayerTree(std::string name);
// Renames a layer and every layer nested under it, moving the entities on them
// to match. One command, so undo puts back both the tree and the entities.
[[nodiscard]] CommandPtr renameLayer(std::string from, std::string to);

} // namespace katana::commands
