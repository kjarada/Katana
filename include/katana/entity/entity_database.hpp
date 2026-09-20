#pragma once

// Owning container of entities (PLAN.MD Phase 05).
//
// Identity   Ids start at 1, increase monotonically and are never reused, even
//            after removal, so an id recorded by an undo step or an external
//            reference can never silently point at a different entity.
// Ordering   Iteration is by ascending id: deterministic across runs.
// Events     Every mutation reports a ChangeEvent to the single observer, which
//            the owning document uses to feed transactions and views.
// Threading  Not thread-safe; owned and mutated by the document thread only.
// Storage    std::map keeps the first version simple and ordered. Revisit with
//            profiling data in Phase 19 before changing it.

#include <cstddef>
#include <functional>
#include <map>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"

namespace katana::entity {

enum class ChangeKind { EntityAdded, EntityModified, EntityRemoved, Cleared };

struct ChangeEvent {
    ChangeKind kind = ChangeKind::EntityModified;
    EntityId id = kInvalidEntityId; // kInvalidEntityId for Cleared
};

class EntityDatabase {
  public:
    using Observer = std::function<void(const ChangeEvent&)>;

    // Validates the geometry, assigns a fresh id (ignoring entity.id) and stores it.
    [[nodiscard]] katana::core::Result<EntityId> add(Entity entity);

    // Stores an entity under the id it already carries: undo of a removal and
    // loading from storage. Fails when the id is invalid or taken.
    [[nodiscard]] katana::core::Status insert(Entity entity);

    // Replaces the stored entity that has entity.id.
    [[nodiscard]] katana::core::Status replace(Entity entity);

    // Removes and returns the entity so the caller can restore it later.
    [[nodiscard]] katana::core::Result<Entity> remove(EntityId id);

    void clear();

    [[nodiscard]] const Entity* find(EntityId id) const;
    [[nodiscard]] bool contains(EntityId id) const { return find(id) != nullptr; }
    [[nodiscard]] std::size_t size() const { return entities_.size(); }
    [[nodiscard]] bool empty() const { return entities_.empty(); }

    [[nodiscard]] std::vector<EntityId> ids() const;
    [[nodiscard]] std::vector<EntityId> idsOnLayer(std::string_view layer) const;
    [[nodiscard]] std::size_t countOnLayer(std::string_view layer) const;

    // Visits entities in ascending id order. The callback must not mutate the database.
    void forEach(const std::function<void(const Entity&)>& visit) const;

    // Union of the bounding boxes of all entities (visible or not).
    [[nodiscard]] katana::geometry::Box2 bounds() const;

    // The id the next add() will assign. Persisted so ids stay unique across sessions.
    [[nodiscard]] EntityId nextId() const { return nextId_; }
    // Only raises the counter; lowering it could resurrect retired ids.
    void reserveIdsBelow(EntityId nextId);

    void setObserver(Observer observer) { observer_ = std::move(observer); }

    // Takes over the entities and the id counter of `other`, leaving THIS
    // database's observer in place, and reports a single Cleared event exactly
    // as clear() does.
    //
    // Loading needs this. A loader must build the contents aside and commit them
    // only once every insertion has succeeded, or a failure halfway leaves the
    // caller's drawing destroyed - but a plain move-assignment would also carry
    // `other`'s (empty) observer across, silently stopping every change
    // notification the Document depends on.
    void adoptContents(EntityDatabase&& other);

  private:
    void notify(ChangeKind kind, EntityId id) const;

    std::map<EntityId, Entity> entities_;
    EntityId nextId_ = 1;
    Observer observer_;
};

} // namespace katana::entity
