#pragma once

// Narrowing a query to the entities that could possibly answer it
// (PLAN.MD Phase 18). Internal to katana_cad.
//
// Snapping, picking and box selection all begin the same way: take a reach box
// around the cursor or the selection window and consider only the entities
// whose extent meets it. Before Phase 18 that was a scan of every entity; with
// an index it is a lookup of the few cells the reach covers.
//
// Both paths live behind this one function so there is exactly one definition
// of "which entities are candidates", and one definition of the box that
// decides it - see queryExtents below, which is the subtle part.

#include <variant>
#include <vector>

#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/spatial_index.hpp"

namespace katana::cad::detail {

// The box the spatial index stores for an entity, and the box the broad phase
// tests against.
//
// It is NOT always the geometric bounding box. Snapping an arc offers its
// CENTRE, and the centre of an arc lies outside the arc's own bounding box -
// for a shallow arc, far outside it. Indexing the plain bounding box would
// therefore make the broad phase reject an arc whose centre is directly under
// the cursor, and centre snap on arcs would quietly stop working at exactly
// the moment the index was switched on.
//
// So the indexed box is the widest extent any query needs: the full circle for
// an arc, the bounding box for everything else. It stays a BROAD phase for
// every caller, because each still applies its own exact test - picking tests
// the true bounding box and so does not become clickable at an arc's centre.
[[nodiscard]] inline katana::geometry::Box2 queryExtents(const katana::entity::Entity& entity)
{
    if (const auto* arc = std::get_if<katana::geometry::Arc2>(&entity.geometry)) {
        return arc->circle().boundingBox();
    }
    return katana::entity::boundingBox(entity.geometry);
}

// Calls visit(entity) for every entity whose queryExtents meets `reach`, in
// ascending id order.
//
// `index` may be null, in which case every entity is scanned. The two paths
// visit exactly the same entities in the same order - asserted by tests that
// run each query both ways and compare. `scratch` is the caller's reusable
// buffer, so a mouse-move path allocates nothing (PLAN.MD section 33).
template <typename Visit>
void forEachCandidate(const katana::entity::Model& model,
                      const katana::geometry::SpatialIndex* index,
                      const katana::geometry::Box2& reach,
                      std::vector<katana::geometry::SpatialId>& scratch, Visit&& visit)
{
    if (index == nullptr) {
        model.entities.forEach([&](const katana::entity::Entity& entity) {
            if (queryExtents(entity).intersects(reach)) {
                visit(entity);
            }
        });
        return;
    }

    index->query(reach, scratch);
    // The index reports ids ascending and find() is a lookup rather than a
    // scan, so the visit order matches the scanning path exactly. Callers rely
    // on that: picking resolves a tie by preferring the entity visited later.
    for (const katana::geometry::SpatialId id : scratch) {
        const katana::entity::Entity* entity =
            model.entities.find(static_cast<katana::entity::EntityId>(id));
        if (entity == nullptr) {
            // The index has outlived an entity. That is a defect in whatever
            // maintains it, but a stale id must not be dereferenced, and
            // skipping it degrades to a narrower answer rather than crashing.
            continue;
        }
        if (queryExtents(*entity).intersects(reach)) {
            visit(*entity);
        }
    }
}

} // namespace katana::cad::detail
