#pragma once

// Selection set, selection filters and picking (PLAN.MD Phases 08/09).

#include <optional>
#include <set>
#include <string>
#include <vector>

#include "katana/cad/layer_overrides.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/spatial_index.hpp"

namespace katana::cad {

using katana::entity::EntityId;

// Restricts which entities picking may return. Default: everything.
struct SelectionFilter {
    std::set<katana::entity::EntityType> types{}; // empty: all types
    std::optional<std::string> layer{};           // empty: all layers
    // The view the pick is made in; null for the document rule alone. A layer
    // hidden in that view cannot be picked there, or Delete would erase
    // something the user cannot see (see LayerOverrides).
    const LayerOverrides* view = nullptr;

    [[nodiscard]] bool accepts(const katana::entity::Entity& entity) const;
};

// Ordered (ascending id), duplicate free.
class SelectionSet {
  public:
    void set(std::vector<EntityId> ids);
    void add(EntityId id);
    void remove(EntityId id);
    void toggle(EntityId id);
    void clear() { ids_.clear(); }

    [[nodiscard]] bool contains(EntityId id) const { return ids_.count(id) > 0; }
    [[nodiscard]] bool empty() const { return ids_.empty(); }
    [[nodiscard]] std::size_t size() const { return ids_.size(); }
    [[nodiscard]] std::vector<EntityId> ids() const { return {ids_.begin(), ids_.end()}; }

    // Drops ids that are no longer in the model. Returns true if anything changed.
    bool prune(const katana::entity::EntityDatabase& entities);

  private:
    std::set<EntityId> ids_;
};

// THE visibility rule, and the one every view and every picker asks, so that
// what the plan view hides the 3D view, a section, a surface built from the
// drawing, snapping and selection all hide too (audit MOD-01, REN-03, REN-04).
// A layer counts as hidden or locked when an ANCESTOR is (PLAN.MD 5.1): turning
// off "design" turns off "design/surface/tin1".
//
//
// `view` is the second half: layers hidden in ONE view (LayerOverrides). It has
// no default on purpose, so that every caller states whether it is asking for a
// view or for the document - pass kNoLayerOverrides for the latter. A default
// would let a new consumer silently ignore the view it is drawn in.
//
// Visible entity on a layer that is shown, unlocked and not hidden in `view`.
[[nodiscard]] bool isSelectable(const katana::entity::Model& model,
                                const katana::entity::Entity& entity,
                                const LayerOverrides& view);
// Visible entity on a layer that is shown and not hidden in `view` (a locked
// layer still draws).
[[nodiscard]] bool isDrawn(const katana::entity::Model& model,
                           const katana::entity::Entity& entity, const LayerOverrides& view);
// The same rule for a caller that has already resolved the layer. The viewport
// draws tens of thousands of entities a frame and needs the layer for its
// colour too, so it resolves once and asks this.
[[nodiscard]] bool isDrawn(const katana::entity::ResolvedLayer& layer,
                           const katana::entity::Entity& entity, const LayerOverrides& view);

// The box round everything `view` draws: each entity passing the rule, by its
// true geometry (entity::boundingBox - an arc's own extent, not its circle's).
// Empty when nothing is drawn. What a view's Zoom Extents frames, so a view
// zooms to what it shows rather than to a stray on a layer it hides - the
// point audit REN-03 made for the 3D view, made here for every view.
[[nodiscard]] katana::geometry::Box2 drawnExtent(const katana::entity::Model& model,
                                                 const LayerOverrides& view);

// Nearest selectable entity whose geometry lies within `tolerance` (model units)
// of `point`. Ties go to the higher id: the entity drawn last, i.e. on top.
// `index`, when supplied, narrows the search instead of scanning the model
// (PLAN.MD Phase 18). It must be in step with `model`; Document keeps one that
// is. The answer is identical either way - the index is a broad phase and the
// exact distance test is unchanged - which is asserted by a test comparing the
// two paths.
[[nodiscard]] std::optional<EntityId>
pickEntity(const katana::entity::Model& model, const katana::geometry::Point2& point,
           double tolerance, const SelectionFilter& filter = {},
           const katana::geometry::SpatialIndex* index = nullptr);

enum class BoxSelectionMode {
    Window,   // only entities entirely inside the box
    Crossing, // entities inside or touching the box
};

[[nodiscard]] std::vector<EntityId>
pickInBox(const katana::entity::Model& model, const katana::geometry::Box2& box,
          BoxSelectionMode mode, const SelectionFilter& filter = {},
          const katana::geometry::SpatialIndex* index = nullptr);

} // namespace katana::cad
