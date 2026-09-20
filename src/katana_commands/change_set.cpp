#include "katana/commands/change_set.hpp"

#include <set>
#include <utility>

#include "katana/entity/entity_geometry.hpp"

namespace katana::commands {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::Model;

namespace {

std::string idContext(EntityId id)
{
    return "id=" + std::to_string(id);
}

Status requireUnlockedLayer(const Model& model, const std::string& layerName, EntityId id)
{
    const katana::entity::Layer* layer = model.layers.find(layerName);
    if (layer == nullptr) {
        return makeError(ErrorCode::NotFound, "layer does not exist",
                         "layer=" + layerName + " " + idContext(id));
    }
    if (layer->locked) {
        return makeError(ErrorCode::CommandRejected, "layer is locked",
                         "layer=" + layerName + " " + idContext(id));
    }
    return {};
}

// Rules for an entity as it will exist after the change.
Status validateResultingEntity(const Model& model, const Entity& entity)
{
    if (auto status = katana::entity::validate(entity.geometry); !status) {
        return status;
    }
    if (auto status = requireUnlockedLayer(model, entity.layer, entity.id); !status) {
        return status;
    }
    if (!entity.style.empty() && model.styles.find(entity.style) == nullptr) {
        return makeError(ErrorCode::NotFound, "style does not exist", "style=" + entity.style);
    }
    for (const auto& [key, value] : entity.properties) {
        if (auto status = model.properties.validate(key, value); !status) {
            return status;
        }
    }
    // Metadata is validated too. It is free-form provenance rather than schema'd
    // data, and PropertyDatabase::validate treats an unknown key leniently, but
    // the FINITENESS check has to apply here as well: the serializer writes a
    // non-finite double as JSON null, and the loader rejects null, so a single
    // NaN reaching metadata produces a project that saves without complaint and
    // can then never be opened again. createEntities() takes caller-supplied
    // entities including metadata - that is how an importer builds them - so
    // without this loop the import path could write an unopenable file.
    for (const auto& [key, value] : entity.metadata) {
        if (auto status = model.properties.validate(key, value); !status) {
            return status;
        }
    }
    return {};
}

// Rules for touching an entity that is currently in the model.
Status validateExistingEntity(const Model& model, EntityId id, std::set<EntityId>& seen)
{
    const Entity* current = model.entities.find(id);
    if (current == nullptr) {
        return makeError(ErrorCode::NotFound, "entity does not exist", idContext(id));
    }
    if (!seen.insert(id).second) {
        return makeError(ErrorCode::InvalidArgument,
                         "entity is listed more than once in the same change", idContext(id));
    }
    return requireUnlockedLayer(model, current->layer, id);
}

} // namespace

Status validateChangeSet(const ChangeSet& changes, const Model& model)
{
    std::set<EntityId> seen;
    for (const EntityId id : changes.remove) {
        if (auto status = validateExistingEntity(model, id, seen); !status) {
            return status;
        }
    }
    for (const Entity& entity : changes.modify) {
        if (auto status = validateExistingEntity(model, entity.id, seen); !status) {
            return status;
        }
        if (auto status = validateResultingEntity(model, entity); !status) {
            return status;
        }
    }
    for (const Entity& entity : changes.add) {
        if (auto status = validateResultingEntity(model, entity); !status) {
            return status;
        }
    }
    return {};
}

ChangeSetCommand::ChangeSetCommand(std::string name, Builder builder, bool destructive)
    : name_(std::move(name)), builder_(std::move(builder)), destructive_(destructive)
{
}

Status ChangeSetCommand::validate(const CommandContext& context) const
{
    const auto changes = builder_(context);
    if (!changes) {
        return changes.error();
    }
    if (changes->empty()) {
        return makeError(ErrorCode::CommandRejected, "command would not change anything",
                         std::string(name_));
    }
    return validateChangeSet(*changes, context.model);
}

Status ChangeSetCommand::execute(CommandContext& context)
{
    if (state_ != State::Pending) {
        return makeError(ErrorCode::InvalidState, "command was already executed",
                         std::string(name_));
    }
    auto built = builder_(context);
    if (!built) {
        return built.error();
    }
    ChangeSet& changes = *built;
    if (changes.empty()) {
        return makeError(ErrorCode::CommandRejected, "command would not change anything",
                         std::string(name_));
    }
    if (auto status = validateChangeSet(changes, context.model); !status) {
        return status;
    }

    // Everything was validated, so the mutations below cannot fail for any
    // reason the model knows about. They are still checked: a failure here is a
    // bug, and the partial work is rolled back before reporting it.
    auto& entities = context.model.entities;
    const auto fail = [&](const katana::core::Error& cause) -> Status {
        (void)undo(context); // state_ is Applied below; restores whatever was recorded
        state_ = State::Pending;
        added_.clear();
        before_.clear();
        after_.clear();
        removed_.clear();
        return makeError(ErrorCode::Internal, "validated change could not be applied",
                         cause.describe());
    };
    state_ = State::Applied;

    for (const EntityId id : changes.remove) {
        auto removed = entities.remove(id);
        if (!removed) {
            return fail(removed.error());
        }
        removed_.push_back(std::move(*removed));
    }
    for (Entity& entity : changes.modify) {
        const Entity previous = *entities.find(entity.id);
        if (auto status = entities.replace(entity); !status) {
            return fail(status.error());
        }
        before_.push_back(previous);
        after_.push_back(std::move(entity));
    }
    for (Entity& entity : changes.add) {
        auto id = entities.add(entity);
        if (!id) {
            return fail(id.error());
        }
        entity.id = *id;
        added_.push_back(std::move(entity));
    }
    return {};
}

Status ChangeSetCommand::undo(CommandContext& context)
{
    if (state_ != State::Applied) {
        return makeError(ErrorCode::InvalidState, "command is not applied", std::string(name_));
    }
    auto& entities = context.model.entities;
    Status firstFailure;
    const auto note = [&](Status status) {
        if (!status && firstFailure) {
            firstFailure = std::move(status);
        }
    };
    // Reverse order of execute().
    for (auto it = added_.rbegin(); it != added_.rend(); ++it) {
        auto removed = entities.remove(it->id);
        note(removed ? Status{} : Status{removed.error()});
    }
    for (auto it = before_.rbegin(); it != before_.rend(); ++it) {
        note(entities.replace(*it));
    }
    for (auto it = removed_.rbegin(); it != removed_.rend(); ++it) {
        note(entities.insert(*it));
    }
    state_ = State::Reverted;
    return firstFailure;
}

Status ChangeSetCommand::redo(CommandContext& context)
{
    if (state_ != State::Reverted) {
        return makeError(ErrorCode::InvalidState, "command is not undone", std::string(name_));
    }
    auto& entities = context.model.entities;
    Status firstFailure;
    const auto note = [&](Status status) {
        if (!status && firstFailure) {
            firstFailure = std::move(status);
        }
    };
    for (const Entity& entity : removed_) {
        auto removed = entities.remove(entity.id);
        note(removed ? Status{} : Status{removed.error()});
    }
    for (const Entity& entity : after_) {
        note(entities.replace(entity));
    }
    for (const Entity& entity : added_) {
        note(entities.insert(entity)); // same ids as the first execution
    }
    state_ = State::Applied;
    return firstFailure;
}

std::vector<EntityId> ChangeSetCommand::createdEntities() const
{
    std::vector<EntityId> ids;
    if (state_ == State::Applied) {
        ids.reserve(added_.size());
        for (const Entity& entity : added_) {
            ids.push_back(entity.id);
        }
    }
    return ids;
}

} // namespace katana::commands
