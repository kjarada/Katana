#pragma once

// Narrowing a query to the entities that could possibly answer it
// (PLAN.MD Phase 18).
//
// Snapping, picking and box selection all begin the same way: take a reach box
// around the cursor or the selection window and consider only the entities
// whose extent meets it. Before Phase 18 that was a scan of every entity; with
// an index it is a lookup of the few cells the reach covers.
//
// Both paths live behind this one function so there is exactly one definition
// of "which entities are candidates", and one definition of the box that
// decides it - see queryExtents below, which is the subtle part.

#include <algorithm>
#include <variant>
#include <vector>

#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/model.hpp"
#include "katana/cad/dimension_draw.hpp"
#include "katana/geometry/spatial_index.hpp"

namespace katana::cad {

// `detail` because queryExtents is an implementation decision of the broad
// phase, not a property of an entity that other code should reason about. It
// is public only because the viewport draws through the same narrowing.
namespace detail {

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
[[nodiscard]] inline katana::geometry::Box2 queryExtents(const katana::entity::Model& model,
                                                         const katana::entity::Entity& entity)
{
    if (const auto* arc = std::get_if<katana::geometry::Arc2>(&entity.geometry)) {
        return arc->circle().boundingBox();
    }
    if (const auto* dimension =
            std::get_if<katana::entity::DimensionGeometry>(&entity.geometry)) {
        // A dimension DRAWS far outside what it measures: the label, the
        // arrows and the extension overshoot all sit beyond the measured
        // points, and how far depends on its style. Culling against the plain
        // bounding box drops a dimension whose label is still on screen.
        //
        // buildDimension is called here rather than a margin being guessed at,
        // because a style with a large text height makes any fixed guess wrong.
        // It costs a handful of vector appends per dimension, and only on the
        // scan path - the indexed path stores this box once when the entity
        // changes.
        const auto drawing = buildDimension(*dimension, resolveDimensionStyle(model, entity));
        if (!drawing.extent.empty()) {
            return drawing.extent;
        }
    }
    return katana::entity::boundingBox(entity.geometry);
}

// Whether the index is worth using for a query this wide, or whether the plain
// ordered scan is cheaper.
//
// It is NOT free to ask an index for everything. A query covering the whole
// drawing gathers every id, sorts them, and then looks each one up again,
// against a single ordered walk of the entities. Measured in Release at
// 100 000 entities, by the share of the drawing's AREA on screen:
//
//     area on screen    visible   scan      indexed
//        1 %              900     4060 us     170 us     index 24x better
//        9 %            8 995     4029 us    2586 us     index 1.6x better
//       25 %           25 195     4881 us    4147 us     index 1.2x better
//       49 %           49 000     4426 us    6173 us     SCAN 1.4x better
//      100 %          100 000     4018 us    8068 us     SCAN 2.0x better
//
// The crossover is around 35% of the area, which is the threshold below. A
// zoomed-out repaint is exactly the case that would otherwise get slower, and
// getting slower is not an acceptable price for getting faster elsewhere.
//
// SpatialIndex::bounds() is never shrunk when an entity is removed (that would
// make every delete O(n)), so it can be larger than the data really is. That
// biases this towards the index, by a margin a rebuild removes.
[[nodiscard]] inline bool worthIndexing(const katana::geometry::SpatialIndex& index,
                                        const katana::geometry::Box2& reach)
{
    const katana::geometry::Box2 extent = index.bounds();
    if (extent.empty() || reach.empty()) {
        return !extent.empty(); // nothing indexed: the scan is trivially cheap
    }
    const double total = extent.width() * extent.height();
    if (!(total > 0.0)) {
        return true; // a degenerate extent says nothing; the index cannot be worse
    }
    const double overlapWidth =
        std::min(extent.max.x, reach.max.x) - std::max(extent.min.x, reach.min.x);
    const double overlapHeight =
        std::min(extent.max.y, reach.max.y) - std::max(extent.min.y, reach.min.y);
    if (overlapWidth <= 0.0 || overlapHeight <= 0.0) {
        return true; // the query is off the data entirely; the index says so at once
    }
    constexpr double kScanAboveAreaShare = 0.35;
    return (overlapWidth * overlapHeight) / total < kScanAboveAreaShare;
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
    if (index == nullptr || !worthIndexing(*index, reach)) {
        model.entities.forEach([&](const katana::entity::Entity& entity) {
            if (queryExtents(model, entity).intersects(reach)) {
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
        if (queryExtents(model, *entity).intersects(reach)) {
            visit(*entity);
        }
    }
}

} // namespace detail

} // namespace katana::cad
