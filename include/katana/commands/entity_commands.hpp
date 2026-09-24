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
// Renames a property on every entity that has it, keeping its value and its
// type. Refuses a target the entity already has: the two values differ, and
// silently keeping one of them would lose the other. An archive import
// names properties after the attribute tree it flattened ("Asset/Dimensions/Size"),
// which is the first thing a user wants to tidy.
[[nodiscard]] CommandPtr renameEntityProperty(std::vector<EntityId> ids, std::string from,
                                              std::string to);
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

// ---- the table managers -------------------------------------------------------------
//
// Delete, merge and purge discard definitions and say so (isDestructive).
// Every in-use refusal reads entity::tableUsage and gives a count and the
// first holder: "used by 1 layer, 2 styles and 418 entities, e.g.
// layer=survey".

// ---- linetypes -------------------------------------------------------------------
[[nodiscard]] CommandPtr createLinetype(katana::entity::Linetype linetype);
[[nodiscard]] CommandPtr updateLinetype(katana::entity::Linetype linetype);
// nullptr when `linetype` equals the stored one - saving an unedited form is
// not an edit and must push no undo step. A missing name still gets a
// command, whose validate() says NotFound.
[[nodiscard]] CommandPtr updateLinetypeIfChanged(const katana::entity::Model& model,
                                                 katana::entity::Linetype linetype);
// Refuses while any layer or style still names it, and refuses "continuous".
[[nodiscard]] CommandPtr deleteLinetype(std::string name);
// Renames it and repoints every layer and style that named it, as one undo
// step. Refuses an existing target name - two linetypes of the same name
// would have to be merged, and picking one definition would change how
// everything wearing the other draws. mergeLinetype is that, said on purpose.
[[nodiscard]] CommandPtr renameLinetype(std::string from, std::string to);
// Repoints every layer and style naming `from` to `into`, then deletes
// `from`: one undo step restores both. Refuses "continuous" as `from`, and
// an `into` that is not a linetype in the model (a library linestyle is not
// one: the commands cannot see the library).
[[nodiscard]] CommandPtr mergeLinetype(std::string from, std::string into);
// A copy of `from` named `to`.
[[nodiscard]] CommandPtr duplicateLinetype(std::string from, std::string to);

// ---- dimension styles ------------------------------------------------------------
[[nodiscard]] CommandPtr createDimensionStyle(katana::entity::DimensionStyle style);
[[nodiscard]] CommandPtr updateDimensionStyle(katana::entity::DimensionStyle style);
// Refuses while any layer still names it, and refuses "Standard".
[[nodiscard]] CommandPtr deleteDimensionStyle(std::string name);

// ---- hatch patterns --------------------------------------------------------------
[[nodiscard]] CommandPtr createHatchPattern(katana::entity::HatchPattern pattern);
[[nodiscard]] CommandPtr updateHatchPattern(katana::entity::HatchPattern pattern);
// Refuses while any layer or style still names it, and refuses "none".
[[nodiscard]] CommandPtr deleteHatchPattern(std::string name);

// ---- styles ----------------------------------------------------------------------
// A style is how an entity that is not ByLayer is drawn: linetype, weight,
// colour, hatch and - for a point - its symbol. Delete refuses while any
// entity still names it. Names are what a library linestyle arrives as.
[[nodiscard]] CommandPtr createStyle(katana::entity::Style style);
[[nodiscard]] CommandPtr updateStyle(katana::entity::Style style);
// nullptr when `style` equals the stored one; see updateLinetypeIfChanged.
[[nodiscard]] CommandPtr updateStyleIfChanged(const katana::entity::Model& model,
                                              katana::entity::Style style);
[[nodiscard]] CommandPtr deleteStyle(std::string name);
// Renames it and repoints every entity that wore it, as one undo step.
// An archive import names a style after a linestyle ("TOPO Natural Surface
// Point"), which is the first thing a user wants to shorten.
[[nodiscard]] CommandPtr renameStyle(std::string from, std::string to);
// Moves every entity wearing `from` onto `into`, then deletes `from`, as one
// undo step - two styles that should have been one. Refuses a missing
// `from` or `into`, and `from == into`.
[[nodiscard]] CommandPtr mergeStyle(std::string from, std::string into);
// A copy of `from` named `to`.
[[nodiscard]] CommandPtr duplicateStyle(std::string from, std::string to);

// ---- purge -----------------------------------------------------------------------
// Items to delete together. Judged as a set: a linetype named only by styles
// in the same set is free.
struct TableItems {
    std::vector<std::string> styles{};
    std::vector<std::string> linetypes{};
    std::vector<std::string> hatchPatterns{};

    [[nodiscard]] bool empty() const
    {
        return styles.empty() && linetypes.empty() && hatchPatterns.empty();
    }
    [[nodiscard]] std::size_t size() const
    {
        return styles.size() + linetypes.size() + hatchPatterns.size();
    }
    friend bool operator==(const TableItems&, const TableItems&) = default;
};
// Deletes them all as ONE undo step ("PurgeTables"). Refuses an empty set -
// an empty undo step is a defect (audit QT-01's shape) - a protected or
// missing name, and anything still used once the set is gone, naming it and
// its first holder. cad::purgeCommand is what finds the set.
[[nodiscard]] CommandPtr purgeTableItems(TableItems items);

// ---- alignments ------------------------------------------------------------------
// Create and update validate the definition by solving it, so an alignment
// that cannot be built is refused naming its PI rather than stored.
[[nodiscard]] CommandPtr createAlignment(katana::entity::Alignment alignment);
[[nodiscard]] CommandPtr updateAlignment(katana::entity::Alignment alignment);
[[nodiscard]] CommandPtr deleteAlignment(std::string name);

} // namespace katana::commands
