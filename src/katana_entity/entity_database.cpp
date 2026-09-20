#include "katana/entity/entity_database.hpp"

#include <string>
#include <utility>

#include "katana/entity/entity_geometry.hpp"

namespace katana::entity {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

std::string idContext(EntityId id)
{
    return "id=" + std::to_string(id);
}

Status validateEntity(const Entity& entity)
{
    if (entity.layer.empty()) {
        return makeError(ErrorCode::InvalidArgument, "entity layer name is empty");
    }
    return validate(entity.geometry);
}

} // namespace

Result<EntityId> EntityDatabase::add(Entity entity)
{
    if (auto status = validateEntity(entity); !status) {
        return status.error();
    }
    const EntityId id = nextId_++;
    entity.id = id;
    entities_.emplace(id, std::move(entity));
    notify(ChangeKind::EntityAdded, id);
    return id;
}

Status EntityDatabase::insert(Entity entity)
{
    if (entity.id == kInvalidEntityId) {
        return makeError(ErrorCode::InvalidArgument, "cannot insert an entity without an id");
    }
    if (contains(entity.id)) {
        return makeError(ErrorCode::AlreadyExists, "entity id is already in use",
                         idContext(entity.id));
    }
    if (auto status = validateEntity(entity); !status) {
        return status;
    }
    const EntityId id = entity.id;
    reserveIdsBelow(id + 1);
    entities_.emplace(id, std::move(entity));
    notify(ChangeKind::EntityAdded, id);
    return {};
}

Status EntityDatabase::replace(Entity entity)
{
    const auto found = entities_.find(entity.id);
    if (found == entities_.end()) {
        return makeError(ErrorCode::NotFound, "entity does not exist", idContext(entity.id));
    }
    if (auto status = validateEntity(entity); !status) {
        return status;
    }
    const EntityId id = entity.id;
    found->second = std::move(entity);
    notify(ChangeKind::EntityModified, id);
    return {};
}

Result<Entity> EntityDatabase::remove(EntityId id)
{
    const auto found = entities_.find(id);
    if (found == entities_.end()) {
        return makeError(ErrorCode::NotFound, "entity does not exist", idContext(id));
    }
    Entity removed = std::move(found->second);
    entities_.erase(found);
    notify(ChangeKind::EntityRemoved, id);
    return removed;
}

void EntityDatabase::clear()
{
    entities_.clear(); // nextId_ is deliberately kept: ids are never reused
    notify(ChangeKind::Cleared, kInvalidEntityId);
}

void EntityDatabase::adoptContents(EntityDatabase&& other)
{
    entities_ = std::move(other.entities_);
    nextId_ = other.nextId_;
    other.entities_.clear();
    other.nextId_ = 1;
    // observer_ is deliberately NOT touched: it belongs to this database's
    // owner, not to the contents being adopted.
    notify(ChangeKind::Cleared, kInvalidEntityId);
}

const Entity* EntityDatabase::find(EntityId id) const
{
    const auto found = entities_.find(id);
    return found == entities_.end() ? nullptr : &found->second;
}

std::vector<EntityId> EntityDatabase::ids() const
{
    std::vector<EntityId> result;
    result.reserve(entities_.size());
    for (const auto& [id, entity] : entities_) {
        result.push_back(id);
    }
    return result;
}

std::vector<EntityId> EntityDatabase::idsOnLayer(std::string_view layer) const
{
    std::vector<EntityId> result;
    for (const auto& [id, entity] : entities_) {
        if (entity.layer == layer) {
            result.push_back(id);
        }
    }
    return result;
}

std::size_t EntityDatabase::countOnLayer(std::string_view layer) const
{
    std::size_t count = 0;
    for (const auto& [id, entity] : entities_) {
        count += entity.layer == layer ? 1 : 0;
    }
    return count;
}

void EntityDatabase::forEach(const std::function<void(const Entity&)>& visit) const
{
    for (const auto& [id, entity] : entities_) {
        visit(entity);
    }
}

katana::geometry::Box2 EntityDatabase::bounds() const
{
    katana::geometry::Box2 box;
    for (const auto& [id, entity] : entities_) {
        box.expand(boundingBox(entity.geometry));
    }
    return box;
}

void EntityDatabase::reserveIdsBelow(EntityId nextId)
{
    if (nextId > nextId_) {
        nextId_ = nextId;
    }
}

void EntityDatabase::notify(ChangeKind kind, EntityId id) const
{
    if (observer_) {
        observer_(ChangeEvent{kind, id});
    }
}

} // namespace katana::entity
