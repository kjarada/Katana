#pragma once

// Selection set, selection filters and picking (PLAN.MD Phases 08/09).

#include <optional>
#include <set>
#include <string>
#include <vector>

#include "katana/entity/model.hpp"

namespace katana::cad {

using katana::entity::EntityId;

// Restricts which entities picking may return. Default: everything.
struct SelectionFilter {
    std::set<katana::entity::EntityType> types{}; // empty: all types
    std::optional<std::string> layer{};           // empty: all layers

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

// Visible entity on a visible, unlocked layer.
[[nodiscard]] bool isSelectable(const katana::entity::Model& model,
                                const katana::entity::Entity& entity);
// Visible entity on a visible layer (locked layers still draw).
[[nodiscard]] bool isDrawn(const katana::entity::Model& model,
                           const katana::entity::Entity& entity);

// Nearest selectable entity whose geometry lies within `tolerance` (model units)
// of `point`. Ties go to the higher id: the entity drawn last, i.e. on top.
[[nodiscard]] std::optional<EntityId> pickEntity(const katana::entity::Model& model,
                                                 const katana::geometry::Point2& point,
                                                 double tolerance,
                                                 const SelectionFilter& filter = {});

enum class BoxSelectionMode {
    Window,   // only entities entirely inside the box
    Crossing, // entities inside or touching the box
};

[[nodiscard]] std::vector<EntityId> pickInBox(const katana::entity::Model& model,
                                              const katana::geometry::Box2& box,
                                              BoxSelectionMode mode,
                                              const SelectionFilter& filter = {});

} // namespace katana::cad
