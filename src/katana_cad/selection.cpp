#include "katana/cad/selection.hpp"

#include "katana/cad/spatial_query.hpp"

#include <algorithm>
#include <array>

#include "katana/entity/entity_geometry.hpp"
#include "katana/geometry/intersection.hpp"
#include "katana/geometry/polygon.hpp"

namespace katana::cad {

using katana::entity::Entity;
using katana::entity::Model;
using katana::geometry::Arc2;
using katana::geometry::Box2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;

bool SelectionFilter::accepts(const Entity& entity) const
{
    if (!types.empty() && types.count(entity.type()) == 0) {
        return false;
    }
    return !layer || entity.layer == *layer;
}

void SelectionSet::set(std::vector<EntityId> ids)
{
    ids_ = std::set<EntityId>(ids.begin(), ids.end());
}

void SelectionSet::add(EntityId id)
{
    ids_.insert(id);
}

void SelectionSet::remove(EntityId id)
{
    ids_.erase(id);
}

void SelectionSet::toggle(EntityId id)
{
    if (ids_.erase(id) == 0) {
        ids_.insert(id);
    }
}

bool SelectionSet::prune(const katana::entity::EntityDatabase& entities)
{
    return std::erase_if(ids_, [&](EntityId id) { return !entities.contains(id); }) > 0;
}

bool isDrawn(const Model& model, const Entity& entity, const LayerOverrides& view)
{
    return isDrawn(model.layers.resolve(entity.layer), entity, view);
}

bool isDrawn(const katana::entity::ResolvedLayer& layer, const Entity& entity,
             const LayerOverrides& view)
{
    return entity.visible && layer.shown && !view.hides(entity.layer);
}

bool isSelectable(const Model& model, const Entity& entity, const LayerOverrides& view)
{
    // The drawn test once, here, rather than repeated inline: it used to be
    // written out a second time and the two could drift.
    const katana::entity::ResolvedLayer layer = model.layers.resolve(entity.layer);
    return isDrawn(layer, entity, view) && !layer.locked;
}

namespace {

const LayerOverrides& viewOf(const SelectionFilter& filter)
{
    return filter.view != nullptr ? *filter.view : kNoLayerOverrides;
}

} // namespace

std::optional<EntityId> pickEntity(const Model& model, const Point2& point, double tolerance,
                                   const SelectionFilter& filter,
                                   const katana::geometry::SpatialIndex* index)
{
    std::optional<EntityId> best;
    double bestDistance = tolerance;
    const Box2 reach = Box2(point, point).inflated(tolerance);
    std::vector<katana::geometry::SpatialId> scratch;
    detail::forEachCandidate(model, index, reach, scratch, [&](const Entity& entity) {
        // The broad phase uses queryExtents, which is wider than the bounding
        // box for an arc so that snapping can reach its centre. Picking must
        // NOT be that generous - clicking an arc's centre should not select it
        // - so the true bounding box is still tested here.
        if (!isSelectable(model, entity, viewOf(filter)) || !filter.accepts(entity) ||
            !katana::entity::boundingBox(entity.geometry).intersects(reach)) {
            return;
        }
        const double distance = katana::entity::distanceTo(entity.geometry, point);
        if (distance <= bestDistance) { // <=: later (higher id, drawn on top) wins ties
            bestDistance = distance;
            best = entity.id;
        }
    });
    return best;
}

namespace {

std::array<Segment2, 4> edgesOf(const Box2& box)
{
    const Point2 a = box.min;
    const Point2 b(box.max.x, box.min.y);
    const Point2 c = box.max;
    const Point2 d(box.min.x, box.max.y);
    return {Segment2{a, b}, Segment2{b, c}, Segment2{c, d}, Segment2{d, a}};
}

// True when the drawn geometry has at least one point inside or on the box.
struct TouchesBox {
    const Box2& box;

    bool operator()(const katana::entity::PointGeometry& g) const { return box.contains(g.position); }
    bool operator()(const Segment2& g) const { return katana::geometry::clip(g, box).has_value(); }
    bool operator()(const Polyline2& g) const
    {
        for (std::size_t i = 0; i < g.segmentCount(); ++i) {
            if (katana::geometry::clip(g.segment(i), box)) {
                return true;
            }
        }
        return false;
    }
    bool operator()(const Circle2& g) const
    {
        if (box.contains(g.pointAtAngle(0.0))) {
            return true; // some point of the curve is inside
        }
        return crossesAnEdge(g);
    }
    bool operator()(const Arc2& g) const
    {
        return box.contains(g.startPoint()) || box.contains(g.endPoint()) || crossesAnEdge(g);
    }
    // Text and dimensions are picked by their extents.
    bool operator()(const katana::entity::TextGeometry& g) const
    {
        return katana::entity::boundingBox(g).intersects(box);
    }
    bool operator()(const katana::entity::DimensionGeometry& g) const
    {
        return katana::entity::boundingBox(g).intersects(box);
    }

    template <typename Curve> bool crossesAnEdge(const Curve& curve) const
    {
        const auto edges = edgesOf(box);
        return std::any_of(edges.begin(), edges.end(), [&](const Segment2& edge) {
            return katana::geometry::intersect(edge, curve).exists();
        });
    }
};

} // namespace

std::vector<EntityId> pickInBox(const Model& model, const Box2& box, BoxSelectionMode mode,
                                const SelectionFilter& filter,
                                const katana::geometry::SpatialIndex* index)
{
    std::vector<EntityId> picked;
    if (box.empty()) {
        return picked;
    }
    std::vector<katana::geometry::SpatialId> scratch;
    detail::forEachCandidate(model, index, box, scratch, [&](const Entity& entity) {
        if (!isSelectable(model, entity, viewOf(filter)) || !filter.accepts(entity)) {
            return;
        }
        const Box2 extents = katana::entity::boundingBox(entity.geometry);
        const bool inside = box.contains(extents);
        const bool selected =
            inside || (mode == BoxSelectionMode::Crossing && extents.intersects(box) &&
                       std::visit(TouchesBox{box}, entity.geometry));
        if (selected) {
            picked.push_back(entity.id);
        }
    });
    return picked;
}

} // namespace katana::cad
